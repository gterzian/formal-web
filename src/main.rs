use clap::{Parser, Subcommand};
use log::error;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::ExitCode;
use verification::run_validation_from_iter;

#[derive(Parser, Debug)]
#[command(name = "formal-web")]
#[command(about = "Convenient repository development entrypoint")]
struct Cli {
    #[arg(long, alias = "tla", global = true, default_value_t = false)]
    verify: bool,

    #[arg(long, default_value_t = false)]
    headless: bool,

    #[command(subcommand)]
    command: Option<CommandKind>,
}

#[derive(Subcommand, Debug)]
enum CommandKind {
    #[command(name = "wpt")]
    Wpt {
        #[command(flatten)]
        args: wpt_runner::TestWptArgs,
    },

    #[command(name = "webdriver")]
    WebDriver(automation::WebDriverArgs),

    #[command(name = "cdp")]
    Cdp(automation::CdpArgs),
}

fn delegated_tla_validate_command() -> Option<ExitCode> {
    let args = std::env::args_os().collect::<Vec<_>>();
    if args.get(1).is_none_or(|arg| arg != "validate-tla") {
        return None;
    }

    let forwarded_args = std::iter::once(OsString::from("tla-validate"))
        .chain(args.into_iter().skip(2))
        .collect::<Vec<_>>();
    Some(match run_validation_from_iter(forwarded_args) {
        Ok(exit_code) => exit_code,
        Err(error) => {
            error!("formal-web: {error}");
            ExitCode::from(1)
        }
    })
}

/// If the binary isn't running from inside a .app bundle, assemble one and
/// re-exec from within it. This enables the OS's embedded XPC service discovery
/// so the content process can use XPC without launchd's watchdog.
fn ensure_bundle() {
    let exe = std::env::current_exe().expect("failed to get executable path");
    let exe_path = exe.to_string_lossy();

    // Already in a bundle?
    if exe_path.contains(".app/") {
        return;
    }

    let app_name = "FormalWeb";
    let target_dir = exe.parent().expect("executable has no parent");
    let app_bundle = target_dir.join(format!("{}.app", app_name));
    let macos_dir = app_bundle.join("Contents/MacOS");
    let xpc_dir =
        app_bundle.join("Contents/XPCServices/com.formal-web.app.content.xpc/Contents/MacOS");
    let bundle_exe = macos_dir.join(app_name);

    if bundle_exe.exists() {
        let error = std::process::Command::new(&bundle_exe)
            .args(std::env::args_os().skip(1))
            .exec();
        panic!("failed to re-exec from bundle: {error}");
    }

    // Create the bundle structure.
    std::fs::create_dir_all(&macos_dir).expect("failed to create MacOS dir");
    std::fs::create_dir_all(&xpc_dir).expect("failed to create XPC dir");

    // Copy ourselves into the bundle.
    std::fs::copy(&exe, &bundle_exe).expect("failed to copy binary into bundle");

    // Copy the content binary.
    let content_src = target_dir.join("formal-web-content");
    if content_src.exists() {
        std::fs::copy(&content_src, xpc_dir.join("content"))
            .expect("failed to copy content binary");
    }

    // Symlink net and media helpers.
    let _ = std::os::unix::fs::symlink(
        target_dir.join("formal-web-net"),
        macos_dir.join("formal-web-net"),
    );
    let _ = std::os::unix::fs::symlink(
        target_dir.join("formal-web-media"),
        macos_dir.join("formal-web-media"),
    );
    let _ = std::os::unix::fs::symlink(
        // Relative from MacOS/ to XPCServices/../MacOS/content
        Path::new("../XPCServices/com.formal-web.app.content.xpc/Contents/MacOS/content"),
        macos_dir.join("formal-web-content"),
    );

    // Write Info.plist for the app.
    std::fs::write(
        app_bundle.join("Contents/Info.plist"),
        br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>com.formal-web.app</string>
    <key>CFBundleExecutable</key>
    <string>FormalWeb</string>
</dict>
</plist>
"#,
    )
    .expect("failed to write app Info.plist");

    // Write Info.plist for the XPC service.
    std::fs::create_dir_all(
        app_bundle.join("Contents/XPCServices/com.formal-web.app.content.xpc/Contents"),
    )
    .expect("failed to create XPC Contents dir");
    std::fs::write(
        app_bundle.join(
            "Contents/XPCServices/com.formal-web.app.content.xpc/Contents/Info.plist",
        ),
        br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>com.formal-web.app.content</string>
    <key>CFBundlePackageType</key>
    <string>XPC!</string>
    <key>CFBundleExecutable</key>
    <string>content</string>
    <key>XPCService</key>
    <dict>
        <key>ServiceType</key>
        <string>Application</string>
        <key>MultipleInstances</key>
        <true/>
    </dict>
</dict>
</plist>
"#,
    )
    .expect("failed to write XPC Info.plist");

    // Re-exec from the bundle.
    let error = std::process::Command::new(&bundle_exe)
        .args(std::env::args_os().skip(1))
        .exec();
    panic!("failed to re-exec from bundle: {error}");
}

fn main() -> ExitCode {
    ensure_bundle();
    env_logger::init();
    if let Some(exit_code) = delegated_tla_validate_command() {
        return exit_code;
    }

    let cli = Cli::parse();
    let (wpt_args, command) = match cli.command {
        Some(CommandKind::Wpt { args }) => (Some(args), None),
        other => (None, other),
    };

    if let Some(args) = wpt_args {
        return match wpt_runner::run(args, cli.verify) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                error!("formal-web: {error}");
                ExitCode::from(1)
            }
        };
    }

    let result = match command {
        None => embedder::run_default(cli.verify, cli.headless),
        Some(CommandKind::WebDriver(args)) => {
            embedder::run_webdriver(args, cli.verify, cli.headless)
        }
        Some(CommandKind::Cdp(args)) => embedder::run_cdp(args, cli.verify, cli.headless),
        Some(CommandKind::Wpt { .. }) => Ok(()),
    };

    if let Err(error) = result {
        error!("formal-web: {error}");
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}

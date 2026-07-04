use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const PREBUILD_TARGET_DIR_NAME: &str = "sidecar-prebuild";

fn main() {
    #[allow(unused_mut)]
    let mut prebuild_binaries_list: Vec<(&str, &str)> =
        vec![("content", "formal-web-content"), ("net", "formal-web-net")];

    // Only prebuild the media binary when the media feature is enabled.
    #[cfg(feature = "media")]
    {
        prebuild_binaries_list.push(("media", "formal-web-media"));
    }

    for path in [
        "Cargo.toml",
        "build.rs",
        "src",
        "content/Cargo.toml",
        "content/src",
        "embedder/Cargo.toml",
        "embedder/src",
        "ipc_messages/Cargo.toml",
        "ipc_messages/src",
        "net/Cargo.toml",
        "net/src",
        "user_agent/Cargo.toml",
        "user_agent/src",
        "media/Cargo.toml",
        "media/src",
        "webview/Cargo.toml",
        "webview/src",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }

    if let Err(error) = prebuild_binaries(&prebuild_binaries_list) {
        panic!("failed to prebuild binaries: {error}");
    }
}

fn prebuild_binaries(prebuild_list: &[(&str, &str)]) -> Result<(), String> {
    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR")
            .map_err(|error| format!("missing CARGO_MANIFEST_DIR for build script: {error}"))?,
    );
    let profile = env::var("PROFILE")
        .map_err(|error| format!("missing PROFILE for build script: {error}"))?;
    let target_root = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest_dir.join("target"));
    let prebuild_target_root = target_root.join(PREBUILD_TARGET_DIR_NAME);

    // Do NOT wipe the prebuild target directory. Cargo manages its own
    // cache invalidation and places a `.cargo-lock` file in the target
    // directory for mutual exclusion. If two concurrent builds share the
    // same target dir and one removes it, the other sees "No such file or
    // directory" errors for intermediate compilation artifacts.
    // Cargo will automatically rebuild stale artifacts when features or
    // profiles change. If a genuine resolution conflict arises, clean the
    // two packages with `cargo clean -p content -p net` inside this target
    // dir instead of a full wipe.

    let mut command = Command::new("cargo");
    command.arg("build");
    command.arg("--locked");
    if profile == "release" {
        command.arg("--release");
    }
    // Determine the features to pass to prebuild packages.
    // The content crate has its own backend features (boa/jsc) and media feature.
    let has_media = cfg!(feature = "media");
    let has_boa = cfg!(feature = "boa");
    if has_boa {
        // Boa backend: override content defaults (which use jsc)
        if has_media {
            command.args(["--no-default-features", "--features", "boa,media"]);
        } else {
            command.args(["--no-default-features", "--features", "boa"]);
        }
    } else {
        // JSC backend
        if has_media {
            // Default — content defaults are media+jsc, already correct
        } else {
            // JSC without media
            command.args(["--no-default-features", "--features", "jsc"]);
        }
    }
    command.arg("--target-dir").arg(&prebuild_target_root);
    for (package_name, binary_name) in prebuild_list {
        command
            .arg("-p")
            .arg(package_name)
            .arg("--bin")
            .arg(binary_name);
    }
    command.current_dir(&manifest_dir);

    let status = command.status().map_err(|error| {
        format!("failed to start cargo build for prebuild binaries in {profile} profile: {error}")
    })?;
    if !status.success() {
        return Err(format!(
            "cargo build for prebuild binaries in {profile} profile exited with status {status}"
        ));
    }

    let prebuild_profile_dir = prebuild_target_root.join(&profile);
    let target_profile_dir = target_root.join(&profile);
    fs::create_dir_all(&target_profile_dir).map_err(|error| {
        format!(
            "failed to create target profile directory {}: {error}",
            target_profile_dir.display()
        )
    })?;

    for (_package_name, binary_name) in prebuild_list {
        copy_prebuilt_binary(&prebuild_profile_dir, &target_profile_dir, binary_name)?;
    }

    Ok(())
}

fn copy_prebuilt_binary(
    source_profile_dir: &Path,
    target_profile_dir: &Path,
    binary_name: &str,
) -> Result<(), String> {
    let executable_name = format!("{binary_name}{}", env::consts::EXE_SUFFIX);
    let source_path = source_profile_dir.join(&executable_name);
    if !source_path.is_file() {
        return Err(format!(
            "expected prebuilt executable {} at {}",
            executable_name,
            source_path.display()
        ));
    }

    let target_path = target_profile_dir.join(&executable_name);
    fs::copy(&source_path, &target_path).map_err(|error| {
        format!(
            "failed to copy prebuilt executable from {} to {}: {error}",
            source_path.display(),
            target_path.display()
        )
    })?;

    let permissions = fs::metadata(&source_path)
        .map_err(|error| {
            format!(
                "failed to read prebuilt executable metadata for {}: {error}",
                source_path.display()
            )
        })?
        .permissions();
    fs::set_permissions(&target_path, permissions).map_err(|error| {
        format!(
            "failed to preserve prebuilt executable permissions on {}: {error}",
            target_path.display()
        )
    })?;

    Ok(())
}

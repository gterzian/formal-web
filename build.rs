use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SIDECAR_TARGET_DIR_NAME: &str = "sidecar-prebuild";
const SIDECAR_BINARIES: [(&str, &str); 2] = [
    ("content", "formal-web-content"),
    ("net", "formal-web-net"),
];

fn main() {
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
        "webview/Cargo.toml",
        "webview/src",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }

    prebuild_sidecars().unwrap_or_else(|error| panic!("failed to prebuild sidecars: {error}"));
}

fn prebuild_sidecars() -> Result<(), String> {
    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR")
            .map_err(|error| format!("missing CARGO_MANIFEST_DIR for build script: {error}"))?,
    );
    let profile = env::var("PROFILE")
        .map_err(|error| format!("missing PROFILE for build script: {error}"))?;
    let target_root = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest_dir.join("target"));
    let sidecar_target_root = target_root.join(SIDECAR_TARGET_DIR_NAME);

    let mut command = Command::new("cargo");
    command.arg("build");
    if profile == "release" {
        command.arg("--release");
    }
    command.arg("--target-dir").arg(&sidecar_target_root);
    for (package_name, binary_name) in SIDECAR_BINARIES {
        command
            .arg("-p")
            .arg(package_name)
            .arg("--bin")
            .arg(binary_name);
    }
    command.current_dir(&manifest_dir);

    let status = command.status().map_err(|error| {
        format!(
            "failed to start cargo build for sidecar binaries in {profile} profile: {error}"
        )
    })?;
    if !status.success() {
        return Err(format!(
            "cargo build for sidecar binaries in {profile} profile exited with status {status}"
        ));
    }

    let sidecar_profile_dir = sidecar_target_root.join(&profile);
    let target_profile_dir = target_root.join(&profile);
    fs::create_dir_all(&target_profile_dir).map_err(|error| {
        format!(
            "failed to create target profile directory {}: {error}",
            target_profile_dir.display()
        )
    })?;

    for (_package_name, binary_name) in SIDECAR_BINARIES {
        copy_sidecar_binary(&sidecar_profile_dir, &target_profile_dir, binary_name)?;
    }

    Ok(())
}

fn copy_sidecar_binary(
    source_profile_dir: &Path,
    target_profile_dir: &Path,
    binary_name: &str,
) -> Result<(), String> {
    let executable_name = format!("{binary_name}{}", env::consts::EXE_SUFFIX);
    let source_path = source_profile_dir.join(&executable_name);
    if !source_path.is_file() {
        return Err(format!(
            "expected sidecar executable {} at {}",
            executable_name,
            source_path.display()
        ));
    }

    let target_path = target_profile_dir.join(&executable_name);
    fs::copy(&source_path, &target_path).map_err(|error| {
        format!(
            "failed to copy sidecar executable from {} to {}: {error}",
            source_path.display(),
            target_path.display()
        )
    })?;

    let permissions = fs::metadata(&source_path)
        .map_err(|error| {
            format!(
                "failed to read sidecar executable metadata for {}: {error}",
                source_path.display()
            )
        })?
        .permissions();
    fs::set_permissions(&target_path, permissions).map_err(|error| {
        format!(
            "failed to preserve sidecar executable permissions on {}: {error}",
            target_path.display()
        )
    })?;

    Ok(())
}

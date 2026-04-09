use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn executable_name(stem: &str) -> String {
    if env::consts::EXE_EXTENSION.is_empty() {
        String::from(stem)
    } else {
        format!("{stem}.{}", env::consts::EXE_EXTENSION)
    }
}

fn profile_output_dir(out_dir: &Path) -> PathBuf {
    out_dir
        .ancestors()
        .nth(3)
        .expect("OUT_DIR should be target/<profile>/build/<pkg>/out")
        .to_path_buf()
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=content/Cargo.toml");
    println!("cargo:rerun-if-changed=content/src");
    println!("cargo:rerun-if-changed=lean-toolchain");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR should be set"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR should be set"));
    let profile = env::var("PROFILE").expect("PROFILE should be set");
    let cargo = env::var("CARGO").unwrap_or_else(|_| String::from("cargo"));

    let lean_prefix = Command::new("lean")
        .arg("--print-prefix")
        .output()
        .expect("failed to run `lean --print-prefix`");
    if !lean_prefix.status.success() {
        panic!("`lean --print-prefix` exited with status {}", lean_prefix.status);
    }

    let lean_prefix = String::from_utf8(lean_prefix.stdout)
        .expect("`lean --print-prefix` returned non-UTF-8 output");
    let lean_lib_dir = PathBuf::from(lean_prefix.trim()).join("lib").join("lean");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lean_lib_dir.display());

    let profile_dir = profile_output_dir(&out_dir);
    let target_dir = profile_dir
        .parent()
        .expect("profile directory should have a target parent")
        .to_path_buf();
    let content_target_dir = target_dir.join("formal-web-content");
    let content_manifest = manifest_dir.join("content").join("Cargo.toml");

    let mut command = Command::new(cargo);
    command
        .arg("build")
        .arg("--manifest-path")
        .arg(&content_manifest)
        .arg("--bin")
        .arg("content")
        .env("CARGO_TARGET_DIR", &content_target_dir)
        .current_dir(&manifest_dir);
    if profile == "release" {
        command.arg("--release");
    }

    let status = command.status().expect("failed to build the content child process");
    if !status.success() {
        panic!("building the content child process failed with status {status}");
    }

    let built_content = content_target_dir
        .join(&profile)
        .join(executable_name("content"));
    let copied_content = profile_dir.join(executable_name("content"));
    fs::copy(&built_content, &copied_content).unwrap_or_else(|error| {
        panic!(
            "failed to copy content binary from {} to {}: {error}",
            built_content.display(),
            copied_content.display()
        )
    });
}

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/lean_shim.c");
    println!("cargo:rerun-if-changed=src/macos_compat.m");

    let lean_prefix = Command::new("lean")
        .arg("--print-prefix")
        .output()
        .expect("failed to run `lean --print-prefix`");

    assert!(
        lean_prefix.status.success(),
        "`lean --print-prefix` exited with status {}",
        lean_prefix.status
    );

    let lean_prefix = String::from_utf8(lean_prefix.stdout)
        .expect("`lean --print-prefix` returned non-UTF-8 output");
    let include_dir = PathBuf::from(lean_prefix.trim()).join("include");

    let mut build = cc::Build::new();
    build.file("src/lean_shim.c").include(&include_dir);

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        build.file("src/macos_compat.m");
    }

    build.compile("formalwebffi_lean_shim");

    if let Ok(target) = env::var("TARGET") {
        println!("cargo:warning=building Lean shim for {target}");
    }
}
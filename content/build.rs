//! Content crate build script.
//!
//! Sets `boa_backend` or `jsc_backend` cfg flags based on the target platform.
//! On Apple platforms (macOS, iOS) the default backend is JSC; everywhere
//! else it's Boa.

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    // JSC is only available on Apple platforms.
    if target_os == "macos" || target_os == "ios" || target_os == "tvos" {
        println!("cargo:rustc-cfg=jsc_backend");
    } else {
        println!("cargo:rustc-cfg=boa_backend");
    }

    // Rerun if the target OS changes (e.g., cross-compiling).
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
}

//! Content crate build script.
//!
//! Sets an engine-specific cfg flag based on the active Cargo feature.
//! The engine features are mutually exclusive.

fn main() {
    let has_boa = std::env::var("CARGO_FEATURE_BOA").is_ok();
    let has_v8 = std::env::var("CARGO_FEATURE_V8").is_ok();
    let has_wasm = std::env::var("CARGO_FEATURE_WASM").is_ok();
    let enabled_backend_count = [has_boa, has_v8]
        .into_iter()
        .filter(|enabled| *enabled)
        .count();

    if enabled_backend_count != 1 {
        panic!("exactly one of `boa` or `v8` must be enabled");
    }

    if has_boa {
        println!("cargo:rustc-cfg=boa_backend");
    } else {
        let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
        let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
        if target_arch != "aarch64" || target_os != "macos" {
            panic!(
                "v8 backend currently supports only aarch64-apple-darwin; selected target is {target_arch}-{target_os}"
            );
        }
        if has_wasm {
            panic!("features `v8` and `wasm` cannot be enabled together");
        }
        println!("cargo:rustc-cfg=v8_backend");
    }

    // Rerun if the active feature changes.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_BOA");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_V8");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_WASM");
}

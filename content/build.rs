//! Content crate build script.
//!
//! Sets `boa_backend` or `jsc_backend` cfg flags based on the active
//! Cargo feature (`boa` vs `jsc`).  The features are mutually exclusive.
//!
//! On non-Apple platforms, `jsc` is rejected with a compile error.

fn main() {
    let has_jsc = std::env::var("CARGO_FEATURE_JSC").is_ok();
    let has_boa = std::env::var("CARGO_FEATURE_BOA").is_ok();

    if has_jsc && has_boa {
        panic!("features `boa` and `jsc` are mutually exclusive — enable only one");
    }

    if !has_jsc && !has_boa {
        panic!("one of `boa` or `jsc` feature must be enabled");
    }

    if has_jsc {
        // JSC is only available on Apple platforms.
        let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
        if target_os != "macos" && target_os != "ios" && target_os != "tvos" {
            panic!(
                "jsc backend is only available on Apple platforms (macOS, iOS, tvOS); \
                 use --no-default-features --features boa,media on non-Apple targets"
            );
        }
        println!("cargo:rustc-cfg=jsc_backend");
    } else {
        println!("cargo:rustc-cfg=boa_backend");
    }

    // Rerun if the active feature changes.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_JSC");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_BOA");
}

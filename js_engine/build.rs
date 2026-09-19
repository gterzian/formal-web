fn main() {
    let has_boa = std::env::var("CARGO_FEATURE_BOA").is_ok();
    let has_jsc = std::env::var("CARGO_FEATURE_JSC").is_ok();
    let has_v8 = std::env::var("CARGO_FEATURE_V8").is_ok();
    let enabled_backend_count = [has_boa, has_jsc, has_v8]
        .into_iter()
        .filter(|enabled| *enabled)
        .count();

    if enabled_backend_count != 1 {
        panic!("exactly one of `boa`, `jsc`, or `v8` must be enabled");
    }

    // When the "jsc" feature is enabled, link JavaScriptCore framework and
    // compile the Objective-C `JSManagedValue` wrapper.  The wrapper needs
    // Foundation and ARC; it is only meaningful on Apple targets.
    if has_jsc {
        println!("cargo::rustc-link-lib=framework=JavaScriptCore");
        let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
        if target_os == "macos" || target_os == "ios" {
            cc::Build::new()
                .file("src/jsc_gc_wrapper.m")
                .flag("-fobjc-arc")
                .compile("jsc_gc_wrapper");
            println!("cargo::rustc-link-lib=framework=Foundation");
        } else {
            panic!("the jsc backend requires an Apple target (JSManagedValue is Objective-C)");
        }
        println!("cargo::rerun-if-changed=src/jsc_gc_wrapper.m");
        println!("cargo::rerun-if-changed=src/jsc_gc_wrapper.h");
    }

    if has_v8 {
        let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
        let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
        if target_arch != "aarch64" || target_os != "macos" {
            panic!(
                "v8 backend currently supports only aarch64-apple-darwin; selected target is {target_arch}-{target_os}"
            );
        }
    }
}

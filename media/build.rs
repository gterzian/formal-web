fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Tell rustc about the custom cfg so it doesn't warn.
    println!("cargo::rustc-check-cfg=cfg(avf_default)");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    let is_apple = target_os == "macos" || target_os == "ios";

    // Detect whether the user explicitly selected a backend feature.
    let gst_enabled = std::env::var("CARGO_FEATURE_BACKEND_GSTREAMER").is_ok();
    let avf_enabled = std::env::var("CARGO_FEATURE_BACKEND_AVFOUNDATION").is_ok();

    // When no backend is explicitly selected, pick the platform default:
    //   • Apple  → AVFoundation
    //   • non-Apple → GStreamer (always available, see Cargo.toml)
    if !gst_enabled && !avf_enabled && is_apple {
        println!("cargo:rustc-cfg=avf_default");
    }
}

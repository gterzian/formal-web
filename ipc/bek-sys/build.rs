//! Build script for the BrowserEngineKit Swift shim.
//!
//! BrowserEngineKit is a Swift/Objective-C framework shipped only in the
//! iPhoneOS and iPhoneSimulator SDKs. Its extension-process objects are
//! Swift structs with async initializers — there is no C entry point, so the
//! crate's FFI surface is a Swift file (`swift-shim/BekShim.swift`) compiled
//! here with the real framework in the SDK.
//!
//! Compiling the shim against the real `BrowserEngineKit` module is the
//! design check for the `bek` backend: if a function name, argument type, or
//! closure signature declared in `src/shim.rs` does not match what the
//! framework actually exposes, `swiftc` fails and the `cargo build` for the
//! iOS target fails with it. See `ipc/bek-sys`'s documentation and
//! `ipc/ARCHITECTURE.md` for the validation flow.
//!
//! Only iOS-family targets compile the shim. On any other target (e.g. a
//! macOS build of the workspace that enables `bek` by mistake) the build is
//! a no-op and `src/lib.rs`'s fallback module supplies the same Rust API
//! returning transport errors at runtime.

use std::env;
use std::path::PathBuf;
use std::process::Command;

/// iOS-family Rust triples and the Swift target each one maps to.
/// BEK requires iOS 17.4+, so the Swift side is compiled with that
/// deployment target regardless of the Rust triple's default minimum.
fn swift_target(triple: &str) -> String {
    let os_version = "17.4";
    match triple {
        "aarch64-apple-ios" => format!("arm64-apple-ios{os_version}"),
        "aarch64-apple-ios-sim" => format!("arm64-apple-ios{os_version}-simulator"),
        "x86_64-apple-ios" => format!("x86_64-apple-ios{os_version}-simulator"),
        _ => unreachable!("only iOS targets reach the swift shim build"),
    }
}

fn main() {
    println!("cargo:rerun-if-changed=swift-shim/BekShim.swift");

    let target = env::var("TARGET").expect("cargo sets TARGET for build scripts");
    let (sdk_name, sdk_flag) = match target.as_str() {
        "aarch64-apple-ios" => ("iphoneos", "-sdk"),
        "aarch64-apple-ios-sim" | "x86_64-apple-ios" => ("iphonesimulator", "-sdk"),
        _ => return, // Non-iOS target: no shim. lib.rs provides a fallback API.
    };

    // Resolve the SDK path once; everything else derives from it.
    let sdk_path = Command::new("xcrun")
        .args(["--sdk", sdk_name, "--show-sdk-path"])
        .output()
        .expect("xcrun must be available to build bek-sys (requires Xcode)")
        .stdout;
    let sdk_path = String::from_utf8(sdk_path)
        .expect("xcrun sdk path is not UTF-8")
        .trim()
        .to_string();

    // Where the archive lands, and where the Swift runtime lives for any
    // later final link of a simulator/device binary against the shim.
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let sdk_root = PathBuf::from(&sdk_path);

    let swift_target = swift_target(&target);
    let status = Command::new("xcrun")
        .args([
            sdk_flag,
            sdk_name,
            "swiftc",
            "-target",
            &swift_target,
            "-parse-as-library",
            "-emit-library",
            "-static",
            "-module-name",
            "BekShim",
            "-o",
        ])
        .arg(out_dir.join("libBekShim.a"))
        .arg("swift-shim/BekShim.swift")
        .status()
        .expect("failed to run swiftc for the BrowserEngineKit shim");

    if !status.success() {
        panic!(
            "swiftc failed to compile the BrowserEngineKit shim for {target}: \
             the bek backend's FFI surface does not match the real \
             BrowserEngineKit API in the {sdk_name} SDK"
        );
    }

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=BekShim");
    // Swift runtime for final links of iOS binaries against the shim.
    let swift_lib_dir = sdk_root.join("usr/lib/swift");
    if swift_lib_dir.exists() {
        println!("cargo:rustc-link-search=native={}", swift_lib_dir.display());
    }
}

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
        "media/Cargo.toml",
        "media/src",
        "webview/Cargo.toml",
        "webview/src",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
}

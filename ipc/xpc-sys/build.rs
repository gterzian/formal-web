fn main() {
    // Only compile the XPC wrapper on Apple targets
    #[cfg(target_vendor = "apple")]
    {
        cc::Build::new()
            .file("src/xpc_wrapper.c")
            .flag("-Wall")
            .flag("-Werror")
            .compile("xpc_wrapper");

        println!("cargo:rustc-link-lib=framework=Foundation");
    }

    println!("cargo:rerun-if-changed=src/xpc_wrapper.h");
    println!("cargo:rerun-if-changed=src/xpc_wrapper.c");
}

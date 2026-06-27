fn main() {
    // When the "jsc" feature is enabled, link JavaScriptCore framework
    #[cfg(feature = "jsc")]
    {
        println!("cargo::rustc-link-lib=framework=JavaScriptCore");
    }
}

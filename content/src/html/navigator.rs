//! Navigator identity member values shared by the window Navigator and
//! WorkerNavigator objects: the user agent, platform, and language strings
//! the NavigatorID and NavigatorLanguage mixins expose.  The strings are the
//! embedder's platform identity; there is no per-realm state.

/// <https://html.spec.whatwg.org/#dom-navigator-useragent>
pub(crate) fn navigator_user_agent() -> String {
    String::from("Mozilla/5.0 (formal-web)")
}

/// <https://html.spec.whatwg.org/#dom-navigator-platform>
pub(crate) fn navigator_platform() -> String {
    #[cfg(target_os = "macos")]
    {
        String::from("MacIntel")
    }
    #[cfg(target_os = "linux")]
    {
        String::from("Linux x86_64")
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        String::from("")
    }
}

/// <https://html.spec.whatwg.org/#dom-navigator-language>
pub(crate) fn navigator_language() -> String {
    String::from("en-US")
}

use js_engine::gc_struct;

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

/// <https://html.spec.whatwg.org/#dom-navigator-languages>
pub(crate) fn navigator_languages() -> Vec<String> {
    vec![navigator_language(), String::from("en")]
}

/// <https://html.spec.whatwg.org/#navigator>
#[gc_struct]
pub(crate) struct Navigator {}

impl Navigator {
    pub(crate) fn new() -> Self {
        Self {}
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-appcodename>
    pub(crate) fn app_code_name(&self) -> String {
        // Must return the string "Mozilla".
        String::from("Mozilla")
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-appname>
    pub(crate) fn app_name(&self) -> String {
        // Must return the string "Netscape".
        String::from("Netscape")
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-appversion>
    pub(crate) fn app_version(&self) -> String {
        // Must return either the string "4.0" or a string starting with
        // "5.0 (".
        format!("5.0 ({})", self.oscpu())
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-platform>
    pub(crate) fn platform(&self) -> String {
        // Must return either the empty string or a string representing the
        // platform on which the browser is executing, e.g. "MacIntel",
        // "Win32", "Linux x86_64", "Linux armv81".
        navigator_platform()
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-product>
    pub(crate) fn product(&self) -> String {
        // Must return the string "Gecko".
        String::from("Gecko")
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-productsub>
    pub(crate) fn product_sub(&self) -> String {
        // Must return the appropriate string from the following list:
        // If the navigator compatibility mode is Chrome or WebKit: The
        // string "20030107".
        // If the navigator compatibility mode is Gecko: The string
        // "20100101".
        // Note: The user agent string names neither Chrome nor WebKit, so
        // the navigator compatibility mode is Gecko.
        String::from("20100101")
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-useragent>
    pub(crate) fn user_agent(&self) -> String {
        // Must return the default `User-Agent` value.
        navigator_user_agent()
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-vendor>
    pub(crate) fn vendor(&self) -> String {
        // Must return the appropriate string from the following list:
        // If the navigator compatibility mode is Chrome: The string
        // "Google Inc.".
        // If the navigator compatibility mode is Gecko: The empty string.
        // If the navigator compatibility mode is WebKit: The string "Apple
        // Computer, Inc.".
        String::new()
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-vendorsub>
    pub(crate) fn vendor_sub(&self) -> String {
        // Must return the empty string.
        String::new()
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-taintenabled>
    pub(crate) fn taint_enabled(&self) -> bool {
        // Must return false.
        false
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-oscpu>
    pub(crate) fn oscpu(&self) -> String {
        // Must return either the empty string or a string representing the
        // platform on which the browser is executing, e.g. "Windows NT 10.0;
        // Win64; x64", "Linux x86_64".
        #[cfg(target_os = "macos")]
        {
            String::from("Macintosh")
        }
        #[cfg(target_os = "linux")]
        {
            String::from("Linux x86_64")
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            String::new()
        }
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-language>
    pub(crate) fn language(&self) -> String {
        // Must return a valid BCP 47 language tag representing either a
        // plausible language or the user's most preferred language.
        navigator_language()
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-languages>
    pub(crate) fn languages(&self) -> Vec<String> {
        // Must return a frozen array of valid BCP 47 language tags
        // representing either one or more plausible languages, or the
        // user's preferred languages, ordered by preference with the most
        // preferred language first. The same object must be returned until
        // the user agent needs to return different values, or values in a
        // different order.
        // Note: The frozen array and its identity are the binding layer's:
        // the realm's global scope caches the array this list was converted
        // to.
        navigator_languages()
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-online>
    pub(crate) fn on_line(&self) -> bool {
        // Must return false if the user agent will not contact the network
        // when the user follows links or when a script requests a remote
        // page (or knows that such an attempt would fail), and must return
        // true otherwise.
        true
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-cookieenabled>
    pub(crate) fn cookie_enabled(&self) -> bool {
        // Must return true if the user agent attempts to handle cookies
        // according to HTTP State Management Mechanism, and false if it
        // ignores cookie change requests.
        false
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-hardwareconcurrency>
    pub(crate) fn hardware_concurrency(&self) -> u64 {
        // Must return a number between 1 and the number of logical
        // processors potentially available to the user agent.
        std::thread::available_parallelism()
            .map(|parallelism| parallelism.get() as u64)
            .unwrap_or(1)
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-pdfviewerenabled>
    pub(crate) fn pdf_viewer_enabled(&self) -> bool {
        // The pdfViewerEnabled getter steps are to return the user agent's
        // PDF viewer supported.
        false
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-javaenabled>
    pub(crate) fn java_enabled(&self) -> bool {
        // The javaEnabled() method steps are to return false.
        false
    }
}

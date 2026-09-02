use js_engine::gc_struct;

use super::navigator::{navigator_language, navigator_platform, navigator_user_agent};

/// <https://html.spec.whatwg.org/#the-workernavigator-object>
#[gc_struct]
pub(crate) struct WorkerNavigator {}

impl WorkerNavigator {
    pub(crate) fn new() -> Self {
        Self {}
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-useragent>
    pub(crate) fn user_agent(&self) -> String {
        // The userAgent getter steps are to return this's user agent.
        // Note: The user agent string is reported by the embedder for the
        // window navigator; the worker returns the same value.
        navigator_user_agent()
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-platform>
    pub(crate) fn platform(&self) -> String {
        // The platform getter steps are to return this's platform.
        navigator_platform()
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-language>
    pub(crate) fn language(&self) -> String {
        // The language getter steps are to return this's languages[0].
        navigator_language()
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-online>
    pub(crate) fn on_line(&self) -> bool {
        // The onLine getter steps are to return this's online status.
        true
    }

    /// <https://html.spec.whatwg.org/#dom-navigator-hardwareconcurrency>
    pub(crate) fn hardware_concurrency(&self) -> u64 {
        // The hardwareConcurrency getter steps are to return this's
        // hardware concurrency.
        std::thread::available_parallelism()
            .map(|parallelism| parallelism.get() as u64)
            .unwrap_or(1)
    }
}

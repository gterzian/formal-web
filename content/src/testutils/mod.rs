//! <https://testutils.spec.whatwg.org/#the-testutils-namespace>
//!
//! The `TestUtils` namespace provides testing-only APIs (garbage collection
//! trigger, etc.) exposed on `Window` and `Worker` globals for WPT and
//! developer tooling.

/// <https://testutils.spec.whatwg.org/#the-testutils-namespace>
pub(crate) struct TestUtils;

impl TestUtils {
    /// <https://testutils.spec.whatwg.org/#dom-testutils-gc>
    ///
    /// Perform a garbage collection covering at least the entry realm.
    /// Returns a promise that resolves after GC completes.
    pub(crate) fn gc(
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<<Types as JsTypes>::JsValue, Types> {
        // Step 1: "Let p be a new promise."
        let (promise, resolvers) = ec.new_promise_pending()?;

        // Step 2: "Run the following in parallel:"
        // Note: In a single-threaded content process, garbage collection
        // runs synchronously.  The spec's "in parallel" is approximated
        // by triggering GC immediately and resolving.
        //
        // Step 2.1: "Run implementation-defined steps to perform a
        // garbage collection covering at least the entry Realm."
        ec.gc();

        // Step 2.2: "Resolve p."
        let undefined = ec.value_undefined();
        ec.call(&resolvers.resolve, &undefined, &[])?;

        Ok(promise)
    }
}

use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::Types;

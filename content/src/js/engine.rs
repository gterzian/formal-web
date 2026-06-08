use boa_engine::{
    builtins::object::OrdinaryObject,
    object::NativeObject,
    Context, JsObject, JsResult,
};

// ─────────────────────────────────────────────────────────────────────────
//  JsEngine trait — abstracts the ECMAScript engine operations needed by
//  the Web IDL binding layer and the content process.
// ─────────────────────────────────────────────────────────────────────────

/// A JavaScript engine context providing the core operations that the
/// content process's Web IDL binding layer and HTML/DOM code need.
///
/// This trait wraps the concrete [`Context`] type from `boa_engine` so that
/// the rest of the codebase depends on an abstract interface rather than
/// on Boa directly.  Each method is documented with a link to the relevant
/// ECMAScript or HTML specification algorithm.
pub trait JsEngine {
    // ── Boa context escape hatch ──

    /// Returns a mutable reference to the underlying Boa `Context`.
    ///
    /// This escape hatch exists for operations that have not yet been
    /// abstracted into the trait (e.g. creating `NativeFunction` via
    /// `FunctionObjectBuilder`, registering classes with `ClassBuilder`).
    /// New code should prefer the named trait methods instead.
    fn inner_context_mut(&mut self) -> &mut Context;

    // ── Global object ──

    /// <https://tc39.es/ecma262/#sec-global-object>
    ///
    /// Returns the realm's global object.
    fn global_object(&self) -> JsObject;

    // ── Job queue (microtasks) ──

    /// <https://html.spec.whatwg.org/#perform-a-microtask-checkpoint>
    ///
    /// Runs all pending promise jobs (microtasks).  This corresponds to
    /// the HTML "perform a microtask checkpoint" algorithm.
    fn run_jobs(&mut self) -> JsResult<()>;

    // ── Host-defined data storage (interface registry, etc.) ──

    /// <https://tc39.es/ecma262/#table-realm-record-fields>
    ///
    /// Stores host-defined data on the realm.  Used by the Web IDL binding
    /// layer to register interface prototypes and constructors.
    fn insert_data<D: NativeObject>(&mut self, data: D);

    /// Retrieves host-defined data from the realm.
    fn get_data<D: NativeObject>(&self) -> Option<&D>;

    /// Removes host-defined data from the realm, returning it.
    fn remove_data<D: NativeObject>(&mut self) -> Option<Box<D>>;

    // ── Object construction ──

    /// Creates a new ordinary object with the given prototype.
    ///
    /// <https://tc39.es/ecma262/#sec-ordinaryobjectcreate>
    fn construct_object(&self, prototype: Option<JsObject>) -> JsObject;
}

// ─────────────────────────────────────────────────────────────────────────
//  BoaEngine — wraps boa_engine::Context and implements JsEngine
// ─────────────────────────────────────────────────────────────────────────

/// A concrete [`JsEngine`] implementation wrapping Boa's [`Context`].
///
/// This is the production engine used by the content process.  It delegates
/// all operations to the underlying Boa engine.
pub(crate) struct BoaEngine {
    context: Context,
}

impl BoaEngine {
    /// Creates a new `BoaEngine` from an already-initialized [`Context`].
    pub(crate) fn new(context: Context) -> Self {
        Self { context }
    }

    /// Consumes the engine and returns the inner [`Context`].
    pub(crate) fn into_inner(self) -> Context {
        self.context
    }
}

impl JsEngine for BoaEngine {
    fn inner_context_mut(&mut self) -> &mut Context {
        &mut self.context
    }

    fn global_object(&self) -> JsObject {
        self.context.global_object()
    }

    fn run_jobs(&mut self) -> JsResult<()> {
        self.context.run_jobs()
    }

    fn insert_data<D: NativeObject>(&mut self, data: D) {
        self.context.insert_data(data);
    }

    fn get_data<D: NativeObject>(&self) -> Option<&D> {
        self.context.get_data::<D>()
    }

    fn remove_data<D: NativeObject>(&mut self) -> Option<Box<D>> {
        self.context.remove_data::<D>()
    }

    fn construct_object(&self, prototype: Option<JsObject>) -> JsObject {
        JsObject::from_proto_and_data(prototype, OrdinaryObject)
    }
}

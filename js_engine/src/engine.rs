//! # Core traits: `JsEngine<T>`, `ExecutionContext<T>`, `EcmascriptHost<T>`, `HostHooks<T>`
//!
//! ## `JsEngine<T>` — ECMA-262 engine factory
//!
//! Factory operations: creates realms, built-in functions, evaluates scripts.
//! Used at initialization time only.  Every method maps to a spec-defined
//! abstract operation (§9.3, §10.3, §16).
//!
//! ## `ExecutionContext<T>` — running execution context (§9.4)
//!
//! The runtime handle for ECMA-262 abstract operations that implicitly
//! reference the surrounding agent's running execution context.  This is the
//! type that flows through every binding function, domain method, and dispatch
//! call — it IS the HTML spec's "realm execution context".
//!
//! Operations: §7.1 Type Conversion, §7.2 Testing and Comparison,
//! §7.3 Operations on Objects, §7.4 Iteration, §9.3 currentRealm,
//! §9.6 Jobs, §27.2 Promise operations, value construction.
//!
//! ## `EcmascriptHost<T>` — Web IDL callback operations
//!
//! A narrower interface covering only the ECMA-262 operations that Web IDL
//! callback algorithms need: `Get`, `IsCallable`, `Call`, microtask
//! checkpoint, and exception reporting.  `ExecutionContext<T>` extends this
//! trait.
//!
//! ## `Completion<T, Ty>`
//!
//! `type Completion<T, Ty> = Result<T, <Ty as JsTypes>::JsValue>` —
//! isomorphic to the spec's Completion Record (§6.2.4).
//!
//! ## `HostHooks<T>`
//!
//! Constructor-time configuration for HTML-specific engine hooks
//! (`HostEnsureCanCompileStrings`, `HostPromiseRejectionTracker`, etc.).
//! Set once during engine construction.
//!
//! ## Open problems
//!
//! - **P2: No `JsEngineErased` type.**  `enqueue_job` takes
//!   `Box<dyn FnOnce() + Send>` — the closure can't access the engine.
//!   Fix: create object-safe `JsEngineErased`.
//! - **P3: `create_builtin_function` not yet used by content code.**
//!   Content still uses Boa's `FunctionObjectBuilder` + `NativeFunction`
//!   because converting all native function registrations is a large
//!   mechanical change and the current interface registry stores
//!   `T::JsObject` not `T::Function`.
//! - **P4: `set_host_hooks` is a no-op for Boa.**  Boa host hooks are set
//!   during `ContextBuilder::host_hooks()`, not at runtime.
//! - **P7: `Callback` is Boa-concrete.**  Derives `boa_gc::Trace`/`Finalize`.
//!   Fix requires abstracting GC trait derives.
//!
//! See `js_engine/README.md` for the full philosophy, design notes, and
//! migration plan.

use log::error;

use crate::enums::{
    IntegrityLevel, IteratorKind, PromiseRejectionOperation, SharedMemoryOrder,
    TypedArrayElementType,
};
use crate::records::{IteratorRecord, PromiseCapability, RealmIntrinsics};
use crate::types::{JsTypes, JsTypesWithRealm};
use crate::{Numeric, PreferredType, PropertyDescriptor};

/// The type of a Completion — an ECMAScript abstract operation's result.
///
/// <https://tc39.es/ecma262/#sec-completion-record-specification-type>
///
/// Isomorphic to `Result<T, JsValue>`:
/// - `Ok(v)` corresponds to a normal completion `~v~`.
/// - `Err(e)` corresponds to a throw completion `*e*`.
/// Rust's `?` corresponds to the spec's `?` (ReturnIfAbrupt).
pub type Completion<T, Ty> = Result<T, <Ty as JsTypes>::JsValue>;

// ────────────────────────────────────────────────────────────────────────────
// <https://webidl.spec.whatwg.org/#call-a-user-objects-operation>
// <https://webidl.spec.whatwg.org/#invoke-a-callback-function>
//
// Narrow interface covering only the ECMA-262 operations that Web IDL callback
// algorithms need.
// ────────────────────────────────────────────────────────────────────────────

/// <https://webidl.spec.whatwg.org/#call-a-user-objects-operation>
/// <https://webidl.spec.whatwg.org/#invoke-a-callback-function>
pub trait EcmascriptHost<T: JsTypes> {
    /// <https://tc39.es/ecma262/#sec-get-o-p>
    fn get(&mut self, object: &T::JsObject, property: &str) -> Completion<T::JsValue, T>;

    /// <https://tc39.es/ecma262/#sec-iscallable>
    fn is_callable(&self, value: &T::JsValue) -> bool;

    /// <https://tc39.es/ecma262/#sec-call>
    fn call(
        &mut self,
        callable: &T::JsObject,
        this_arg: &T::JsValue,
        args: &[T::JsValue],
    ) -> Completion<T::JsValue, T>;

    /// <https://html.spec.whatwg.org/#perform-a-microtask-checkpoint>
    fn perform_a_microtask_checkpoint(&mut self) -> Completion<(), T>;

    /// Report an exception thrown from a callback to the host environment.
    fn report_exception(&mut self, error: T::JsValue);

    // ── Value construction — needed by CreateBuiltinFunction closures ────

    /// <https://tc39.es/ecma262/#sec-ecmascript-language-types>
    fn value_undefined(&mut self) -> T::JsValue;

    /// <https://tc39.es/ecma262/#sec-ecmascript-language-types>
    fn value_null(&mut self) -> T::JsValue;

    /// <https://tc39.es/ecma262/#sec-ecmascript-language-types>
    fn value_from_bool(&mut self, b: bool) -> T::JsValue;

    /// <https://tc39.es/ecma262/#sec-ecmascript-language-types>
    fn value_from_number(&mut self, n: f64) -> T::JsValue;

    /// <https://tc39.es/ecma262/#sec-ecmascript-language-types>
    fn value_from_string(&mut self, s: T::JsString) -> T::JsValue;

    /// <https://tc39.es/ecma262/#sec-ecmascript-language-types>
    /// Construct a `JsString` value from a `&str`.
    fn js_string_from_str(&self, s: &str) -> T::JsString;
}

// ────────────────────────────────────────────────────────────────────────────
// <https://tc39.es/ecma262/#sec-execution-contexts>
//
// The running execution context.  Provides all ECMA-262 abstract operations
// that implicitly reference the surrounding agent's running execution context.
// This is the type threaded through every binding function, domain method, and
// dispatch call — it IS the HTML spec's "realm execution context".
// ────────────────────────────────────────────────────────────────────────────

/// <https://tc39.es/ecma262/#sec-execution-contexts>
pub trait ExecutionContext<T: JsTypes + JsTypesWithRealm>: EcmascriptHost<T> {
    // ────────────────────────────────────────────────────────────────────────
    // §7.1 Type Conversion
    // ────────────────────────────────────────────────────────────────────────

    /// <https://tc39.es/ecma262/#sec-toprimitive>
    fn to_primitive(
        &mut self,
        input: T::JsValue,
        preferred_type: Option<PreferredType>,
    ) -> Completion<T::JsValue, T>;

    /// <https://tc39.es/ecma262/#sec-toboolean>
    fn to_boolean(&self, value: &T::JsValue) -> bool;

    /// <https://tc39.es/ecma262/#sec-tonumber>
    fn to_number(&mut self, value: T::JsValue) -> Completion<f64, T>;

    /// <https://tc39.es/ecma262/#sec-tonumeric>
    fn to_numeric(&mut self, value: T::JsValue) -> Completion<Numeric<T>, T>;

    /// <https://tc39.es/ecma262/#sec-toint32>
    fn to_int32(&mut self, value: T::JsValue) -> Completion<i32, T>;

    /// <https://tc39.es/ecma262/#sec-touint32>
    fn to_uint32(&mut self, value: T::JsValue) -> Completion<u32, T>;

    /// <https://tc39.es/ecma262/#sec-toint16>
    fn to_int16(&mut self, value: T::JsValue) -> Completion<i16, T>;

    /// <https://tc39.es/ecma262/#sec-touint16>
    fn to_uint16(&mut self, value: T::JsValue) -> Completion<u16, T>;

    /// <https://tc39.es/ecma262/#sec-toint8>
    fn to_int8(&mut self, value: T::JsValue) -> Completion<i8, T>;

    /// <https://tc39.es/ecma262/#sec-touint8>
    fn to_uint8(&mut self, value: T::JsValue) -> Completion<u8, T>;

    /// <https://tc39.es/ecma262/#sec-touint8clamp>
    fn to_uint8_clamp(&mut self, value: T::JsValue) -> Completion<u8, T>;

    /// <https://tc39.es/ecma262/#sec-tobigint>
    fn to_bigint(&mut self, value: T::JsValue) -> Completion<T::JsBigInt, T>;

    /// <https://tc39.es/ecma262/#sec-stringtobigint>
    fn string_to_bigint(&mut self, string: T::JsString) -> Option<T::JsBigInt>;

    /// <https://tc39.es/ecma262/#sec-tostring>
    fn to_js_string(&mut self, value: T::JsValue) -> Completion<T::JsString, T>;

    /// <https://tc39.es/ecma262/#sec-toobject>
    fn to_object(&mut self, value: T::JsValue) -> Completion<T::JsObject, T>;

    /// <https://tc39.es/ecma262/#sec-topropertykey>
    fn to_property_key(&mut self, value: T::JsValue) -> Completion<T::PropertyKey, T>;

    /// <https://tc39.es/ecma262/#sec-tolength>
    fn to_length(&mut self, value: T::JsValue) -> Completion<u64, T>;

    /// <https://tc39.es/ecma262/#sec-canonicalnumericindexstring>
    fn canonical_numeric_index_string(&self, argument: &T::JsString) -> Option<f64>;

    /// <https://tc39.es/ecma262/#sec-toindex>
    fn to_index(&mut self, value: T::JsValue) -> Completion<u64, T>;

    // ────────────────────────────────────────────────────────────────────────
    // §7.2 Testing and Comparison
    // ────────────────────────────────────────────────────────────────────────

    /// <https://tc39.es/ecma262/#sec-requireobjectcoercible>
    fn require_object_coercible(&mut self, value: T::JsValue) -> Completion<T::JsValue, T>;

    /// <https://tc39.es/ecma262/#sec-isarray>
    fn is_array(&mut self, value: &T::JsValue) -> Completion<bool, T>;

    /// <https://tc39.es/ecma262/#sec-isconstructor>
    fn is_constructor(&self, value: &T::JsValue) -> bool;

    /// <https://tc39.es/ecma262/#sec-isextensible>
    fn is_extensible(&mut self, object: &T::JsObject) -> Completion<bool, T>;

    /// <https://tc39.es/ecma262/#sec-isintegralnumber>
    fn is_integral_number(&self, value: &T::JsValue) -> bool;

    /// <https://tc39.es/ecma262/#sec-ispropertykey>
    fn is_property_key(&self, value: &T::JsValue) -> bool;

    /// <https://tc39.es/ecma262/#sec-samevalue>
    fn same_value(&self, x: &T::JsValue, y: &T::JsValue) -> bool;

    /// <https://tc39.es/ecma262/#sec-samevaluezero>
    fn same_value_zero(&self, x: &T::JsValue, y: &T::JsValue) -> bool;

    /// <https://tc39.es/ecma262/#sec-islooselyequal>
    fn is_loosely_equal(&mut self, x: T::JsValue, y: T::JsValue) -> Completion<bool, T>;

    /// <https://tc39.es/ecma262/#sec-isstrictlyequal>
    fn is_strictly_equal(&self, x: &T::JsValue, y: &T::JsValue) -> bool;

    // ────────────────────────────────────────────────────────────────────────
    // §7.3 Operations on Objects
    // ────────────────────────────────────────────────────────────────────────

    /// <https://tc39.es/ecma262/#sec-get-o-p>
    fn get(
        &mut self,
        object: T::JsObject,
        property_key: T::PropertyKey,
    ) -> Completion<T::JsValue, T>;

    /// <https://tc39.es/ecma262/#sec-getv>
    fn get_v(
        &mut self,
        value: T::JsValue,
        property_key: T::PropertyKey,
    ) -> Completion<T::JsValue, T>;

    /// <https://tc39.es/ecma262/#sec-set-o-p-v-throw>
    fn set(
        &mut self,
        object: T::JsObject,
        property_key: T::PropertyKey,
        value: T::JsValue,
        throw: bool,
    ) -> Completion<(), T>;

    /// <https://tc39.es/ecma262/#sec-createdataproperty>
    fn create_data_property(
        &mut self,
        object: T::JsObject,
        property_key: T::PropertyKey,
        value: T::JsValue,
    ) -> Completion<bool, T>;

    /// <https://tc39.es/ecma262/#sec-definepropertyorthrow>
    fn define_property_or_throw(
        &mut self,
        object: T::JsObject,
        property_key: T::PropertyKey,
        descriptor: PropertyDescriptor<T>,
    ) -> Completion<(), T>;

    /// <https://tc39.es/ecma262/#sec-deletepropertyorthrow>
    fn delete_property_or_throw(
        &mut self,
        object: T::JsObject,
        property_key: T::PropertyKey,
    ) -> Completion<(), T>;

    /// <https://tc39.es/ecma262/#sec-setprototypeof>
    fn set_prototype(
        &mut self,
        object: T::JsObject,
        prototype: Option<T::JsObject>,
    ) -> Completion<bool, T>;

    /// <https://tc39.es/ecma262/#sec-getmethod>
    fn get_method(
        &mut self,
        value: T::JsValue,
        property_key: T::PropertyKey,
    ) -> Completion<Option<T::Function>, T>;

    /// <https://tc39.es/ecma262/#sec-hasproperty>
    fn has_property(
        &mut self,
        object: T::JsObject,
        property_key: T::PropertyKey,
    ) -> Completion<bool, T>;

    /// <https://tc39.es/ecma262/#sec-hasownproperty>
    fn has_own_property(
        &mut self,
        object: T::JsObject,
        property_key: T::PropertyKey,
    ) -> Completion<bool, T>;

    /// <https://tc39.es/ecma262/#sec-construct>
    fn construct(
        &mut self,
        function: T::Constructor,
        args: &[T::JsValue],
        new_target: Option<T::Constructor>,
    ) -> Completion<T::JsObject, T>;

    /// <https://tc39.es/ecma262/#sec-setintegritylevel>
    fn set_integrity_level(
        &mut self,
        object: T::JsObject,
        level: IntegrityLevel,
    ) -> Completion<bool, T>;

    /// <https://tc39.es/ecma262/#sec-testintegritylevel>
    fn test_integrity_level(
        &mut self,
        object: T::JsObject,
        level: IntegrityLevel,
    ) -> Completion<bool, T>;

    /// <https://tc39.es/ecma262/#sec-speciesconstructor>
    fn species_constructor(
        &mut self,
        object: T::JsObject,
        default_constructor: T::Constructor,
    ) -> Completion<T::Constructor, T>;

    // ────────────────────────────────────────────────────────────────────────
    // §7.4 Iteration
    // ────────────────────────────────────────────────────────────────────────

    /// <https://tc39.es/ecma262/#sec-getiterator>
    fn get_iterator(
        &mut self,
        object: T::JsValue,
        kind: IteratorKind,
        method: Option<T::Function>,
    ) -> Completion<IteratorRecord<T>, T>;

    /// <https://tc39.es/ecma262/#sec-iteratorstepvalue>
    fn iterator_step_value(
        &mut self,
        iterator: &mut IteratorRecord<T>,
    ) -> Completion<Option<T::JsValue>, T>;

    /// <https://tc39.es/ecma262/#sec-iteratorclose>
    fn iterator_close(
        &mut self,
        iterator: IteratorRecord<T>,
        completion: Completion<T::JsValue, T>,
    ) -> Completion<T::JsValue, T>;

    /// <https://tc39.es/ecma262/#sec-asynciteratorclose>
    fn async_iterator_close(
        &mut self,
        iterator: IteratorRecord<T>,
        completion: Completion<T::JsValue, T>,
    ) -> Completion<T::JsValue, T>;

    // ────────────────────────────────────────────────────────────────────────
    // §9.3 Realm — runtime access
    // ────────────────────────────────────────────────────────────────────────

    /// <https://tc39.es/ecma262/#sec-execution-contexts>
    fn current_realm(&self) -> T::Realm;

    /// <https://tc39.es/ecma262/#sec-completion-record-specification-type>
    fn realm_intrinsics(&self, realm: &T::Realm) -> RealmIntrinsics<T>;

    // ────────────────────────────────────────────────────────────────────────
    // §9.6 / §9.7 Jobs (microtask queue)
    // ────────────────────────────────────────────────────────────────────────

    /// <https://tc39.es/ecma262/#sec-enqueuejob>
    fn enqueue_job(&mut self, job: Box<dyn FnOnce() + Send>);

    /// <https://tc39.es/ecma262/#sec-runjobs>
    fn run_jobs(&mut self);

    // ────────────────────────────────────────────────────────────────────────
    // §25.1 ArrayBuffer Abstract Operations — runtime queries
    // ────────────────────────────────────────────────────────────────────────

    /// <https://tc39.es/ecma262/#sec-isdetachedbuffer>
    fn is_detached_buffer(&self, array_buffer: &T::ArrayBuffer) -> bool;

    /// <https://tc39.es/ecma262/#sec-isfixedlengtharraybuffer>
    fn is_fixed_length_array_buffer(&self, array_buffer: &T::ArrayBuffer) -> bool;

    /// <https://tc39.es/ecma262/#sec-getvaluefrombuffer>
    fn get_value_from_buffer(
        &self,
        array_buffer: &T::ArrayBuffer,
        byte_index: u64,
        element_type: TypedArrayElementType,
        is_typed_array: bool,
        order: SharedMemoryOrder,
    ) -> T::JsValue;

    /// <https://tc39.es/ecma262/#sec-setvalueinbuffer>
    fn set_value_in_buffer(
        &mut self,
        array_buffer: &T::ArrayBuffer,
        byte_index: u64,
        element_type: TypedArrayElementType,
        value: T::JsValue,
        is_typed_array: bool,
        order: SharedMemoryOrder,
    ) -> Completion<(), T>;

    // ────────────────────────────────────────────────────────────────────────
    // §27.2 Promise Abstract Operations
    // ────────────────────────────────────────────────────────────────────────

    /// <https://tc39.es/ecma262/#sec-promise-resolve>
    fn promise_resolve(
        &mut self,
        constructor: T::Constructor,
        x: T::JsValue,
    ) -> Completion<T::Promise, T>;

    /// <https://tc39.es/ecma262/#sec-newpromisecapability>
    fn new_promise_capability(
        &mut self,
        constructor: T::Constructor,
    ) -> Completion<PromiseCapability<T>, T>;

    /// <https://tc39.es/ecma262/#sec-performpromisethen>
    fn perform_promise_then(
        &mut self,
        promise: T::Promise,
        on_fulfilled: Option<T::Function>,
        on_rejected: Option<T::Function>,
        result_capability: Option<PromiseCapability<T>>,
    ) -> Completion<T::JsValue, T>;

    // ────────────────────────────────────────────────────────────────────────
    // §27.5 Generator Abstract Operations
    // ────────────────────────────────────────────────────────────────────────

    /// <https://tc39.es/ecma262/#sec-generatorstart>
    fn generator_start(
        &mut self,
        generator: T::Generator,
        closure: T::Function,
    ) -> Completion<(), T>;

    // ────────────────────────────────────────────────────────────────────────
    // Global Object Access
    // ────────────────────────────────────────────────────────────────────────

    /// <https://tc39.es/ecma262/#sec-getglobalobject>
    /// Returns the global object of the current realm.
    fn global_object(&self) -> T::JsObject;

    // ────────────────────────────────────────────────────────────────────────
    // Property Key Construction
    // ────────────────────────────────────────────────────────────────────────

    /// Create a `PropertyKey` from a `&str`.  Used by the Web IDL bindings
    /// infrastructure when defining attributes and operations on interface
    /// prototype objects.
    fn property_key_from_str(&self, s: &str) -> T::PropertyKey;

    /// Create a numeric `PropertyKey` from a `u32` index.
    /// Used for array-index access in binding functions (e.g. iterating
    /// a sequence by numeric index).
    fn property_key_from_index(&self, index: u32) -> T::PropertyKey;

    // ────────────────────────────────────────────────────────────────────────
    // Host-Defined Data Store (analogous to boa_engine::Context::get_data/insert_data)
    // ────────────────────────────────────────────────────────────────────────

    /// Store a value of type `T` in the host-defined data store.
    /// Store a value by TypeId (type-erased, object-safe).
    fn store_host_any(&mut self, id: std::any::TypeId, value: Box<dyn std::any::Any>);

    /// Retrieve a reference to a stored value by TypeId.
    fn get_host_any(&self, id: &std::any::TypeId) -> Option<&dyn std::any::Any>;

    /// Remove and return a stored value by TypeId.
    fn remove_host_any(&mut self, id: &std::any::TypeId) -> Option<Box<dyn std::any::Any>>;

    // ── Platform Object Creation ─────────────────────────────────────────

    /// Create a JS object with the given prototype and type-erased Rust data.
    fn create_object_with_any(
        &mut self,
        prototype: T::JsObject,
        data: Box<dyn std::any::Any + 'static>,
    ) -> T::JsObject;

    // ── Error Construction ──────────────────────────────────────────────

    /// <https://tc39.es/ecma262/#sec-native-error-types-used-in-this-standard-typeerror>
    /// Create a new TypeError with the given message.
    fn new_type_error(&mut self, msg: &str) -> T::JsValue;

    /// <https://tc39.es/ecma262/#sec-native-error-types-used-in-this-standard-rangeerror>
    /// Create a new RangeError with the given message.
    fn new_range_error(&mut self, msg: &str) -> T::JsValue;

    // ────────────────────────────────────────────────────────────────────────
    // Error Reporting
    // ────────────────────────────────────────────────────────────────────────

    fn report_error(&mut self, message: &str) {
        error!("unhandled exception: {message}");
    }

    // ────────────────────────────────────────────────────────────────────────
    // String Utilities (bridge engine-specific JsString ↔ Rust String)
    // ────────────────────────────────────────────────────────────────────────

    /// Extract a Rust `String` from an engine-native `JsString`.
    /// Pure operation — does not execute JS code.
    fn js_string_to_rust_string(&self, s: &T::JsString) -> String;

    /// Convenience: apply ECMA-262 `ToString` then extract as Rust `String`.
    /// Combines `to_js_string(value).and_then(|s| Ok(js_string_to_rust_string(&s)))`.
    fn to_rust_string(&mut self, value: T::JsValue) -> Completion<String, T> {
        let js_string = self.to_js_string(value)?;
        Ok(self.js_string_to_rust_string(&js_string))
    }

    // ────────────────────────────────────────────────────────────────────────
    // Array Construction (replaces engine-specific JsArray APIs)
    // ────────────────────────────────────────────────────────────────────────

    /// Create a new, empty JavaScript array.
    fn create_empty_array(&mut self) -> T::JsObject;

    /// Push a value onto a JavaScript array.
    ///
    /// <https://tc39.es/ecma262/#sec-array.prototype.push>
    fn array_push(&mut self, array: &T::JsObject, value: T::JsValue) -> Completion<(), T>;

    // ────────────────────────────────────────────────────────────────────────
    // Object Construction (replaces engine-specific ObjectInitializer)
    // ────────────────────────────────────────────────────────────────────────

    /// Create a plain JavaScript object, optionally inheriting from a prototype.
    fn create_plain_object(&mut self, prototype: Option<&T::JsObject>) -> T::JsObject;

    /// Set a string-keyed property on a JS object.
    /// Convenience wrapping `set` with a `PropertyKey::String`.
    fn object_set_property(
        &mut self,
        object: T::JsObject,
        key: &str,
        value: T::JsValue,
    ) -> Completion<(), T> {
        self.set(object, self.property_key_from_str(key), value, false)
    }

    // ────────────────────────────────────────────────────────────────────────
    // JSON Serialization
    // ────────────────────────────────────────────────────────────────────────

    /// <https://tc39.es/ecma262/#sec-json.stringify>
    /// Serialize a value to a JSON string.
    fn json_stringify(&mut self, value: T::JsValue) -> Completion<String, T>;

    // ────────────────────────────────────────────────────────────────────────
    // BigInt Construction
    // ────────────────────────────────────────────────────────────────────────

    /// Create a `JsValue` from an `i64` BigInt.  Enables exercising `to_bigint`
    /// and `string_to_bigint` without a BigInt constructor on the trait.
    fn value_from_bigint(&mut self, n: i64) -> T::JsValue;
}

// ────────────────────────────────────────────────────────────────────────────
// <https://tc39.es/ecma262/>
//
// Engine factory: creates realms, built-in functions, evaluates scripts.
// Used at initialization time only.
// ────────────────────────────────────────────────────────────────────────────

/// <https://tc39.es/ecma262/>
pub trait JsEngine<T: JsTypes> {
    // ────────────────────────────────────────────────────────────────────────
    // §9.3 Realm — creation
    // ────────────────────────────────────────────────────────────────────────

    /// <https://tc39.es/ecma262/#sec-createrealm>
    fn create_realm(&mut self) -> T::Realm
    where
        T: JsTypesWithRealm;

    /// <https://tc39.es/ecma262/#sec-setrealmglobalobject>
    fn set_realm_global_object(
        &mut self,
        realm: &T::Realm,
        global: T::JsObject,
        this_value: Option<T::JsObject>,
    ) where
        T: JsTypesWithRealm;

    /// <https://tc39.es/ecma262/#sec-setdefaultglobalbindings>
    fn set_default_global_bindings(&mut self, realm: &T::Realm) -> Completion<(), T>
    where
        T: JsTypesWithRealm;

    // ────────────────────────────────────────────────────────────────────────
    // §10.3 Built-in Function Objects
    // ────────────────────────────────────────────────────────────────────────

    /// <https://tc39.es/ecma262/#sec-createbuiltinfunction>
    ///
    /// Creates a JS-callable function object from native behaviour steps,
    /// following the same pattern Web IDL uses for observable arrays,
    /// callback interfaces, and other native-to-JS bridges.
    ///
    /// The behaviour closure receives the JS arguments, the `this` value,
    /// and a `&mut dyn ExecutionContext<T>` for calling any ECMA-262
    /// runtime operation.
    fn create_builtin_function(
        &mut self,
        behaviour: Box<
            dyn Fn(
                &[T::JsValue],
                T::JsValue,
                &mut dyn ExecutionContext<T>,
            ) -> Completion<T::JsValue, T>,
        >,
        length: u32,
        name: T::PropertyKey,
        realm: &T::Realm,
    ) -> T::Function
    where
        T: JsTypesWithRealm;

    // ────────────────────────────────────────────────────────────────────────
    // §16.1 / §16.2 Script and Module evaluation
    // ────────────────────────────────────────────────────────────────────────

    /// <https://tc39.es/ecma262/#sec-runtime-semantics-scriptevaluation>
    fn evaluate_script(&mut self, source: &str, realm: &T::Realm) -> Completion<T::JsValue, T>
    where
        T: JsTypesWithRealm;

    /// <https://tc39.es/ecma262/#sec-evaluatemodule>
    fn evaluate_module(&mut self, source: &str, realm: &T::Realm) -> Completion<T::JsObject, T>
    where
        T: JsTypesWithRealm;

    // ────────────────────────────────────────────────────────────────────────
    // §25.1 ArrayBuffer Abstract Operations — creation
    // ────────────────────────────────────────────────────────────────────────

    /// <https://tc39.es/ecma262/#sec-allocatearraybuffer>
    fn allocate_array_buffer(
        &mut self,
        constructor: T::Constructor,
        byte_length: u64,
        max_byte_length: Option<u64>,
    ) -> Completion<T::ArrayBuffer, T>;

    /// <https://tc39.es/ecma262/#sec-detacharraybuffer>
    fn detach_array_buffer(
        &mut self,
        array_buffer: T::ArrayBuffer,
        key: Option<T::JsValue>,
    ) -> Completion<(), T>;

    /// <https://tc39.es/ecma262/#sec-clonearraybuffer>
    fn clone_array_buffer(
        &mut self,
        src: T::ArrayBuffer,
        src_byte_offset: u64,
        src_length: u64,
        clone_constructor: T::Constructor,
    ) -> Completion<T::ArrayBuffer, T>;

    // ────────────────────────────────────────────────────────────────────────
    // §25.2 SharedArrayBuffer Abstract Operations
    // ────────────────────────────────────────────────────────────────────────

    /// <https://tc39.es/ecma262/#sec-allocatesharedarraybuffer>
    fn allocate_shared_array_buffer(
        &mut self,
        constructor: T::Constructor,
        byte_length: u64,
    ) -> Completion<T::SharedArrayBuffer, T>;

    // ────────────────────────────────────────────────────────────────────────
    // HTML host hooks
    // ────────────────────────────────────────────────────────────────────────

    /// <https://html.spec.whatwg.org/#host-hooks>
    fn set_host_hooks(&mut self, hooks: HostHooks<T>)
    where
        T: JsTypesWithRealm;
}

// ────────────────────────────────────────────────────────────────────────────
// HostHooks
// ────────────────────────────────────────────────────────────────────────────

/// <https://html.spec.whatwg.org/#javascript-specification-host-hooks>
pub struct HostHooks<T: JsTypesWithRealm> {
    /// <https://html.spec.whatwg.org/#hostensurecancompilestrings>
    pub ensure_can_compile_strings: Option<Box<dyn Fn(&T::Realm) -> Completion<(), T>>>,

    /// <https://html.spec.whatwg.org/#hostpromiserejectiontracker>
    pub promise_rejection_tracker: Option<Box<dyn Fn(T::Promise, PromiseRejectionOperation)>>,

    /// <https://html.spec.whatwg.org/#hostenqueuepromisejob>
    pub enqueue_promise_job: Option<Box<dyn Fn(Box<dyn FnOnce()>, Option<T::Realm>)>>,

    /// <https://html.spec.whatwg.org/#hostloadimportedmodule>
    pub load_imported_module: Option<Box<dyn Fn(ModuleRequest<T>, PromiseCapability<T>)>>,
}

impl<T: JsTypesWithRealm> HostHooks<T> {
    pub fn empty() -> Self {
        Self {
            ensure_can_compile_strings: None,
            promise_rejection_tracker: None,
            enqueue_promise_job: None,
            load_imported_module: None,
        }
    }
}

// Needed for HostHooks field type resolution
use crate::records::ModuleRequest;

//! `BoaContext` — the Boa implementation of `ExecutionContext<BoaTypes>`.
//!
//! Wraps `boa_engine::Context` (the stateful JS runtime: realm, heap,
//! global object, job queue).  Also implements `JsEngine<BoaTypes>` for
//! convenience (factory operations like `create_realm`), but the primary
//! role is the execution context — the per-realm runtime state.
//!
//! ## Layout safety
//!
//! `BoaContext` is `#[repr(transparent)]` over `Context`.  This enables the
//! `create_builtin_function` shim to safely cast `&mut Context` →
//! `&mut BoaContext` → `&mut dyn ExecutionContext<BoaTypes>` inside the
//! `NativeFunction` callback, giving the behaviour closure access to all
//! ECMA-262 runtime operations without an external adapter.
//!
//! ## What's not yet implemented
//!
//! - `evaluate_module` — module loader not wired (`todo!()`)
//! - `generator_start` — VM internal (`todo!()`)
//!
//! ### Silent no-ops (return plausible-looking fake data)
//!
//! These methods return default values instead of implementing the full
//! spec algorithm.  Unlike `todo!()` they do not panic, so callers get
//! no error signal:
//!
//! - `get_value_from_buffer` — always returns `undefined`
//! - `set_value_in_buffer` — always `Ok(())`, does nothing
//! - `is_detached_buffer` — always `false` (Boa's `JsArrayBuffer` doesn't
//!   expose `[[IsDetached]]` through its public API)
//! - `is_fixed_length_array_buffer` — always `true` (resizable ArrayBuffers
//!   not supported)
//! - `species_constructor` — always returns `default_constructor`, ignores
//!   `@@species` entirely
//! - `set_host_hooks` — no-op for Boa (host hooks are set during
//!   `ContextBuilder::host_hooks()`, not at runtime)
//!
//! See `js_engine/README.md` for the migration plan and
//! `super::mod.rs` for known Boa-specific quirks.

use boa_engine::{
    Context, JsBigInt, JsError, JsNativeError, JsResult, JsSymbol, JsValue,
    builtins::array_buffer::AlignedVec,
    job::{GenericJob, Job},
    native_function::NativeFunction,
    object::{
        FunctionObjectBuilder, JsObject,
        builtins::{
            JsArrayBuffer, JsDataView, JsFunction, JsGenerator, JsPromise, JsSharedArrayBuffer,
            JsTypedArray,
        },
        builtins::{JsDate, JsMap, JsRegExp, JsSet},
    },
    property::PropertyKey,
    value::PreferredType as BoaPreferredType,
};

use crate::{
    Completion, EcmascriptHost, ExecutionContext, HostHooks, IntegrityLevel, IteratorKind,
    JsEngine, JsTypes, JsTypesWithRealm, Numeric, PreferredType, SharedMemoryOrder,
    TypedArrayElementType,
    records::{
        IteratorRecord, PromiseCapability, PromiseResolvers, PropertyDescriptor, RealmIntrinsics,
    },
};

use super::types::BoaTypes;

/// Zero-sized marker type for "no captures" — implements `Trace` with no fields.
#[derive(Clone, boa_gc::Finalize, boa_gc::Trace, boa_engine::JsData)]
pub(crate) struct NoCaptures;

/// Wrapper for `Box<dyn Fn>` used by the trait method impls.
/// Domain code should use `create_builtin_fn_with_captures` directly.
pub(crate) struct UnsafeFnBox(
    Box<
        dyn Fn(
            &[JsValue],
            JsValue,
            &mut dyn ExecutionContext<BoaTypes>,
        ) -> Completion<JsValue, BoaTypes>,
    >,
);

// SAFETY: No-op trace — UnsafeFnBox is only used by the trait object
// methods.  Domain code uses `create_builtin_fn_with_captures` instead.
unsafe impl boa_gc::Trace for UnsafeFnBox {
    unsafe fn trace(&self, _tracer: &mut boa_gc::Tracer) {}
    unsafe fn trace_non_roots(&self) {}
    fn run_finalizer(&self) {}
}
impl boa_gc::Finalize for UnsafeFnBox {}

/// Safe standalone function: create a built-in function with traceable captures.
///
/// Generic over `T` so the content crate helper can call it from generic
/// Web IDL infrastructure code.  On the Boa backend `T` is always `BoaTypes`;
/// the unsafe cast inside erases the type parameter.
/// `C` must implement `boa_gc::Trace` (auto-derived via `#[gc_struct]`).
/// `behaviour` must be a `fn` pointer (not a closure) so the transmute is valid.
pub fn create_builtin_fn_with_captures<T, C>(
    ec: &mut dyn ExecutionContext<T>,
    captures: C,
    behaviour: fn(
        &[T::JsValue],
        T::JsValue,
        &C,
        &mut dyn ExecutionContext<T>,
    ) -> Completion<T::JsValue, T>,
    length: u32,
    name: T::PropertyKey,
    is_constructor: bool,
) -> T::Function
where
    T: JsTypes,
    C: boa_gc::Trace + 'static,
{
    // SAFETY: On the Boa backend, T is always BoaTypes.
    // &mut dyn ExecutionContext<T> and &mut dyn ExecutionContext<BoaTypes>
    // have identical fat-pointer layout (2 * usize).
    let boa_ec: &mut dyn ExecutionContext<BoaTypes> = unsafe { std::mem::transmute(ec) };
    // SAFETY: fn pointers are all usize-sized regardless of signature.
    let boa_behaviour: fn(
        &[JsValue],
        JsValue,
        &C,
        &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<JsValue, BoaTypes> = unsafe { std::mem::transmute(behaviour) };
    // SAFETY: T::PropertyKey and PropertyKey have same size at runtime.
    let boa_name: boa_engine::property::PropertyKey = unsafe {
        let mut dst = std::mem::MaybeUninit::uninit();
        std::ptr::copy_nonoverlapping(
            &name as *const T::PropertyKey as *const u8,
            dst.as_mut_ptr() as *mut u8,
            std::mem::size_of::<boa_engine::property::PropertyKey>(),
        );
        std::mem::forget(name);
        dst.assume_init()
    };
    let result = create_builtin_fn_with_captures_impl(
        boa_ec,
        captures,
        boa_behaviour,
        length,
        boa_name,
        is_constructor,
    );
    // SAFETY: T::Function and JsFunction have same size at runtime.
    unsafe {
        let mut dst = std::mem::MaybeUninit::uninit();
        std::ptr::copy_nonoverlapping(
            &result as *const JsFunction as *const u8,
            dst.as_mut_ptr() as *mut u8,
            std::mem::size_of::<JsFunction>(),
        );
        std::mem::forget(result);
        dst.assume_init()
    }
}

/// Core implementation — non-generic, operates on `BoaTypes` concretely.
pub fn create_builtin_fn_with_captures_impl<C, F>(
    ec: &mut dyn ExecutionContext<BoaTypes>,
    captures: C,
    behaviour: F,
    length: u32,
    name: PropertyKey,
    is_constructor: bool,
) -> JsFunction
where
    C: boa_gc::Trace + 'static,
    F: Fn(
            &[JsValue],
            JsValue,
            &C,
            &mut dyn ExecutionContext<BoaTypes>,
        ) -> Completion<JsValue, BoaTypes>
        + Copy
        + 'static,
{
    let context = unsafe { ec_to_ctx(ec) };
    let boa = context_as_engine(context);

    let realm = boa.current_realm();
    let name_str = match &name {
        PropertyKey::String(s) => s.clone(),
        PropertyKey::Symbol(_) => boa_engine::js_string!(""),
        _ => boa_engine::js_string!(""),
    };

    // Store captures C directly in the NativeFunction.  Boa's GC
    // automatically traces through all JsObject references inside C.
    let native = NativeFunction::from_copy_closure_with_captures(
        move |this, args, captures, context| {
            let engine: &mut BoaContext =
                unsafe { &mut *(context as *mut Context as *mut BoaContext) };
            behaviour(args, this.clone(), captures, engine).map_err(JsError::from_opaque)
        },
        captures,
    );

    let mut builder = FunctionObjectBuilder::new(&realm, native)
        .name(name_str)
        .length(length as usize);
    if is_constructor {
        builder = builder.constructor(true);
    }
    builder.build()
}

/// Captures for the `resolve` wrapper used when piping a `.then()` result
/// through a capability.
#[derive(boa_gc::Trace, boa_gc::Finalize)]
struct PromiseThenResolve {
    func: JsFunction,
}

/// Captures for the `reject` wrapper used when piping a `.then()` result
/// through a capability.
#[derive(boa_gc::Trace, boa_gc::Finalize)]
struct PromiseThenReject {
    func: JsFunction,
}

/// Boa execution context.  Wraps a `boa_engine::Context` (the stateful JS
/// runtime: realm, heap, global object) and implements
/// `ExecutionContext<BoaTypes>`.  Also carries `JsEngine<BoaTypes>`
/// factory methods as a convenience.
///
/// This is the runtime state that an `EnvironmentSettingsObject` owns —
/// it IS the HTML spec's "realm execution context."
///
/// # Layout
///
/// `#[repr(transparent)]` guarantees the same memory layout as `Context`,
/// enabling safe pointer casts from `&mut Context` to `&mut BoaContext`
/// inside the `create_builtin_function` shim.
#[repr(transparent)]
pub struct BoaContext {
    context: Context,
}

// ── Bridge casts (temporary) ────────────────────────────────────────────────
//
// These convert between `&mut Context` and `&mut dyn ExecutionContext<BoaTypes>`
// relying on `BoaContext` being `#[repr(transparent)]` over `Context`.
// They will be deleted once all Boa-specific APIs are behind trait methods.

/// Cast `&mut Context` to `&mut BoaContext` via repr(transparent) layout.
///
/// SAFETY: `BoaContext` is `#[repr(transparent)]` over `Context`.
pub fn context_as_engine(context: &mut boa_engine::Context) -> &mut BoaContext {
    // SAFETY: BoaContext has the same repr as Context (repr(transparent)),
    // and this function produces a reference with the same lifetime as the input.
    unsafe { &mut *(context as *mut boa_engine::Context as *mut BoaContext) }
}

/// Cast `&mut Context` to `&mut dyn ExecutionContext<BoaTypes>`.
pub fn context_as_ec(context: &mut boa_engine::Context) -> &mut dyn ExecutionContext<BoaTypes> {
    context_as_engine(context)
}

/// Cast `&Context` to `&dyn ExecutionContext<BoaTypes>` (immutable).
pub fn context_as_ec_ref(context: &boa_engine::Context) -> &dyn ExecutionContext<BoaTypes> {
    unsafe { &*(context as *const boa_engine::Context as *const BoaContext) }
}

/// Cast `&mut dyn ExecutionContext<BoaTypes>` back to `&mut Context`.
///
/// SAFETY: The `dyn ExecutionContext<BoaTypes>` must be backed by a `BoaContext`.
pub unsafe fn ec_to_ctx<'a>(
    ec: &'a mut dyn ExecutionContext<BoaTypes>,
) -> &'a mut boa_engine::Context {
    // SAFETY: BoaContext is repr(transparent) over Context.
    unsafe {
        &mut *(ec as *mut dyn ExecutionContext<BoaTypes> as *mut BoaContext
            as *mut boa_engine::Context)
    }
}

impl BoaContext {
    pub fn new() -> Self {
        Self {
            context: Context::default(),
        }
    }

    /// Wrap an existing `Context` into a `BoaContext`.
    ///
    /// Used during migration from direct `Context` ownership to `BoaContext`
    /// in `content/`.  The context is moved into the engine wrapper.
    pub fn from_context(context: Context) -> Self {
        Self { context }
    }

    pub fn context(&mut self) -> &mut Context {
        &mut self.context
    }
    pub fn context_ref(&self) -> &Context {
        &self.context
    }
    pub fn into_context(self) -> Context {
        self.context
    }

    /// Create a platform object that preserves GC tracing through its
    /// type-erased storage.
    ///
    /// Unlike `create_object_with_any`, this method keeps the concrete type
    /// `T` through to `JsObject::from_proto_and_data`, so the Boa GC can
    /// trace through `GcCell<T>` fields inside platform objects.  This is
    /// the correct path for all Web IDL platform objects (those created
    /// via `create_interface_instance`).
    ///
    /// `create_object_with_any` should only be used for objects that don't
    /// need GC tracing (prototypes, namespace objects, etc.).
    pub fn create_platform_object<
        T: boa_gc::Trace + boa_gc::Finalize + boa_engine::JsData + 'static,
    >(
        &mut self,
        prototype: JsObject,
        data: T,
    ) -> JsObject {
        JsObject::from_proto_and_data(Some(prototype), data)
    }
}

impl Default for BoaContext {
    fn default() -> Self {
        Self::new()
    }
}

fn into_completion<T>(result: JsResult<T>, context: &mut Context) -> Completion<T, BoaTypes> {
    result.map_err(|e| e.into_opaque(context).unwrap_or(JsValue::undefined()))
}

fn typed_array_element_size(element_type: TypedArrayElementType) -> u64 {
    match element_type {
        TypedArrayElementType::Int8 => 1,
        TypedArrayElementType::Uint8 => 1,
        TypedArrayElementType::Uint8Clamped => 1,
        TypedArrayElementType::Int16 => 2,
        TypedArrayElementType::Uint16 => 2,
        TypedArrayElementType::Int32 => 4,
        TypedArrayElementType::Uint32 => 4,
        TypedArrayElementType::Float16 => 2,
        TypedArrayElementType::Float32 => 4,
        TypedArrayElementType::Float64 => 8,
        TypedArrayElementType::BigInt64 => 8,
        TypedArrayElementType::BigUint64 => 8,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// JsEngine<BoaTypes> — factory operations (§9.3, §10.3, §16, §25)
// ═══════════════════════════════════════════════════════════════════════════

impl JsEngine<BoaTypes> for BoaContext {
    // ── §9.3 Realm — creation ───────────────────────────────────────────

    fn create_realm(&mut self) -> boa_engine::realm::Realm
    where
        BoaTypes: JsTypesWithRealm,
    {
        self.context.create_realm().expect("create_realm failed")
    }

    fn set_realm_global_object(
        &mut self,
        _realm: &boa_engine::realm::Realm,
        _global: JsObject,
        _this_value: Option<JsObject>,
    ) where
        BoaTypes: JsTypesWithRealm,
    {
        // HARD: Boa's Context doesn't expose set_realm_global_object through its public API.
        // This is typically done at context construction time.
    }

    fn set_default_global_bindings(
        &mut self,
        _realm: &boa_engine::realm::Realm,
    ) -> Completion<(), BoaTypes>
    where
        BoaTypes: JsTypesWithRealm,
    {
        // A fresh context already has default bindings.
        Ok(())
    }

    // ── §16 Script and Module evaluation ──────────────────────────────────

    fn evaluate_script(
        &mut self,
        source: &str,
        _realm: &boa_engine::realm::Realm,
    ) -> Completion<JsValue, BoaTypes>
    where
        BoaTypes: JsTypesWithRealm,
    {
        into_completion(
            self.context.eval(boa_engine::Source::from_bytes(source)),
            &mut self.context,
        )
    }

    fn evaluate_module(
        &mut self,
        _source: &str,
        _realm: &boa_engine::realm::Realm,
    ) -> Completion<JsObject, BoaTypes>
    where
        BoaTypes: JsTypesWithRealm,
    {
        todo!("Boa module evaluation")
    }

    // ── §25 ArrayBuffer — creation ──────────────────────────────────────

    fn allocate_array_buffer(
        &mut self,
        constructor: JsFunction,
        byte_length: u64,
        _max_byte_length: Option<u64>,
    ) -> Completion<JsArrayBuffer, BoaTypes> {
        // <https://tc39.es/ecma262/#sec-allocatearraybuffer>
        // AllocateArrayBuffer via JS constructor.  The constructor
        // internally performs OrdinaryCreateFromConstructor,
        // CreateByteDataBlock, and slot initialization.
        let arg = JsValue::from(byte_length as f64);
        let obj = into_completion(
            constructor.construct(&[arg], Some(&constructor), &mut self.context),
            &mut self.context,
        )?;
        into_completion(JsArrayBuffer::from_object(obj), &mut self.context)
    }

    fn detach_array_buffer(
        &mut self,
        array_buffer: JsArrayBuffer,
        key: Option<JsValue>,
    ) -> Completion<(), BoaTypes> {
        let undefined = JsValue::undefined();
        let detach_key = key.as_ref().unwrap_or(&undefined);
        into_completion(
            array_buffer.detach(detach_key).map(|_| ()),
            &mut self.context,
        )
    }

    fn clone_array_buffer(
        &mut self,
        src: JsArrayBuffer,
        src_byte_offset: u64,
        src_length: u64,
        _clone_constructor: JsFunction,
    ) -> Completion<JsArrayBuffer, BoaTypes> {
        // <https://tc39.es/ecma262/#sec-clonearraybuffer>
        //
        // Step 1: Assert: IsDetachedBuffer(sourceBuffer) is false.
        // Step 2: Let targetBuffer be ? AllocateArrayBuffer(%ArrayBuffer%, sourceLength).
        //
        // Read source data.
        let src_bytes = src.data().ok_or_else(|| {
            JsValue::from(
                JsNativeError::typ()
                    .with_message("Cannot clone a detached ArrayBuffer")
                    .into_opaque(&mut self.context),
            )
        })?;
        // Step 3: Let sourceBlock be sourceBuffer.[[ArrayBufferData]].
        let start = src_byte_offset as usize;
        let end = start + src_length as usize;
        let slice = &src_bytes[start..end];
        // Step 4: Perform CopyDataBlockBytes(targetBlock, 0, sourceBlock, ...).
        // Create an AlignedVec from the source slice and use from_byte_block.
        let aligned = AlignedVec::from_slice(64, slice);
        // Step 5: Return targetBuffer.
        into_completion(
            JsArrayBuffer::from_byte_block(aligned, &mut self.context),
            &mut self.context,
        )
    }

    fn allocate_shared_array_buffer(
        &mut self,
        _constructor: JsFunction,
        byte_length: u64,
    ) -> Completion<JsSharedArrayBuffer, BoaTypes> {
        // <https://tc39.es/ecma262/#sec-allocatesharedarraybuffer>
        // Steps 1-4 are handled by JsSharedArrayBuffer::new internally.
        into_completion(
            JsSharedArrayBuffer::new(byte_length as usize, &mut self.context),
            &mut self.context,
        )
    }

    // ── Host Hooks ────────────────────────────────────────────────────────

    fn set_host_hooks(&mut self, _hooks: HostHooks<BoaTypes>)
    where
        BoaTypes: JsTypesWithRealm,
    {
        // TODO: store and call through hooks internally
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ExecutionContext<BoaTypes> — running execution context (§7, §9.3 runtime,
// §9.6 jobs, §25 queries, §27 promises, value construction)
// ═══════════════════════════════════════════════════════════════════════════

impl ExecutionContext<BoaTypes> for BoaContext {
    fn as_any_mut(&mut self) -> &mut dyn core::any::Any {
        self
    }

    // ── §10.3 Built-in Function Objects ──────────────────────────────────

    fn create_builtin_fn_static(
        &mut self,
        behaviour: fn(
            &[JsValue],
            JsValue,
            &mut dyn ExecutionContext<BoaTypes>,
        ) -> Completion<JsValue, BoaTypes>,
        length: u32,
        name: PropertyKey,
    ) -> JsFunction {
        create_builtin_fn_with_captures_impl(
            self,
            NoCaptures,
            move |args, this, _captures, ec| behaviour(args, this, ec),
            length,
            name,
            false,
        )
    }

    fn create_builtin_fn(
        &mut self,
        behaviour: Box<
            dyn Fn(
                &[JsValue],
                JsValue,
                &mut dyn ExecutionContext<BoaTypes>,
            ) -> Completion<JsValue, BoaTypes>,
        >,
        length: u32,
        name: PropertyKey,
    ) -> JsFunction {
        let wrapped = UnsafeFnBox(behaviour);
        create_builtin_fn_with_captures_impl(
            self,
            wrapped,
            move |args, this, captures, ec| captures.0(args, this, ec),
            length,
            name,
            false,
        )
    }

    fn create_builtin_function(
        &mut self,
        behaviour: Box<
            dyn Fn(
                &[JsValue],
                JsValue,
                &mut dyn ExecutionContext<BoaTypes>,
            ) -> Completion<JsValue, BoaTypes>,
        >,
        length: u32,
        name: PropertyKey,
        is_constructor: bool,
    ) -> JsFunction {
        let wrapped = UnsafeFnBox(behaviour);
        create_builtin_fn_with_captures_impl(
            self,
            wrapped,
            move |args, this, captures, ec| captures.0(args, this, ec),
            length,
            name,
            is_constructor,
        )
    }

    // ── §7.1 Type Conversion ──────────────────────────────────────────────

    fn to_primitive(
        &mut self,
        input: JsValue,
        preferred_type: Option<PreferredType>,
    ) -> Completion<JsValue, BoaTypes> {
        let hint = match preferred_type {
            Some(PreferredType::String) => BoaPreferredType::String,
            Some(PreferredType::Number) => BoaPreferredType::Number,
            None => BoaPreferredType::Default,
        };
        into_completion(
            input.to_primitive(&mut self.context, hint),
            &mut self.context,
        )
    }

    fn to_boolean(&self, value: &JsValue) -> bool {
        value.to_boolean()
    }

    fn to_number(&mut self, value: JsValue) -> Completion<f64, BoaTypes> {
        into_completion(value.to_number(&mut self.context), &mut self.context)
    }

    fn to_numeric(&mut self, value: JsValue) -> Completion<Numeric<BoaTypes>, BoaTypes> {
        if let Some(bigint) = value.as_bigint() {
            return Ok(Numeric::BigInt(bigint.clone()));
        }
        self.to_number(value).map(Numeric::Number)
    }

    fn to_int32(&mut self, value: JsValue) -> Completion<i32, BoaTypes> {
        into_completion(value.to_i32(&mut self.context), &mut self.context)
    }

    fn to_uint32(&mut self, value: JsValue) -> Completion<u32, BoaTypes> {
        into_completion(value.to_u32(&mut self.context), &mut self.context)
    }

    fn to_int16(&mut self, value: JsValue) -> Completion<i16, BoaTypes> {
        into_completion(value.to_int16(&mut self.context), &mut self.context)
    }

    fn to_uint16(&mut self, value: JsValue) -> Completion<u16, BoaTypes> {
        into_completion(value.to_uint16(&mut self.context), &mut self.context)
    }

    fn to_int8(&mut self, value: JsValue) -> Completion<i8, BoaTypes> {
        into_completion(value.to_int8(&mut self.context), &mut self.context)
    }

    fn to_uint8(&mut self, value: JsValue) -> Completion<u8, BoaTypes> {
        into_completion(value.to_uint8(&mut self.context), &mut self.context)
    }

    fn to_uint8_clamp(&mut self, value: JsValue) -> Completion<u8, BoaTypes> {
        into_completion(value.to_uint8_clamp(&mut self.context), &mut self.context)
    }

    fn to_bigint(&mut self, value: JsValue) -> Completion<boa_engine::JsBigInt, BoaTypes> {
        into_completion(value.to_bigint(&mut self.context), &mut self.context)
    }

    fn string_to_bigint(&mut self, string: boa_engine::JsString) -> Option<boa_engine::JsBigInt> {
        boa_engine::JsBigInt::from_string(&string.to_std_string_escaped())
    }

    fn to_js_string(&mut self, value: JsValue) -> Completion<boa_engine::JsString, BoaTypes> {
        into_completion(value.to_string(&mut self.context), &mut self.context)
    }

    fn to_object(&mut self, value: JsValue) -> Completion<JsObject, BoaTypes> {
        into_completion(value.to_object(&mut self.context), &mut self.context)
    }

    fn to_property_key(&mut self, value: JsValue) -> Completion<PropertyKey, BoaTypes> {
        // Spec: ToPropertyKey converts value to primitive with hint String,
        // then if it is Symbol, returns that Symbol, otherwise ToString.
        let primitive = into_completion(
            value.to_primitive(&mut self.context, BoaPreferredType::String),
            &mut self.context,
        )?;
        if let Some(sym) = primitive.as_symbol() {
            return Ok(PropertyKey::from(sym));
        }
        let string = into_completion(primitive.to_string(&mut self.context), &mut self.context)?;
        Ok(PropertyKey::from(string))
    }

    fn to_length(&mut self, value: JsValue) -> Completion<u64, BoaTypes> {
        // <https://tc39.es/ecma262/#sec-tolength>
        //
        // Note: Spec returns a Number (u64 in our trait type).
        let number = into_completion(value.to_number(&mut self.context), &mut self.context)?;
        // Step 1: Let length be ? ToIntegerOrInfinity(arg).
        // Step 2: If length ≤ 0, return +0𝔽.
        if number.is_nan() || number <= 0.0 {
            Ok(0)
        } else {
            // Step 3: Return 𝔽(min(length, 2^53 - 1)).
            Ok((number.min(9007199254740991.0)) as u64)
        }
    }

    fn canonical_numeric_index_string(&self, argument: &boa_engine::JsString) -> Option<f64> {
        let s = argument.to_std_string_escaped();
        if let Ok(n) = s.parse::<f64>() {
            if n.to_string() == s || (n.is_infinite() && (s.starts_with('-') || s.starts_with('+')))
            {
                return Some(n);
            }
        }
        None
    }

    fn to_index(&mut self, value: JsValue) -> Completion<u64, BoaTypes> {
        // <https://tc39.es/ecma262/#sec-toindex>
        //
        // Note: Our trait returns u64.  The spec's output is a mathematical
        // integer, which we represent as u64.
        //
        // Step 1: Let int be ? ToIntegerOrInfinity(arg).
        let number = into_completion(value.to_number(&mut self.context), &mut self.context)?;
        // ToIntegerOrInfinity: NaN, +0, -0 → 0; +∞ → +∞; -∞ → -∞; otherwise truncate.
        let integer = if number.is_nan() || number == 0.0 {
            0.0
        } else if !number.is_finite() {
            number
        } else {
            number.trunc()
        };
        // Step 2: If int is not in the inclusive interval from 0 to 2^53 - 1,
        // throw a RangeError exception.
        if integer < 0.0 || integer > 9007199254740991.0 {
            return Err(JsValue::from(
                JsNativeError::range()
                    .with_message("Invalid index")
                    .into_opaque(&mut self.context),
            ));
        }
        // Step 3: Return int.
        Ok(integer as u64)
    }

    // ── §7.2 Testing and Comparison ───────────────────────────────────────

    fn require_object_coercible(&mut self, value: JsValue) -> Completion<JsValue, BoaTypes> {
        if value.is_undefined() || value.is_null() {
            Err(JsValue::from(
                JsNativeError::typ()
                    .with_message("Cannot convert undefined or null to object")
                    .into_opaque(&mut self.context),
            ))
        } else {
            Ok(value)
        }
    }

    fn is_array(&mut self, value: &JsValue) -> Completion<bool, BoaTypes> {
        Ok(value.as_object().is_some_and(|o| o.is_array()))
    }

    // is_callable is inherited from EcmascriptHost<BoaTypes>

    fn is_constructor(&self, value: &JsValue) -> bool {
        value.as_object().is_some_and(|o| o.is_constructor())
    }

    fn is_extensible(&mut self, object: &JsObject) -> Completion<bool, BoaTypes> {
        into_completion(object.is_extensible(&mut self.context), &mut self.context)
    }

    fn is_integral_number(&self, value: &JsValue) -> bool {
        value
            .as_number()
            .is_some_and(|n| n.is_finite() && n.trunc() == n)
    }

    fn is_property_key(&self, value: &JsValue) -> bool {
        value.is_string() || value.as_symbol().is_some()
    }

    fn same_value(&self, x: &JsValue, y: &JsValue) -> bool {
        JsValue::same_value(x, y)
    }

    fn same_value_zero(&self, x: &JsValue, y: &JsValue) -> bool {
        JsValue::same_value_zero(x, y)
    }

    fn is_loosely_equal(&mut self, x: JsValue, y: JsValue) -> Completion<bool, BoaTypes> {
        into_completion(x.equals(&y, &mut self.context), &mut self.context)
    }

    fn is_strictly_equal(&self, x: &JsValue, y: &JsValue) -> bool {
        x.strict_equals(y)
    }

    // ── §7.3 Operations on Objects ────────────────────────────────────────

    fn get(
        &mut self,
        object: JsObject,
        property_key: PropertyKey,
    ) -> Completion<JsValue, BoaTypes> {
        into_completion(
            object.get(property_key, &mut self.context),
            &mut self.context,
        )
    }

    fn get_v(
        &mut self,
        value: JsValue,
        property_key: PropertyKey,
    ) -> Completion<JsValue, BoaTypes> {
        // GetV: ToObject then Get
        let object = into_completion(value.to_object(&mut self.context), &mut self.context)?;
        into_completion(
            object.get(property_key, &mut self.context),
            &mut self.context,
        )
    }

    fn set(
        &mut self,
        object: JsObject,
        property_key: PropertyKey,
        value: JsValue,
        _throw: bool,
    ) -> Completion<(), BoaTypes> {
        into_completion(
            object
                .set(property_key, value, false, &mut self.context)
                .map(|_| ()),
            &mut self.context,
        )
    }

    fn create_data_property(
        &mut self,
        object: JsObject,
        property_key: PropertyKey,
        value: JsValue,
    ) -> Completion<bool, BoaTypes> {
        into_completion(
            object.create_data_property(property_key, value, &mut self.context),
            &mut self.context,
        )
    }

    fn to_property_descriptor(
        &mut self,
        desc_obj: JsObject,
    ) -> Completion<PropertyDescriptor<BoaTypes>, BoaTypes> {
        // <https://tc39.es/ecma262/#sec-topropertydescriptor>
        //
        // Step 1: If obj is not an Object, throw a TypeError exception.
        // (guaranteed by the JsObject parameter type)

        // Step 2: Let propertyDesc be a new Property Descriptor that initially has no fields.
        let mut desc = PropertyDescriptor {
            value: None,
            writable: None,
            get: None,
            set: None,
            enumerable: None,
            configurable: None,
        };

        // Step 3: Let hasEnumerable be ? HasProperty(obj, "enumerable").
        // Step 4: If hasEnumerable is true, then ...
        if let Some(enumerable) = has_property_then_get_boolean(self, &desc_obj, "enumerable")? {
            desc.enumerable = Some(enumerable);
        }

        // Step 5: Let hasConfigurable be ? HasProperty(obj, "configurable").
        // Step 6: If hasConfigurable is true, then ...
        if let Some(configurable) = has_property_then_get_boolean(self, &desc_obj, "configurable")?
        {
            desc.configurable = Some(configurable);
        }

        // Step 7-8: Let hasValue be ? HasProperty(obj, "value"). If hasValue is true, then ...
        if has_property(self, &desc_obj, "value")? {
            let value = crate::EcmascriptHost::get(self, &desc_obj, "value")?;
            desc.value = Some(value);
        }

        // Step 9-10: Let hasWritable be ? HasProperty(obj, "writable"). If hasWritable is true, then ...
        if let Some(writable) = has_property_then_get_boolean(self, &desc_obj, "writable")? {
            desc.writable = Some(writable);
        }

        // Step 11-13: Let hasGet be ? HasProperty(obj, "get"). If hasGet is true, then ...
        if has_property(self, &desc_obj, "get")? {
            let getter = crate::EcmascriptHost::get(self, &desc_obj, "get")?;
            if getter.is_object() && !self.is_callable(&getter) {
                // Step 13: If IsCallable(getter) is false and getter is not undefined, throw a TypeError.
                return Err(self.new_type_error("getter must be callable or undefined"));
            }
            if getter.is_object() {
                let obj = getter.as_object().unwrap().clone();
                desc.get = JsFunction::from_object(obj);
            }
        }

        // Step 14-16: Let hasSet be ? HasProperty(obj, "set"). If hasSet is true, then ...
        if has_property(self, &desc_obj, "set")? {
            let setter = crate::EcmascriptHost::get(self, &desc_obj, "set")?;
            if setter.is_object() && !self.is_callable(&setter) {
                return Err(self.new_type_error("setter must be callable or undefined"));
            }
            if setter.is_object() {
                let obj = setter.as_object().unwrap().clone();
                desc.set = JsFunction::from_object(obj);
            }
        }

        // Step 17-18: If propertyDesc has a [[Getter]] or [[Setter]], check no [[Value]] or [[Writable]].
        if (desc.get.is_some() || desc.set.is_some())
            && (desc.value.is_some() || desc.writable.is_some())
        {
            return Err(self.new_type_error(
                "Invalid property descriptor: cannot have both accessor and data fields",
            ));
        }

        // Step 19: Return propertyDesc.
        Ok(desc)
    }

    fn define_property_or_throw(
        &mut self,
        object: JsObject,
        property_key: PropertyKey,
        descriptor: PropertyDescriptor<BoaTypes>,
    ) -> Completion<(), BoaTypes> {
        let boa_desc = boa_engine::property::PropertyDescriptor::builder()
            .maybe_value(descriptor.value)
            .maybe_writable(descriptor.writable)
            .maybe_get(descriptor.get)
            .maybe_set(descriptor.set)
            .maybe_enumerable(descriptor.enumerable)
            .maybe_configurable(descriptor.configurable)
            .build();
        into_completion(
            object
                .define_property_or_throw(property_key, boa_desc, &mut self.context)
                .map(|_| ()),
            &mut self.context,
        )
    }

    fn delete_property_or_throw(
        &mut self,
        object: JsObject,
        property_key: PropertyKey,
    ) -> Completion<(), BoaTypes> {
        into_completion(
            object
                .delete_property_or_throw(property_key, &mut self.context)
                .map(|_| ()),
            &mut self.context,
        )
    }

    fn get_prototype_of(&mut self, object: JsObject) -> Completion<Option<JsObject>, BoaTypes> {
        Ok(object.prototype())
    }

    fn set_prototype(
        &mut self,
        object: JsObject,
        prototype: Option<JsObject>,
    ) -> Completion<bool, BoaTypes> {
        Ok(object.set_prototype(prototype))
    }

    fn get_method(
        &mut self,
        value: JsValue,
        property_key: PropertyKey,
    ) -> Completion<Option<JsFunction>, BoaTypes> {
        let prop = into_completion(
            {
                let object =
                    into_completion(value.to_object(&mut self.context), &mut self.context)?;
                object.get(property_key, &mut self.context)
            },
            &mut self.context,
        )?;
        if let Some(object) = prop.as_object() {
            if object.is_callable() {
                return Ok(JsFunction::from_object(object.clone()));
            }
        }
        Ok(None)
    }

    fn has_property(
        &mut self,
        object: JsObject,
        property_key: PropertyKey,
    ) -> Completion<bool, BoaTypes> {
        into_completion(
            object.has_property(property_key, &mut self.context),
            &mut self.context,
        )
    }

    fn has_own_property(
        &mut self,
        object: JsObject,
        property_key: PropertyKey,
    ) -> Completion<bool, BoaTypes> {
        into_completion(
            object.has_own_property(property_key, &mut self.context),
            &mut self.context,
        )
    }

    fn own_property_keys(&mut self, object: JsObject) -> Completion<Vec<PropertyKey>, BoaTypes> {
        into_completion(
            object.own_property_keys(&mut self.context),
            &mut self.context,
        )
    }

    fn get_own_property(
        &mut self,
        object: JsObject,
        property_key: PropertyKey,
    ) -> Completion<Option<PropertyDescriptor<BoaTypes>>, BoaTypes> {
        // <https://tc39.es/ecma262/#sec-ordinarygetownproperty>
        //
        // Use Boa's internal `__get_own_property__` via the built-in
        // Object.getOwnPropertyDescriptor function, accessed through
        // the intrinsics (not the global) to avoid user-space hijacking.
        //
        // Note: we call the Rust-level `OrdinaryObject::get_own_property_descriptor`
        // directly rather than going through the global binding.  This avoids
        // user code reassigning Object or patching getOwnPropertyDescriptor.
        // The per-element call still uses ToObject on the argument, but since
        // our `object` parameter is already a JsObject this is a no-op.
        let descriptor_val = into_completion(
            boa_engine::builtins::object::OrdinaryObject::get_own_property_descriptor(
                &JsValue::undefined(),
                &[
                    JsValue::from(object),
                    boa_property_key_to_value(&property_key),
                ],
                &mut self.context,
            ),
            &mut self.context,
        )?;

        if descriptor_val.is_undefined() {
            return Ok(None);
        }

        let descriptor_obj = descriptor_val.as_object().ok_or_else(|| {
            JsValue::from(
                JsNativeError::typ()
                    .with_message("Object.getOwnPropertyDescriptor returned a non-object")
                    .into_opaque(&mut self.context),
            )
        })?;

        let value = descriptor_field_value(&descriptor_obj, "value", &mut self.context)?;
        let writable = descriptor_field_value(&descriptor_obj, "writable", &mut self.context)?
            .map(|field| field.to_boolean());
        let get = descriptor_field_value(&descriptor_obj, "get", &mut self.context)?
            .and_then(|field| field.as_object())
            .and_then(JsFunction::from_object);
        let set = descriptor_field_value(&descriptor_obj, "set", &mut self.context)?
            .and_then(|field| field.as_object())
            .and_then(JsFunction::from_object);
        let enumerable = descriptor_field_value(&descriptor_obj, "enumerable", &mut self.context)?
            .map(|field| field.to_boolean());
        let configurable =
            descriptor_field_value(&descriptor_obj, "configurable", &mut self.context)?
                .map(|field| field.to_boolean());

        Ok(Some(PropertyDescriptor {
            value,
            writable,
            get,
            set,
            enumerable,
            configurable,
        }))
    }

    fn construct(
        &mut self,
        function: JsFunction,
        args: &[JsValue],
        new_target: Option<JsFunction>,
    ) -> Completion<JsObject, BoaTypes> {
        into_completion(
            function.construct(args, new_target.as_ref().map(|f| &**f), &mut self.context),
            &mut self.context,
        )
    }

    fn set_integrity_level(
        &mut self,
        object: JsObject,
        level: IntegrityLevel,
    ) -> Completion<bool, BoaTypes> {
        let boa_level = match level {
            IntegrityLevel::Sealed => boa_engine::object::IntegrityLevel::Sealed,
            IntegrityLevel::Frozen => boa_engine::object::IntegrityLevel::Frozen,
        };
        into_completion(
            object.set_integrity_level(boa_level, &mut self.context),
            &mut self.context,
        )
    }

    fn test_integrity_level(
        &mut self,
        object: JsObject,
        level: IntegrityLevel,
    ) -> Completion<bool, BoaTypes> {
        let boa_level = match level {
            IntegrityLevel::Sealed => boa_engine::object::IntegrityLevel::Sealed,
            IntegrityLevel::Frozen => boa_engine::object::IntegrityLevel::Frozen,
        };
        into_completion(
            object.test_integrity_level(boa_level, &mut self.context),
            &mut self.context,
        )
    }

    fn species_constructor(
        &mut self,
        _object: JsObject,
        default_constructor: JsFunction,
    ) -> Completion<JsFunction, BoaTypes> {
        Ok(default_constructor)
    }

    // ── §7.4 Iteration ───────────────────────────────────────────────────

    fn get_iterator(
        &mut self,
        object: JsValue,
        kind: IteratorKind,
        method: Option<JsFunction>,
    ) -> Completion<IteratorRecord<BoaTypes>, BoaTypes> {
        match kind {
            IteratorKind::Async => {
                let method = match method {
                    Some(m) => Some(m),
                    None => self.get_method(
                        object.clone(),
                        PropertyKey::from(JsSymbol::async_iterator()),
                    )?,
                };
                match method {
                    Some(m) => get_iterator_from_method(self, object, m),
                    None => {
                        let sync_method = self
                            .get_method(object.clone(), PropertyKey::from(JsSymbol::iterator()))?;
                        let sync_method = sync_method.ok_or_else(|| {
                            JsValue::from(
                                JsNativeError::typ()
                                    .with_message("object is not iterable")
                                    .into_opaque(&mut self.context),
                            )
                        })?;
                        let sync_record = get_iterator_from_method(self, object, sync_method)?;
                        Ok(sync_record)
                    }
                }
            }
            IteratorKind::Sync => {
                let method = match method {
                    Some(m) => Some(m),
                    None => {
                        self.get_method(object.clone(), PropertyKey::from(JsSymbol::iterator()))?
                    }
                };
                let method = method.ok_or_else(|| {
                    JsValue::from(
                        JsNativeError::typ()
                            .with_message("object is not iterable")
                            .into_opaque(&mut self.context),
                    )
                })?;
                get_iterator_from_method(self, object, method)
            }
        }
    }

    fn iterator_step_value(
        &mut self,
        iterator: &mut IteratorRecord<BoaTypes>,
    ) -> Completion<Option<JsValue>, BoaTypes> {
        let result = into_completion(
            iterator
                .next_method
                .call(&iterator.iterator.clone().into(), &[], &mut self.context),
            &mut self.context,
        )?;
        let result_obj = result.as_object().ok_or_else(|| {
            JsValue::from(
                JsNativeError::typ()
                    .with_message("Iterator result is not an object")
                    .into_opaque(&mut self.context),
            )
        })?;
        let done_val = into_completion(
            result_obj.get(
                PropertyKey::from(boa_engine::js_string!("done")),
                &mut self.context,
            ),
            &mut self.context,
        )?;
        let done = done_val.to_boolean();
        if done {
            iterator.done = true;
            return Ok(None);
        }
        let value = into_completion(
            result_obj.get(
                PropertyKey::from(boa_engine::js_string!("value")),
                &mut self.context,
            ),
            &mut self.context,
        )
        .map_err(|e| {
            iterator.done = true;
            e
        })?;
        Ok(Some(value))
    }

    fn iterator_close(
        &mut self,
        iterator: IteratorRecord<BoaTypes>,
        completion: Completion<JsValue, BoaTypes>,
    ) -> Completion<JsValue, BoaTypes> {
        let iter_value = JsValue::from(iterator.iterator);
        let return_key: PropertyKey = boa_engine::js_string!("return").into();
        let inner_result = self.get_method(iter_value.clone(), return_key);
        let inner_result = match inner_result {
            Ok(Some(return_fn)) => {
                EcmascriptHost::call(self, &JsObject::from(return_fn), &iter_value, &[])
            }
            Ok(None) => {
                return completion;
            }
            Err(e) => {
                if completion.is_err() {
                    return completion;
                }
                return Err(e);
            }
        };
        let completion_value = completion?;
        let inner_value = inner_result?;
        if !inner_value.is_object() {
            return Err(JsValue::from(
                JsNativeError::typ()
                    .with_message("Iterator return result is not an object")
                    .into_opaque(&mut self.context),
            ));
        }
        Ok(completion_value)
    }

    fn async_iterator_close(
        &mut self,
        iterator: IteratorRecord<BoaTypes>,
        completion: Completion<JsValue, BoaTypes>,
    ) -> Completion<JsValue, BoaTypes> {
        let iter_value = JsValue::from(iterator.iterator);
        let return_key: PropertyKey = boa_engine::js_string!("return").into();
        let inner_result = self.get_method(iter_value.clone(), return_key);
        match inner_result {
            Ok(Some(return_fn)) => {
                let callable = JsObject::from(return_fn);
                match EcmascriptHost::call(self, &callable, &iter_value, &[]) {
                    Ok(val) => {
                        if !val.is_object() {
                            return Err(JsValue::from(
                                JsNativeError::typ()
                                    .with_message("Async iterator return result is not an object")
                                    .into_opaque(&mut self.context),
                            ));
                        }
                        completion
                    }
                    Err(e) => {
                        if completion.is_err() {
                            return completion;
                        }
                        Err(e)
                    }
                }
            }
            Ok(None) => completion,
            Err(e) => {
                if completion.is_err() {
                    return completion;
                }
                Err(e)
            }
        }
    }

    // ── §9.3 Realm — runtime access ──────────────────────────────────────

    fn current_realm(&self) -> boa_engine::realm::Realm
    where
        BoaTypes: JsTypesWithRealm,
    {
        self.context.realm().clone()
    }

    fn realm_intrinsics(&self, _realm: &boa_engine::realm::Realm) -> RealmIntrinsics<BoaTypes>
    where
        BoaTypes: JsTypesWithRealm,
    {
        let intrinsics = self.context.intrinsics();
        let constructors = intrinsics.constructors();
        RealmIntrinsics {
            array_buffer: JsFunction::from_object(constructors.array_buffer().constructor())
                .expect("ArrayBuffer constructor"),
            shared_array_buffer: JsFunction::from_object(
                constructors.shared_array_buffer().constructor(),
            )
            .expect("SharedArrayBuffer constructor"),
            promise: JsFunction::from_object(constructors.promise().constructor())
                .expect("Promise constructor"),
            object: JsFunction::from_object(constructors.object().constructor())
                .expect("Object constructor"),
            function: JsFunction::from_object(constructors.function().constructor())
                .expect("Function constructor"),
            error: JsFunction::from_object(constructors.error().constructor())
                .expect("Error constructor"),
            type_error: JsFunction::from_object(constructors.type_error().constructor())
                .expect("TypeError constructor"),
            range_error: JsFunction::from_object(constructors.range_error().constructor())
                .expect("RangeError constructor"),
            syntax_error: JsFunction::from_object(constructors.syntax_error().constructor())
                .expect("SyntaxError constructor"),
            reference_error: JsFunction::from_object(constructors.reference_error().constructor())
                .expect("ReferenceError constructor"),
            uri_error: JsFunction::from_object(constructors.uri_error().constructor())
                .expect("URIError constructor"),
            eval_error: JsFunction::from_object(constructors.eval_error().constructor())
                .expect("EvalError constructor"),
            array: JsFunction::from_object(constructors.array().constructor())
                .expect("Array constructor"),
            uint8_array: JsFunction::from_object(constructors.typed_uint8_array().constructor())
                .expect("Uint8Array constructor"),
            boolean: JsFunction::from_object(constructors.boolean().constructor())
                .expect("Boolean constructor"),
            number: JsFunction::from_object(constructors.number().constructor())
                .expect("Number constructor"),
            string: JsFunction::from_object(constructors.string().constructor())
                .expect("String constructor"),
            bigint: JsFunction::from_object(constructors.bigint().constructor())
                .expect("BigInt constructor"),
            date: JsFunction::from_object(constructors.date().constructor())
                .expect("Date constructor"),
            regexp: JsFunction::from_object(constructors.regexp().constructor())
                .expect("RegExp constructor"),
            map: JsFunction::from_object(constructors.map().constructor())
                .expect("Map constructor"),
            set: JsFunction::from_object(constructors.set().constructor())
                .expect("Set constructor"),
            boolean_prototype: constructors.boolean().prototype(),
            number_prototype: constructors.number().prototype(),
            string_prototype: constructors.string().prototype(),
            bigint_prototype: constructors.bigint().prototype(),
            date_prototype: constructors.date().prototype(),
            regexp_prototype: constructors.regexp().prototype(),
            map_prototype: constructors.map().prototype(),
            set_prototype: constructors.set().prototype(),
            error_prototype: constructors.error().prototype(),
            type_error_prototype: constructors.type_error().prototype(),
            range_error_prototype: constructors.range_error().prototype(),
            syntax_error_prototype: constructors.syntax_error().prototype(),
            reference_error_prototype: constructors.reference_error().prototype(),
            uri_error_prototype: constructors.uri_error().prototype(),
            eval_error_prototype: constructors.eval_error().prototype(),
            object_prototype: constructors.object().prototype(),
            function_prototype: constructors.function().prototype(),
            async_iterator_prototype: intrinsics.objects().iterator_prototypes().async_iterator(),
        }
    }

    fn realm_global_object(&self) -> JsObject
    where
        BoaTypes: JsTypesWithRealm,
    {
        self.context.global_object()
    }

    // ── §7.3 Functions ────────────────────────────────────────────────────

    fn get_function_realm(
        &mut self,
        _function: &JsObject,
    ) -> Completion<boa_engine::realm::Realm, BoaTypes>
    where
        BoaTypes: JsTypesWithRealm,
    {
        // <https://tc39.es/ecma262/#sec-getfunctionrealm>
        //
        // Steps 1-3 require accessing the function's [[Realm]] internal slot,
        // which is stored as `pub(crate)` on Boa's NativeFunction and is not
        // accessible from outside the `boa_engine` crate.  Bound function and
        // Proxy exotic object checks are also not possible through the public
        // API.  In practice, for the Web IDL `internally-create-a-new-object-
        // implementing-the-interface` algorithm, `newTarget` is always created
        // in the current realm, so returning the current realm (step 4) is
        // correct for all current uses.
        //
        // Note: If cross-realm subclassing is needed, this must be updated
        // to extract the function's realm through Boa's internal API.
        //
        // Step 4: Return the current Realm Record.
        Ok(self.context.realm().clone())
    }

    // ── §9.6 Jobs ─────────────────────────────────────────────────────────

    fn enqueue_job(&mut self, _job: Box<dyn FnOnce()>) {
        let realm = self.context.realm().clone();
        let mut deferred_job = Some(_job);
        let job = GenericJob::new(
            move |_context| {
                if let Some(job) = deferred_job.take() {
                    job();
                }
                Ok(JsValue::undefined())
            },
            realm,
        );
        self.context.enqueue_job(Job::from(job));
    }

    fn enqueue_job_with_realm(
        &mut self,
        realm: boa_engine::realm::Realm,
        job: Box<dyn FnOnce(&mut dyn ExecutionContext<BoaTypes>)>,
    ) {
        let mut deferred_job = Some(job);
        let generic_job = GenericJob::new(
            move |context| {
                if let Some(job) = deferred_job.take() {
                    job(context_as_ec(context));
                }
                Ok(JsValue::undefined())
            },
            realm,
        );
        self.context.enqueue_job(Job::from(generic_job));
    }

    fn run_jobs(&mut self) {
        let _ = self.context.run_jobs();
    }

    // ── §25 ArrayBuffer — runtime queries ─────────────────────────────────

    fn allocate_array_buffer(
        &mut self,
        constructor: JsFunction,
        byte_length: u64,
        max_byte_length: Option<u64>,
    ) -> Completion<JsArrayBuffer, BoaTypes> {
        JsEngine::allocate_array_buffer(self, constructor, byte_length, max_byte_length)
    }

    fn clone_array_buffer(
        &mut self,
        src: JsArrayBuffer,
        src_byte_offset: u64,
        src_length: u64,
        clone_constructor: JsFunction,
    ) -> Completion<JsArrayBuffer, BoaTypes> {
        JsEngine::clone_array_buffer(self, src, src_byte_offset, src_length, clone_constructor)
    }

    fn detach_array_buffer(
        &mut self,
        array_buffer: JsArrayBuffer,
        key: Option<JsValue>,
    ) -> Completion<(), BoaTypes> {
        JsEngine::detach_array_buffer(self, array_buffer, key)
    }

    fn is_detached_buffer(&self, _array_buffer: &JsArrayBuffer) -> bool {
        false // HARD: Boa's JsArrayBuffer doesn't expose is_detached publicly
    }

    fn is_fixed_length_array_buffer(&self, _array_buffer: &JsArrayBuffer) -> bool {
        true
    }

    fn get_value_from_buffer(
        &mut self,
        _array_buffer: &JsArrayBuffer,
        _byte_index: u64,
        _element_type: TypedArrayElementType,
        _is_typed_array: bool,
        _order: SharedMemoryOrder,
    ) -> JsValue {
        JsValue::undefined()
    }

    fn set_value_in_buffer(
        &mut self,
        _array_buffer: &JsArrayBuffer,
        _byte_index: u64,
        _element_type: TypedArrayElementType,
        _value: JsValue,
        _is_typed_array: bool,
        _order: SharedMemoryOrder,
    ) -> Completion<(), BoaTypes> {
        Ok(())
    }

    // ── §23.2 TypedArray Objects ──────────────────────────────────────────

    fn typed_array_buffer(
        &mut self,
        typed_array: &JsTypedArray,
    ) -> Completion<JsArrayBuffer, BoaTypes> {
        let buffer_val = into_completion(typed_array.buffer(&mut self.context), &mut self.context)?;
        let buffer_obj = buffer_val
            .as_object()
            .ok_or_else(|| self.new_type_error("TypedArray buffer is not an object"))?;
        JsArrayBuffer::from_object(buffer_obj.clone())
            .map_err(|_| self.new_type_error("TypedArray buffer is not an ArrayBuffer"))
    }

    fn typed_array_byte_offset(&mut self, typed_array: &JsTypedArray) -> Completion<u64, BoaTypes> {
        let offset = into_completion(
            typed_array.byte_offset(&mut self.context),
            &mut self.context,
        )?;
        Ok(offset as u64)
    }

    fn typed_array_byte_length(&mut self, typed_array: &JsTypedArray) -> Completion<u64, BoaTypes> {
        let length = into_completion(
            typed_array.byte_length(&mut self.context),
            &mut self.context,
        )?;
        Ok(length as u64)
    }

    fn typed_array_element_type(
        &self,
        typed_array: &JsTypedArray,
    ) -> Option<TypedArrayElementType> {
        let kind = typed_array.kind()?;
        Some(match kind {
            boa_engine::builtins::typed_array::TypedArrayKind::Int8 => TypedArrayElementType::Int8,
            boa_engine::builtins::typed_array::TypedArrayKind::Uint8 => {
                TypedArrayElementType::Uint8
            }
            boa_engine::builtins::typed_array::TypedArrayKind::Uint8Clamped => {
                TypedArrayElementType::Uint8Clamped
            }
            boa_engine::builtins::typed_array::TypedArrayKind::Int16 => {
                TypedArrayElementType::Int16
            }
            boa_engine::builtins::typed_array::TypedArrayKind::Uint16 => {
                TypedArrayElementType::Uint16
            }
            boa_engine::builtins::typed_array::TypedArrayKind::Int32 => {
                TypedArrayElementType::Int32
            }
            boa_engine::builtins::typed_array::TypedArrayKind::Uint32 => {
                TypedArrayElementType::Uint32
            }
            // Float16 type: behind the `float16` feature (disabled by default since
            // we use boa_engine with default-features=false).
            #[cfg(feature = "float16")]
            boa_engine::builtins::typed_array::TypedArrayKind::Float16 => {
                TypedArrayElementType::Float16
            }
            boa_engine::builtins::typed_array::TypedArrayKind::Float32 => {
                TypedArrayElementType::Float32
            }
            boa_engine::builtins::typed_array::TypedArrayKind::Float64 => {
                TypedArrayElementType::Float64
            }
            boa_engine::builtins::typed_array::TypedArrayKind::BigInt64 => {
                TypedArrayElementType::BigInt64
            }
            boa_engine::builtins::typed_array::TypedArrayKind::BigUint64 => {
                TypedArrayElementType::BigUint64
            }
        })
    }

    fn construct_typed_array_view(
        &mut self,
        element_type: TypedArrayElementType,
        buffer: JsArrayBuffer,
        byte_offset: u64,
        byte_length: u64,
    ) -> Completion<JsTypedArray, BoaTypes> {
        let constructor_name = match element_type {
            TypedArrayElementType::Int8 => "Int8Array",
            TypedArrayElementType::Uint8 => "Uint8Array",
            TypedArrayElementType::Uint8Clamped => "Uint8ClampedArray",
            TypedArrayElementType::Int16 => "Int16Array",
            TypedArrayElementType::Uint16 => "Uint16Array",
            TypedArrayElementType::Int32 => "Int32Array",
            TypedArrayElementType::Uint32 => "Uint32Array",
            TypedArrayElementType::Float16 => "Float16Array",
            TypedArrayElementType::Float32 => "Float32Array",
            TypedArrayElementType::Float64 => "Float64Array",
            TypedArrayElementType::BigInt64 => "BigInt64Array",
            TypedArrayElementType::BigUint64 => "BigUint64Array",
        };
        let global = self.context.global_object();
        let constructor_val = into_completion(
            global.get(
                boa_engine::JsString::from(constructor_name),
                &mut self.context,
            ),
            &mut self.context,
        )?;
        let element_size = typed_array_element_size(element_type);
        let length = byte_length / element_size;
        let buffer_val: JsValue = JsValue::from(buffer);
        let offset_val = JsValue::new(byte_offset as f64);
        let length_val = JsValue::new(length as f64);
        let constructor_obj = constructor_val
            .as_object()
            .ok_or_else(|| self.new_type_error("typed array constructor is not an object"))?;
        let result = into_completion(
            constructor_obj.construct(
                &[buffer_val, offset_val, length_val],
                None,
                &mut self.context,
            ),
            &mut self.context,
        )?;
        JsTypedArray::from_object(result)
            .map_err(|_| self.new_type_error("Failed to create TypedArray view"))
    }

    // ── §25.3 DataView Objects ────────────────────────────────────────────

    fn data_view_buffer(&mut self, data_view: &JsDataView) -> Completion<JsArrayBuffer, BoaTypes> {
        let buffer_val = into_completion(data_view.buffer(&mut self.context), &mut self.context)?;
        let buffer_obj = buffer_val
            .as_object()
            .ok_or_else(|| self.new_type_error("DataView buffer is not an object"))?;
        JsArrayBuffer::from_object(buffer_obj.clone())
            .map_err(|_| self.new_type_error("DataView buffer is not an ArrayBuffer"))
    }

    fn data_view_byte_offset(&mut self, data_view: &JsDataView) -> Completion<u64, BoaTypes> {
        into_completion(data_view.byte_offset(&mut self.context), &mut self.context)
    }

    fn data_view_byte_length(&mut self, data_view: &JsDataView) -> Completion<u64, BoaTypes> {
        into_completion(data_view.byte_length(&mut self.context), &mut self.context)
    }

    fn construct_data_view_from_buffer(
        &mut self,
        buffer: JsArrayBuffer,
        byte_offset: u64,
        byte_length: u64,
    ) -> Completion<JsDataView, BoaTypes> {
        into_completion(
            JsDataView::from_js_array_buffer(
                buffer,
                Some(byte_offset),
                Some(byte_length),
                &mut self.context,
            ),
            &mut self.context,
        )
    }

    // ── §25.1 ArrayBuffer — data access ───────────────────────────────────

    fn array_buffer_data(&self, array_buffer: &JsArrayBuffer) -> Option<Vec<u8>> {
        array_buffer.data().map(|aligned| aligned.to_vec())
    }

    // ── §22.2 Date ────────────────────────────────────────────────────────

    fn get_date_value(&mut self, date: &JsObject) -> Completion<f64, BoaTypes> {
        let js_date = JsDate::from_object(date.clone())
            .map_err(|_| self.new_type_error("object is not a Date"))?;
        let time = js_date.get_time(&mut self.context).map_err(|e| {
            e.into_opaque(&mut self.context)
                .unwrap_or_else(|_| JsValue::undefined())
        })?;
        time.as_number()
            .ok_or_else(|| self.new_type_error("Date.getTime did not return a number"))
    }

    // ── §22.3 RegExp ─────────────────────────────────────────────────────

    fn get_regexp_source(&mut self, regexp: &JsObject) -> Completion<String, BoaTypes> {
        let js_regexp = JsRegExp::from_object(regexp.clone())
            .map_err(|_| self.new_type_error("object is not a RegExp"))?;
        let source: String = js_regexp.source(&mut self.context).map_err(|e| {
            e.into_opaque(&mut self.context)
                .unwrap_or_else(|_| JsValue::undefined())
        })?;
        Ok(source)
    }

    fn get_regexp_flags(&mut self, regexp: &JsObject) -> Completion<String, BoaTypes> {
        let js_regexp = JsRegExp::from_object(regexp.clone())
            .map_err(|_| self.new_type_error("object is not a RegExp"))?;
        let flags: String = js_regexp.flags(&mut self.context).map_err(|e| {
            e.into_opaque(&mut self.context)
                .unwrap_or_else(|_| JsValue::undefined())
        })?;
        Ok(flags)
    }

    // ── §24.1 Map ────────────────────────────────────────────────────────

    fn map_get_entries(&mut self, map: &JsMap) -> Completion<Vec<(JsValue, JsValue)>, BoaTypes> {
        let mut entries = Vec::new();
        map.for_each_native(|key, val| {
            entries.push((key, val));
            Ok(())
        })
        .map_err(|_| self.new_type_error("failed to iterate Map entries"))?;
        Ok(entries)
    }

    fn map_set_entry(
        &mut self,
        map: &JsMap,
        key: JsValue,
        value: JsValue,
    ) -> Completion<(), BoaTypes> {
        map.set(key, value, &mut self.context)
            .map(|_| ())
            .map_err(|e| {
                e.into_opaque(&mut self.context)
                    .unwrap_or_else(|_| JsValue::undefined())
            })
    }

    // ── §24.2 Set ────────────────────────────────────────────────────────

    fn set_get_values(&mut self, set: &JsSet) -> Completion<Vec<JsValue>, BoaTypes> {
        let mut values = Vec::new();
        set.for_each_native(|val| {
            values.push(val);
            Ok(())
        })
        .map_err(|_| self.new_type_error("failed to iterate Set values"))?;
        Ok(values)
    }

    fn set_add_entry(&mut self, set: &JsSet, value: JsValue) -> Completion<(), BoaTypes> {
        set.add(value, &mut self.context).map(|_| ()).map_err(|e| {
            e.into_opaque(&mut self.context)
                .unwrap_or_else(|_| JsValue::undefined())
        })
    }

    // ── §27 Promise ───────────────────────────────────────────────────────

    fn promise_resolve(
        &mut self,
        _constructor: JsFunction,
        x: JsValue,
    ) -> Completion<JsPromise, BoaTypes> {
        into_completion(JsPromise::resolve(x, &mut self.context), &mut self.context)
    }

    fn new_promise_capability(
        &mut self,
        _constructor: JsFunction,
    ) -> Completion<PromiseCapability<BoaTypes>, BoaTypes> {
        let (promise, resolvers) = JsPromise::new_pending(&mut self.context);
        Ok(PromiseCapability {
            promise: JsValue::from(promise),
            resolve: resolvers.resolve,
            reject: resolvers.reject,
        })
    }

    fn new_promise_pending(
        &mut self,
    ) -> Completion<(JsValue, PromiseResolvers<BoaTypes>), BoaTypes> {
        let (promise, resolvers) = JsPromise::new_pending(&mut self.context);
        let ec: &mut dyn ExecutionContext<BoaTypes> = self;
        Ok((
            JsValue::from(promise),
            PromiseResolvers::new(resolvers.resolve.into(), resolvers.reject.into(), ec),
        ))
    }

    fn perform_promise_then(
        &mut self,
        promise: JsPromise,
        on_fulfilled: Option<JsFunction>,
        on_rejected: Option<JsFunction>,
        result_capability: Option<PromiseCapability<BoaTypes>>,
    ) -> Completion<JsValue, BoaTypes> {
        let result = into_completion(
            promise.then(on_fulfilled, on_rejected, &mut self.context),
            &mut self.context,
        )?;

        // If a result_capability was provided, pipe the .then() result
        // through the capability's promise by chaining a second .then().
        // This ensures callers that create a PromiseCapability and pass
        // it to perform_promise_then (e.g. stream code) have their
        // capability's promise properly resolved/rejected.
        if let Some(cap) = result_capability {
            let realm = self.context.realm().clone();

            // Create resolve wrapper: call cap.resolve with the value
            let resolve = cap.resolve;
            let resolve_native = NativeFunction::from_copy_closure_with_captures(
                move |_this, args, captures, context| {
                    let value = args
                        .first()
                        .cloned()
                        .unwrap_or_else(|| JsValue::undefined());
                    let resolve = &captures.func;
                    resolve.call(&JsValue::undefined(), &[value], context)
                },
                PromiseThenResolve { func: resolve },
            );
            let resolve_fn: JsFunction = FunctionObjectBuilder::new(&realm, resolve_native)
                .name(boa_engine::js_string!(""))
                .length(1)
                .build();

            // Create reject wrapper: call cap.reject with the reason
            let reject = cap.reject;
            let reject_native = NativeFunction::from_copy_closure_with_captures(
                move |_this, args, captures, context| {
                    let reason = args
                        .first()
                        .cloned()
                        .unwrap_or_else(|| JsValue::undefined());
                    let reject = &captures.func;
                    reject.call(&JsValue::undefined(), &[reason], context)
                },
                PromiseThenReject { func: reject },
            );
            let reject_fn: JsFunction = FunctionObjectBuilder::new(&realm, reject_native)
                .name(boa_engine::js_string!(""))
                .length(1)
                .build();

            // Chain .then() on the result promise to pipe through to capability
            let _ = result.then(Some(resolve_fn), Some(reject_fn), &mut self.context);

            // Return resultCapability.[[Promise]] per spec step.
            return Ok(JsValue::from(cap.promise));
        }

        Ok(JsValue::from(result))
    }

    fn promise_state(
        &mut self,
        promise: &JsObject,
    ) -> Completion<crate::enums::PromiseState<BoaTypes>, BoaTypes> {
        let js_promise = JsPromise::from_object(promise.clone()).map_err(|native_error| {
            let js_error: boa_engine::JsError = native_error.into();
            js_error
                .into_opaque(&mut self.context)
                .unwrap_or_else(|_| JsValue::undefined())
        })?;
        Ok(match js_promise.state() {
            boa_engine::builtins::promise::PromiseState::Pending => {
                crate::enums::PromiseState::Pending
            }
            boa_engine::builtins::promise::PromiseState::Fulfilled(v) => {
                crate::enums::PromiseState::Fulfilled(v)
            }
            boa_engine::builtins::promise::PromiseState::Rejected(v) => {
                crate::enums::PromiseState::Rejected(v)
            }
        })
    }

    // ── §27.5 Generator ───────────────────────────────────────────────────

    fn generator_start(
        &mut self,
        _generator: JsGenerator,
        _closure: JsFunction,
    ) -> Completion<(), BoaTypes> {
        todo!("Boa generator_start")
    }

    // ── Global Object Access ──────────────────────────────────────────────

    fn global_object(&self) -> JsObject {
        self.context.global_object()
    }

    // ── Property Key Construction ─────────────────────────────────────────

    fn property_key_from_str(&self, s: &str) -> PropertyKey {
        PropertyKey::from(boa_engine::js_string!(s))
    }

    fn property_key_from_index(&self, index: u32) -> PropertyKey {
        PropertyKey::Index(
            boa_engine::property::NonMaxU32::new(index)
                .expect("property_key_from_index: index exceeds NonMaxU32 range"),
        )
    }

    fn property_key_from_symbol(&self, sym: &JsSymbol) -> PropertyKey {
        PropertyKey::from(sym.clone())
    }

    fn value_from_property_key(&mut self, key: PropertyKey) -> JsValue {
        JsValue::from(key)
    }

    fn property_key_from_well_known_symbol(&mut self, name: &str) -> PropertyKey {
        let sym = match name {
            "asyncIterator" => JsSymbol::async_iterator(),
            "hasInstance" => JsSymbol::has_instance(),
            "isConcatSpreadable" => JsSymbol::is_concat_spreadable(),
            "iterator" => JsSymbol::iterator(),
            "match" => JsSymbol::r#match(),
            "matchAll" => JsSymbol::match_all(),
            "replace" => JsSymbol::replace(),
            "search" => JsSymbol::search(),
            "species" => JsSymbol::species(),
            "split" => JsSymbol::split(),
            "toPrimitive" => JsSymbol::to_primitive(),
            "toStringTag" => JsSymbol::to_string_tag(),
            "unscopables" => JsSymbol::unscopables(),
            "dispose" => JsSymbol::dispose(),
            "asyncDispose" => JsSymbol::async_dispose(),
            _ => {
                // Fallback: treat as a string key
                return PropertyKey::from(boa_engine::js_string!(name));
            }
        };
        PropertyKey::from(sym)
    }

    fn property_key_to_rust_string(&self, key: &PropertyKey) -> String {
        match key {
            PropertyKey::String(s) => s.to_std_string_escaped(),
            PropertyKey::Symbol(sym) => format!(
                "Symbol({})",
                sym.description()
                    .map(|s| s.to_std_string_escaped())
                    .unwrap_or_default()
            ),
            PropertyKey::Index(i) => i.get().to_string(),
        }
    }

    // ── Host-Defined Data Store ───────────────────────────────────────────

    // ── Error Reporting ──────────────────────────────────────────────────

    fn report_error(&mut self, message: &str) {
        log::error!("unhandled exception: {message}");
    }

    // ── Host-Defined Data Store (type-erased) ──────────────────────────

    fn store_host_any(&mut self, id: std::any::TypeId, value: Box<dyn std::any::Any>) {
        let mut host_any_map = self
            .context
            .remove_data::<HostAnyMap>()
            .map(|boxed| *boxed)
            .unwrap_or_default();
        host_any_map.0.insert(id, value);
        self.context.insert_data(host_any_map);
    }

    fn get_host_any(&self, id: &std::any::TypeId) -> Option<&dyn std::any::Any> {
        self.context
            .get_data::<HostAnyMap>()
            .and_then(|host_any_map| host_any_map.0.get(id).map(|boxed| boxed.as_ref()))
    }

    fn remove_host_any(&mut self, id: &std::any::TypeId) -> Option<Box<dyn std::any::Any>> {
        let mut host_any_map = *self.context.remove_data::<HostAnyMap>()?;
        let removed = host_any_map.0.remove(id);
        if !host_any_map.0.is_empty() {
            self.context.insert_data(host_any_map);
        }
        removed
    }

    // ── Platform Object Creation ─────────────────────────────────────────

    fn create_object_with_any(
        &mut self,
        prototype: JsObject,
        data: Box<dyn std::any::Any + 'static>,
    ) -> JsObject {
        // The data may already be wrapped in a `TraceableBox` (when called
        // from `create_interface_instance`), or it may be raw data (when
        // called for prototypes, namespace objects, etc.).  Try to recover
        // the `TraceableBox` first, otherwise wrap in a no-op box.
        // The data may already be wrapped in a `TraceableBox` (when called
        // from `create_interface_instance` / `host_hooks.rs`), or it may be
        // raw data (prototype objects, namespace objects, etc.).  In the
        // latter case, use no-op tracing — safe because those objects don't
        // contain `GcCell<T>` fields.
        let traceable = match data.downcast::<TraceableBox>() {
            Ok(boxed) => *boxed,
            Err(raw) => TraceableBox::noop(raw),
        };
        let wrapper = NativeDataWrapper(traceable);
        JsObject::from_proto_and_data(Some(prototype), wrapper)
    }

    fn with_object_any(&self, object: &JsObject) -> Option<&dyn std::any::Any> {
        // Use try_borrow instead of downcast_ref to avoid panicking when
        // the JsObject's GcRefCell is already mutably borrowed (e.g. during
        // re-entrant property access inside Boa's VM).
        if !object.is::<NativeDataWrapper>() {
            return None;
        }
        let borrow = object.try_borrow().ok()?;
        // SAFETY: we verified is::<NativeDataWrapper>(), so the data is
        // Object<NativeDataWrapper>.  GcRef::cast changes only the type
        // parameter, keeping the same Ref guard valid.
        let cast: boa_gc::GcRef<'_, boa_engine::object::Object<NativeDataWrapper>> =
            unsafe { boa_gc::GcRef::cast(borrow) };
        // SAFETY: The TraceableBox lives in the GC heap and outlives this
        // function call.
        Some(unsafe { &*(cast.data().0.as_any_ref() as *const dyn std::any::Any) })
    }

    fn with_object_any_mut(&mut self, object: &JsObject) -> Option<&mut dyn std::any::Any> {
        let mut wrapper = object.downcast_mut::<NativeDataWrapper>()?;
        // SAFETY: The TraceableBox lives in the JsObject's GC heap, which
        // outlives this function call.  The RefMut guard is temporary but
        // the pointed-to data remains valid as long as the JsObject is alive.
        Some(unsafe { &mut *(wrapper.0.as_any_mut() as *mut dyn std::any::Any) })
    }

    fn with_object_any_mut_with(
        &mut self,
        object: &JsObject,
        f: Box<dyn FnOnce(&mut dyn std::any::Any, &mut dyn ExecutionContext<BoaTypes>) + '_>,
    ) {
        let mut wrapper = match object.downcast_mut::<NativeDataWrapper>() {
            Some(w) => w,
            None => return,
        };
        // SAFETY: Same as `with_object_any_mut` — the data lives in the GC heap.
        let data: &mut dyn std::any::Any =
            unsafe { &mut *(wrapper.0.as_any_mut() as *mut dyn std::any::Any) };
        let ec: &mut dyn ExecutionContext<BoaTypes> = self;
        f(data, ec);
    }

    fn new_type_error(&mut self, msg: &str) -> JsValue {
        let owned: String = msg.to_string();
        let err_obj = JsNativeError::typ()
            .with_message(owned)
            .into_opaque(&mut self.context);
        JsValue::from(err_obj)
    }

    fn new_range_error(&mut self, msg: &str) -> JsValue {
        let owned: String = msg.to_string();
        let err_obj = JsNativeError::range()
            .with_message(owned)
            .into_opaque(&mut self.context);
        JsValue::from(err_obj)
    }

    fn new_syntax_error(&mut self, msg: &str) -> JsValue {
        let owned: String = msg.to_string();
        let err_obj = JsNativeError::syntax()
            .with_message(owned)
            .into_opaque(&mut self.context);
        JsValue::from(err_obj)
    }

    fn create_proxy(
        &mut self,
        target: boa_engine::JsObject,
        handler: boa_engine::JsObject,
    ) -> Completion<boa_engine::JsObject, BoaTypes> {
        let proxy_ctor = self
            .context
            .intrinsics()
            .constructors()
            .proxy()
            .constructor();
        let target_val = boa_engine::JsValue::from(target);
        let handler_val = boa_engine::JsValue::from(handler);
        proxy_ctor
            .construct(
                &[target_val, handler_val],
                Some(&proxy_ctor),
                &mut self.context,
            )
            .map_err(|e| {
                e.into_opaque(&mut self.context)
                    .unwrap_or_else(|_| boa_engine::JsValue::undefined())
            })
    }

    // ── String Utilities ─────────────────────────────────────────────

    fn js_string_to_rust_string(&self, s: &boa_engine::JsString) -> String {
        s.to_std_string_escaped()
    }

    // ── Array Construction ───────────────────────────────────────────

    fn create_empty_array(&mut self) -> JsObject {
        boa_engine::object::builtins::JsArray::new(&mut self.context)
            .expect("JsArray::new should not fail")
            .into()
    }

    fn array_push(&mut self, array: &JsObject, value: JsValue) -> Completion<(), BoaTypes> {
        // Use the "length" property to determine the next index.
        let length_val = into_completion(
            array.get(
                PropertyKey::from(boa_engine::js_string!("length")),
                &mut self.context,
            ),
            &mut self.context,
        )?;
        let length = length_val.to_length(&mut self.context).map_err(|e| {
            e.into_opaque(&mut self.context)
                .unwrap_or_else(|_| JsValue::undefined())
        })?;
        let index_key = PropertyKey::from(length);
        // Set the value at the new index and update length.
        into_completion(
            array.set(index_key, value, false, &mut self.context),
            &mut self.context,
        )?;
        let new_length = length + 1;
        into_completion(
            array.set(
                PropertyKey::from(boa_engine::js_string!("length")),
                JsValue::new(new_length as f64),
                false,
                &mut self.context,
            ),
            &mut self.context,
        )?;
        Ok(())
    }

    // ── Object Construction ──────────────────────────────────────────

    fn create_plain_object(&mut self, prototype: Option<&JsObject>) -> JsObject {
        let proto = prototype.cloned();
        JsObject::from_proto_and_data(proto, ())
    }

    fn json_stringify(&mut self, value: JsValue) -> Completion<String, BoaTypes> {
        let ctx = &mut self.context;
        let result = value.to_json(ctx);
        match result {
            Ok(v) => Ok(
                serde_json::to_string(&v.unwrap_or(serde_json::Value::Null)).unwrap_or_default()
            ),
            Err(e) => Err(e.into_opaque(ctx).unwrap_or(JsValue::undefined())),
        }
    }

    fn value_from_bigint(&mut self, n: i64) -> JsValue {
        JsValue::from(JsBigInt::from(n))
    }

    fn evaluate_script(&mut self, source: &str) -> Completion<JsValue, BoaTypes> {
        into_completion(
            self.context.eval(boa_engine::Source::from_bytes(source)),
            &mut self.context,
        )
    }
}

/// A type-erased container for platform object data that preserves GC
/// tracing information through function pointers.
///
/// Wraps `Box<dyn Any>` plus vtable-like function pointers for
/// `boa_gc::Trace`/`boa_gc::Finalize` dispatch.  The function pointers
/// are set at construction time based on the concrete type `T`, allowing
/// the GC to traverse into platform object fields (like `GcCell<T>`)
/// even after the concrete type has been erased to `dyn Any`.
///
/// # Safety
///
/// The caller must ensure `T` implements `boa_gc::Trace` + `boa_gc::Finalize`.
/// This is guaranteed for `#[gc_struct]` types.
pub struct TraceableBox {
    inner: Box<dyn std::any::Any>,
    // SAFETY: These function pointers must be valid for the lifetime of
    // `inner` and must correctly trace/finalize the concrete type inside.
    trace_fn: unsafe fn(&dyn std::any::Any, &mut boa_gc::Tracer),
    trace_non_roots_fn: unsafe fn(&dyn std::any::Any),
    finalize_fn: fn(&dyn std::any::Any),
}

impl TraceableBox {
    /// Create a new `TraceableBox` for a concrete type that implements
    /// `Trace` + `Finalize`.
    pub fn new<T: std::any::Any + boa_gc::Trace + boa_gc::Finalize>(data: T) -> Self {
        unsafe fn trace_impl<T: boa_gc::Trace + 'static>(
            data: &dyn std::any::Any,
            tracer: &mut boa_gc::Tracer,
        ) {
            // SAFETY: The data was created with Box::new(data) where
            // data: T, so downcast_ref<T>() always succeeds.
            unsafe {
                boa_gc::Trace::trace(data.downcast_ref::<T>().unwrap_unchecked(), tracer);
            }
        }
        unsafe fn trace_non_roots_impl<T: boa_gc::Trace + 'static>(data: &dyn std::any::Any) {
            unsafe {
                boa_gc::Trace::trace_non_roots(data.downcast_ref::<T>().unwrap_unchecked());
            }
        }
        fn finalize_impl<T: boa_gc::Trace + 'static>(data: &dyn std::any::Any) {
            data.downcast_ref::<T>().unwrap().run_finalizer();
        }
        TraceableBox {
            inner: Box::new(data),
            trace_fn: trace_impl::<T>,
            trace_non_roots_fn: trace_non_roots_impl::<T>,
            finalize_fn: finalize_impl::<T>,
        }
    }

    /// Create a no-op `TraceableBox` for data that does NOT contain GC
    /// roots (e.g. unit values for prototype objects).  This is the
    /// fallback used when `create_object_with_any` receives data that
    /// was not wrapped via `TraceableBox::new`.
    pub(crate) fn noop(data: Box<dyn std::any::Any + 'static>) -> Self {
        TraceableBox {
            inner: data,
            trace_fn: |_, _| {},
            trace_non_roots_fn: |_| {},
            finalize_fn: |_| {},
        }
    }

    /// Downcast the inner data to a concrete type.
    pub fn downcast_ref<T: std::any::Any>(&self) -> Option<&T> {
        self.inner.downcast_ref::<T>()
    }

    /// Downcast the inner data to a concrete type (mutable).
    pub fn downcast_mut<T: std::any::Any>(&mut self) -> Option<&mut T> {
        self.inner.downcast_mut::<T>()
    }

    /// Access the inner data as `&dyn Any`.
    pub fn as_any_ref(&self) -> &dyn std::any::Any {
        &*self.inner
    }

    /// Access the inner data as `&mut dyn Any`.
    pub fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        &mut *self.inner
    }

    /// Safe access: call the stored trace function pointer.
    pub(crate) unsafe fn trace_data(&self, tracer: &mut boa_gc::Tracer) {
        unsafe { (self.trace_fn)(&*self.inner, tracer) }
    }

    pub(crate) unsafe fn trace_non_roots_data(&self) {
        unsafe { (self.trace_non_roots_fn)(&*self.inner) }
    }

    pub(crate) fn finalize_data(&self) {
        (self.finalize_fn)(&*self.inner);
    }
}

// SAFETY: The trace/finalize functions are bound to the correct concrete
// type at construction time.  For `noop` boxes the functions are no-ops.
unsafe impl boa_gc::Trace for TraceableBox {
    unsafe fn trace(&self, tracer: &mut boa_gc::Tracer) {
        // SAFETY: trace_data calls the type-specific trace fn.
        unsafe { self.trace_data(tracer) };
    }
    unsafe fn trace_non_roots(&self) {
        unsafe { self.trace_non_roots_data() };
    }
    fn run_finalizer(&self) {
        self.finalize_data();
    }
}

impl boa_gc::Finalize for TraceableBox {}
impl boa_engine::JsData for TraceableBox {}

/// Wrapper that implements `NativeObject` for data stored in JS objects.
///
/// The inner `TraceableBox` preserves GC tracing through the type-erased
/// storage, ensuring that `GcCell<T>` fields inside platform objects
/// remain visible to the Boa GC.
///
/// Used by `create_object_with_any` and retrieved via `with_object_any`.
pub struct NativeDataWrapper(pub TraceableBox);
/// Type-erased storage wrapper for the host-defined data store.
#[derive(Default)]
struct HostAnyMap(std::collections::HashMap<std::any::TypeId, Box<dyn std::any::Any>>);

// SAFETY: The stored data is only accessed through type-safe downcasts.
unsafe impl boa_gc::Trace for HostAnyMap {
    unsafe fn trace(&self, _: &mut boa_gc::Tracer) {}
    unsafe fn trace_non_roots(&self) {}
    fn run_finalizer(&self) {}
}
impl boa_gc::Finalize for HostAnyMap {}

// SAFETY: Delegates to `TraceableBox` which has type-correct trace/finalize
// function pointers bound at construction time.  On the `noop` path (data
// without GC roots) the functions are no-ops.
unsafe impl boa_gc::Trace for NativeDataWrapper {
    unsafe fn trace(&self, tracer: &mut boa_gc::Tracer) {
        // SAFETY: TraceableBox::trace safely delegates to its stored fn.
        unsafe { self.0.trace(tracer) };
    }
    unsafe fn trace_non_roots(&self) {
        unsafe { self.0.trace_non_roots() };
    }
    fn run_finalizer(&self) {
        self.0.run_finalizer();
    }
}

impl boa_gc::Finalize for NativeDataWrapper {}

impl boa_engine::JsData for NativeDataWrapper {}

// ═══════════════════════════════════════════════════════════════════════════
// EcmascriptHost<BoaTypes> — Web IDL callback operations
// ═══════════════════════════════════════════════════════════════════════════

impl EcmascriptHost<BoaTypes> for BoaContext {
    fn get(&mut self, object: &JsObject, property: &str) -> Completion<JsValue, BoaTypes> {
        into_completion(
            object.get(
                PropertyKey::from(boa_engine::js_string!(property)),
                &mut self.context,
            ),
            &mut self.context,
        )
    }

    fn is_callable(&self, value: &JsValue) -> bool {
        value.as_object().is_some_and(|o| o.is_callable())
    }

    fn call(
        &mut self,
        callable: &JsObject,
        this_arg: &JsValue,
        args: &[JsValue],
    ) -> Completion<JsValue, BoaTypes> {
        let function = JsFunction::from_object(callable.clone()).ok_or_else(|| {
            JsValue::from(
                JsNativeError::typ()
                    .with_message("callback is not callable")
                    .into_opaque(&mut self.context),
            )
        })?;
        into_completion(
            function.call(this_arg, args, &mut self.context),
            &mut self.context,
        )
    }

    fn perform_a_microtask_checkpoint(&mut self) -> Completion<(), BoaTypes> {
        let _ = self.context.run_jobs();
        Ok(())
    }

    fn report_exception(&mut self, error: JsValue) {
        let message = error.to_string(&mut self.context).ok().map_or_else(
            || "unknown error".to_string(),
            |s| s.to_std_string_escaped(),
        );
        log::error!("uncaught callback error: {message}");
    }

    fn gc(&mut self) {
        boa_gc::force_collect();
    }

    fn value_undefined(&mut self) -> JsValue {
        JsValue::undefined()
    }

    fn value_null(&mut self) -> JsValue {
        JsValue::null()
    }

    fn value_from_bool(&mut self, b: bool) -> JsValue {
        JsValue::from(b)
    }

    fn value_from_number(&mut self, n: f64) -> JsValue {
        JsValue::from(n)
    }

    fn value_from_string(&mut self, s: boa_engine::JsString) -> JsValue {
        JsValue::from(s)
    }

    fn js_string_from_str(&self, s: &str) -> boa_engine::JsString {
        boa_engine::js_string!(s)
    }
}

/// §7.4.3 GetIteratorFromMethod ( obj, method )
fn get_iterator_from_method(
    engine: &mut BoaContext,
    obj: JsValue,
    method: JsFunction,
) -> Completion<IteratorRecord<BoaTypes>, BoaTypes> {
    let context = &mut engine.context;
    let iterator = into_completion(method.call(&obj, &[], context), context)?;
    let iterator_obj = iterator.as_object().ok_or_else(|| {
        JsValue::from(
            JsNativeError::typ()
                .with_message("iterator result is not an object")
                .into_opaque(context),
        )
    })?;
    let next_value = into_completion(
        iterator_obj.get(PropertyKey::from(boa_engine::js_string!("next")), context),
        context,
    )?;
    let next_method = JsFunction::from_object(next_value.as_object().ok_or_else(|| {
        JsValue::from(
            JsNativeError::typ()
                .with_message("iterator next method is not a function")
                .into_opaque(context),
        )
    })?)
    .ok_or_else(|| {
        JsValue::from(
            JsNativeError::typ()
                .with_message("iterator next method is not a function")
                .into_opaque(context),
        )
    })?;
    Ok(IteratorRecord {
        iterator: iterator_obj,
        next_method,
        done: false,
    })
}

fn boa_property_key_to_value(property_key: &PropertyKey) -> JsValue {
    match property_key {
        PropertyKey::String(string) => JsValue::from(string.clone()),
        PropertyKey::Symbol(symbol) => JsValue::from(symbol.clone()),
        PropertyKey::Index(index) => JsValue::from(index.get()),
    }
}

fn descriptor_field_value(
    descriptor_obj: &JsObject,
    field_name: &str,
    context: &mut Context,
) -> Completion<Option<JsValue>, BoaTypes> {
    let field_key = PropertyKey::from(boa_engine::js_string!(field_name));
    let present = descriptor_obj
        .has_property(field_key.clone(), context)
        .map_err(|error| {
            error
                .into_opaque(context)
                .unwrap_or_else(|_| JsValue::undefined())
        })?;
    if !present {
        return Ok(None);
    }
    let value = descriptor_obj.get(field_key, context).map_err(|error| {
        error
            .into_opaque(context)
            .unwrap_or_else(|_| JsValue::undefined())
    })?;
    Ok(Some(value))
}

/// Check if an object has a given property.
fn has_property(
    ec: &mut dyn ExecutionContext<BoaTypes>,
    obj: &JsObject,
    field_name: &str,
) -> Completion<bool, BoaTypes> {
    let key = PropertyKey::from(boa_engine::js_string!(field_name));
    let field_key: PropertyKey = key;
    ec.has_property(obj.clone(), field_key)
}

/// If an object has a given property, return its ToBoolean value.
/// Returns `Ok(None)` if the property does not exist.
fn has_property_then_get_boolean(
    ec: &mut dyn ExecutionContext<BoaTypes>,
    obj: &JsObject,
    field_name: &str,
) -> Completion<Option<bool>, BoaTypes> {
    let field_key = PropertyKey::from(boa_engine::js_string!(field_name));
    let present = ec.has_property(obj.clone(), field_key.clone())?;
    if !present {
        return Ok(None);
    }
    // Use ExecutionContext::get (takes PropertyKey), not EcmascriptHost::get (takes &str).
    let val = ExecutionContext::get(ec, obj.clone(), field_key)?;
    Ok(Some(ec.to_boolean(&val)))
}

/// <https://tc39.es/ecma262/#sec-createbuiltinfunction>
///
/// Creates a built-in function whose captures are stored as a concrete
/// traceable type `C`, enabling Boa's GC to trace through JS-object
/// references inside the captures.
///
/// Use this instead of [`BoaContext::create_builtin_function`] when the
/// behaviour closure captures values that contain `JsObject` or `GcCell`
/// fields (e.g., stream controllers, readers, promises).  The default
/// `create_builtin_function` wraps all captures in a `Box<dyn Fn(...)>`
/// with no-op GC tracing, which can cause "not a callable function" errors
/// when Boa's GC collects the captured objects.
///
/// The `C` type must implement `boa_gc::Trace + 'static`.  For `#[gc_struct]`
/// types this is automatically derived.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExecutionContext, JsTypes};

    /// Verify that `create_builtin_function` creates a JS-callable function
    /// that can receive arguments and return values through the generic trait.
    #[test]
    fn create_builtin_function_doubles() {
        let mut engine = BoaContext::new();

        // Create a function that doubles its first argument using
        // generic ExecutionContext operations (to_number, value_from_number).
        let double_fn: JsFunction = create_builtin_fn_with_captures_impl(
            &mut engine,
            NoCaptures,
            |args: &[JsValue],
             _this: JsValue,
             _captures: &NoCaptures,
             host: &mut dyn ExecutionContext<BoaTypes>| {
                let n = host.to_number(args.first().cloned().unwrap_or(JsValue::undefined()))?;
                Ok(host.value_from_number(n * 2.0))
            },
            1,
            PropertyKey::from(boa_engine::js_string!("double")),
            false,
        );

        // Register the function on the global object.
        let global = engine.context.global_object();
        let _ = global.set(
            PropertyKey::from(boa_engine::js_string!("double")),
            JsValue::from(double_fn),
            false,
            &mut engine.context,
        );

        // Call it from JS and check the result.
        let result = engine
            .context
            .eval(boa_engine::Source::from_bytes("double(21)"))
            .expect("eval should succeed");
        assert_eq!(result.as_number(), Some(42.0));
    }

    #[test]
    fn value_construction_and_downcasts() {
        let mut engine = BoaContext::new();
        let undef = engine.value_undefined();
        let null = engine.value_null();
        let bool_val = engine.value_from_bool(true);
        let num_val = engine.value_from_number(42.0);
        let str_val = engine.value_from_string(boa_engine::js_string!("hello"));

        assert!(BoaTypes::value_is_undefined(&undef));
        assert!(BoaTypes::value_is_null(&null));
        assert_eq!(BoaTypes::value_as_bool(&bool_val), Some(true));
        assert!((BoaTypes::value_as_number(&num_val).unwrap() - 42.0).abs() < 0.001);
        assert!(BoaTypes::value_as_string(&str_val).is_some());
        assert!(BoaTypes::value_as_object(&num_val).is_none());
    }

    #[test]
    fn type_conversion_to_number_and_string() {
        let mut engine = BoaContext::new();
        let num = engine.value_from_number(42.5);
        let n = engine.to_number(num).unwrap();
        assert!((n - 42.5).abs() < 0.001);

        let num_val = engine.value_from_number(123.0);
        let s = engine.to_rust_string(num_val).unwrap();
        assert_eq!(s, "123");
    }

    #[test]
    fn create_plain_object_and_array() {
        let mut engine = BoaContext::new();
        let obj = engine.create_plain_object(None);
        let val = engine.value_from_number(99.0);
        engine.object_set_property(obj.clone(), "x", val).unwrap();

        let pk = engine.property_key_from_str("x");
        let retrieved = ExecutionContext::get(&mut engine, obj, pk).unwrap();
        let n = engine.to_number(retrieved).unwrap();
        assert!((n - 99.0).abs() < 0.001);

        let arr = engine.create_empty_array();
        let v10 = engine.value_from_number(10.0);
        engine.array_push(&arr, v10).unwrap();
        let v20 = engine.value_from_number(20.0);
        engine.array_push(&arr, v20).unwrap();
        let pk0 = engine.property_key_from_index(0);
        let v0 = ExecutionContext::get(&mut engine, arr.clone(), pk0).unwrap();
        assert!((engine.to_number(v0).unwrap() - 10.0).abs() < 0.001);
    }

    #[test]
    fn error_construction() {
        let mut engine = BoaContext::new();
        let type_err = engine.new_type_error("bad type");
        let range_err = engine.new_range_error("out of range");
        assert!(type_err.is_object());
        assert!(range_err.is_object());
    }

    #[test]
    fn host_data_store() {
        let mut engine = BoaContext::new();
        let id = std::any::TypeId::of::<String>();
        engine.store_host_any(id, Box::new("test data".to_string()));
        assert!(engine.get_host_any(&id).is_some());
        assert!(engine.remove_host_any(&id).is_some());
        assert!(engine.get_host_any(&id).is_none());
    }

    #[test]
    fn realm_intrinsics_finds_constructors() {
        let engine = BoaContext::new();
        let realm = engine.current_realm();
        let intrinsics = engine.realm_intrinsics(&realm);
        let _ = intrinsics.object;
        let _ = intrinsics.promise;
        let _ = intrinsics.array;
    }

    #[test]
    fn evaluate_script_via_engine() {
        let mut engine = BoaContext::new();
        let realm = engine.create_realm();
        let result = crate::JsEngine::evaluate_script(&mut engine, "40 + 2", &realm).unwrap();
        let n = engine.to_number(result).unwrap();
        assert!((n - 42.0).abs() < 0.001);
    }

    #[test]
    fn promise_new_capability_and_resolve() {
        let mut engine = BoaContext::new();
        let realm = engine.current_realm();
        let intrinsics = engine.realm_intrinsics(&realm);
        let pcap = engine.new_promise_capability(intrinsics.promise).unwrap();
        assert!(pcap.promise.is_object());

        let undef = engine.value_undefined();
        let val = engine.value_from_number(7.0);
        let result = EcmascriptHost::call(&mut engine, &pcap.resolve, &undef, &[val]);
        assert!(result.is_ok());
    }

    #[test]
    fn is_callable_and_call() {
        let mut engine = BoaContext::new();
        let realm = engine.current_realm();
        let fn_val = crate::JsEngine::evaluate_script(
            &mut engine,
            "(function(x) { return x * 2; })",
            &realm,
        )
        .unwrap();
        assert!(engine.is_callable(&fn_val));
        let fn_obj = fn_val.as_object().unwrap().clone();
        let undef = engine.value_undefined();
        let arg = engine.value_from_number(21.0);
        let result = EcmascriptHost::call(&mut engine, &fn_obj, &undef, &[arg]).unwrap();
        let n = engine.to_number(result).unwrap();
        assert!((n - 42.0).abs() < 0.001);
    }

    #[test]
    fn same_value_and_comparison() {
        let mut engine = BoaContext::new();
        let v1 = engine.value_from_number(1.0);
        let v2 = engine.value_from_number(1.0);
        let v3 = engine.value_from_number(2.0);
        assert!(engine.same_value(&v1, &v2));
        assert!(!engine.same_value(&v1, &v3));
    }

    #[test]
    fn allocate_array_buffer_via_engine() {
        let mut engine = BoaContext::new();
        let realm = engine.current_realm();
        let intrinsics = engine.realm_intrinsics(&realm);
        let ab =
            JsEngine::allocate_array_buffer(&mut engine, intrinsics.array_buffer, 8, None).unwrap();
        let _ = ab;
    }
}

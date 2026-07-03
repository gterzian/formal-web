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
//! - `enqueue_job` — no-op (Boa job trait not wired)
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
    },
    property::PropertyKey,
    value::PreferredType as BoaPreferredType,
};

use crate::{
    Completion, EcmascriptHost, ExecutionContext, HostHooks, IntegrityLevel, IteratorKind,
    JsEngine, JsTypesWithRealm, Numeric, PreferredType, SharedMemoryOrder, TypedArrayElementType,
    records::{
        IteratorRecord, PromiseCapability, PromiseResolvers, PropertyDescriptor, RealmIntrinsics,
    },
};

use super::types::BoaTypes;

// ── GC Trace for Behaviour trait object ────────────────────────────
//
// `Box<dyn Behaviour<BoaTypes>>` is passed as captures to
// `NativeFunction::from_copy_closure_with_captures`, which requires
// `Trace`.  The trait object itself holds no GC-managed data — the
// concrete captures inside the Behaviour impl are already rooted by
// their parent stream/controller objects.
unsafe impl boa_gc::Trace for dyn crate::Behaviour<BoaTypes> {
    unsafe fn trace(&self, _tracer: &mut boa_gc::Tracer) {}
    unsafe fn trace_non_roots(&self) {}
    fn run_finalizer(&self) {}
}

impl boa_gc::Finalize for dyn crate::Behaviour<BoaTypes> {}

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

    // ── §10.3 Built-in Function Objects ──────────────────────────────────

    /// <https://tc39.es/ecma262/#sec-createbuiltinfunction>
    fn create_builtin_function_with_captures<C: crate::gc::Trace + boa_engine::Trace + 'static>(
        &mut self,
        captures: C,
        behaviour: fn(
            &[JsValue],
            JsValue,
            &C,
            &mut dyn ExecutionContext<BoaTypes>,
        ) -> Completion<JsValue, BoaTypes>,
        length: u32,
        name: PropertyKey,
    ) -> JsFunction {
        let realm = self.current_realm();
        let name_str = match &name {
            PropertyKey::String(s) => s.clone(),
            PropertyKey::Symbol(_) => boa_engine::js_string!(""),
            _ => boa_engine::js_string!(""),
        };

        let native = NativeFunction::from_copy_closure_with_captures(
            move |_this: &JsValue,
                  args: &[JsValue],
                  captures: &C,
                  context: &mut Context|
                  -> JsResult<JsValue> {
                let engine: &mut BoaContext =
                    unsafe { &mut *(context as *mut Context as *mut BoaContext) };
                behaviour(args, _this.clone(), captures, engine)
                    .map_err(|e| JsError::from_opaque(e))
            },
            captures,
        );

        FunctionObjectBuilder::new(&realm, native)
            .name(name_str)
            .length(length as usize)
            .build()
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
        let number = into_completion(value.to_number(&mut self.context), &mut self.context)?;
        if number.is_nan() || number <= 0.0 {
            Ok(0)
        } else {
            Ok((number.min(f64::from(u32::MAX))) as u64)
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
        let n = into_completion(value.to_number(&mut self.context), &mut self.context)?;
        if n.is_nan() || n < 0.0 || !n.is_finite() {
            return Err(JsValue::from(
                JsNativeError::range()
                    .with_message("Invalid index")
                    .into_opaque(&mut self.context),
            ));
        }
        let integer = n.trunc() as u64;
        if integer as f64 != n || integer > 9007199254740992 {
            return Err(JsValue::from(
                JsNativeError::range()
                    .with_message("Invalid index")
                    .into_opaque(&mut self.context),
            ));
        }
        Ok(integer)
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
        let enumerable_val = crate::EcmascriptHost::get(self, &desc_obj, "enumerable")?;
        let configurable_val = crate::EcmascriptHost::get(self, &desc_obj, "configurable")?;
        let value = {
            let val = crate::EcmascriptHost::get(self, &desc_obj, "value")?;
            if !val.is_undefined() { Some(val) } else { None }
        };
        let writable_val = crate::EcmascriptHost::get(self, &desc_obj, "writable")?;
        let get_val = crate::EcmascriptHost::get(self, &desc_obj, "get")?;
        let set_val = crate::EcmascriptHost::get(self, &desc_obj, "set")?;

        let enumerable = if !enumerable_val.is_undefined() {
            Some(self.to_boolean(&enumerable_val))
        } else {
            None
        };
        let configurable = if !configurable_val.is_undefined() {
            Some(self.to_boolean(&configurable_val))
        } else {
            None
        };
        let writable = if !writable_val.is_undefined() {
            Some(self.to_boolean(&writable_val))
        } else {
            None
        };
        let get_fn = if !get_val.is_undefined() && !get_val.is_null() {
            let obj = self.to_object(get_val)?;
            JsFunction::from_object(obj)
        } else {
            None
        };
        let set_fn = if !set_val.is_undefined() && !set_val.is_null() {
            let obj = self.to_object(set_val)?;
            JsFunction::from_object(obj)
        } else {
            None
        };

        Ok(PropertyDescriptor {
            value,
            writable,
            get: get_fn,
            set: set_fn,
            enumerable,
            configurable,
        })
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
        let global = self.context.global_object();
        let object_ctor_val = into_completion(
            global.get(
                PropertyKey::from(boa_engine::js_string!("Object")),
                &mut self.context,
            ),
            &mut self.context,
        )?;
        let object_ctor = object_ctor_val.as_object().ok_or_else(|| {
            JsValue::from(
                JsNativeError::typ()
                    .with_message("Object constructor is not available")
                    .into_opaque(&mut self.context),
            )
        })?;
        let descriptor_fn_val = into_completion(
            object_ctor.get(
                PropertyKey::from(boa_engine::js_string!("getOwnPropertyDescriptor")),
                &mut self.context,
            ),
            &mut self.context,
        )?;
        let descriptor_fn =
            JsFunction::from_object(descriptor_fn_val.as_object().ok_or_else(|| {
                JsValue::from(
                    JsNativeError::typ()
                        .with_message("Object.getOwnPropertyDescriptor is not callable")
                        .into_opaque(&mut self.context),
                )
            })?)
            .ok_or_else(|| {
                JsValue::from(
                    JsNativeError::typ()
                        .with_message("Object.getOwnPropertyDescriptor is not callable")
                        .into_opaque(&mut self.context),
                )
            })?;

        let key_value = boa_property_key_to_value(&property_key);
        let descriptor_val = into_completion(
            descriptor_fn.call(
                &JsValue::from(object_ctor.clone()),
                &[JsValue::from(object), key_value],
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
            object_prototype: constructors.object().prototype(),
            function_prototype: constructors.function().prototype(),
            async_iterator_prototype: intrinsics
                .objects()
                .iterator_prototypes()
                .async_iterator(),
        }
    }

    fn realm_global_object(&self) -> JsObject
    where
        BoaTypes: JsTypesWithRealm,
    {
        self.context.global_object()
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

    fn is_detached_buffer(&self, _array_buffer: &JsArrayBuffer) -> bool {
        false // HARD: Boa's JsArrayBuffer doesn't expose is_detached publicly
    }

    fn is_fixed_length_array_buffer(&self, _array_buffer: &JsArrayBuffer) -> bool {
        true
    }

    fn get_value_from_buffer(
        &self,
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
        // HARD: TypedArrayKind is not publicly accessible from Boa.
        // Callers should use BYTES_PER_ELEMENT via JS getter instead.
        let _ = typed_array.kind();
        None
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
        Ok((
            JsValue::from(promise),
            PromiseResolvers {
                resolve: resolvers.resolve.into(),
                reject: resolvers.reject.into(),
            },
        ))
    }

    fn perform_promise_then(
        &mut self,
        promise: JsPromise,
        on_fulfilled: Option<JsFunction>,
        on_rejected: Option<JsFunction>,
        _result_capability: Option<PromiseCapability<BoaTypes>>,
    ) -> Completion<JsValue, BoaTypes> {
        let result = into_completion(
            promise.then(on_fulfilled, on_rejected, &mut self.context),
            &mut self.context,
        )?;
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
        // SAFETY: The data must be downcastable to a type that implements
        // NativeObject.  The caller (create_interface_instance) ensures
        // this by providing data of type T where T: NativeObject + 'static.
        // We use the NativeDataWrapper to satisfy the NativeObject bound.
        // The downcast on retrieval (downcast_ref) uses the correct TypeId.
        let wrapper = NativeDataWrapper(data);
        JsObject::from_proto_and_data(Some(prototype), wrapper)
    }

    fn with_object_any(&self, object: &JsObject) -> Option<&dyn std::any::Any> {
        let wrapper = object.downcast_ref::<NativeDataWrapper<Box<dyn std::any::Any>>>()?;
        // SAFETY: The NativeDataWrapper is stored inside the JsObject which
        // lives in the GC heap rooted by `self`.  The data is valid for the
        // lifetime of `&self`.
        Some(unsafe { &*(wrapper.0.as_ref() as *const dyn std::any::Any) })
    }

    fn with_object_any_mut(&mut self, object: &JsObject) -> Option<&mut dyn std::any::Any> {
        let mut wrapper = object.downcast_mut::<NativeDataWrapper<Box<dyn std::any::Any>>>()?;
        // SAFETY: Same as `with_object_any` — the data lives in the GC heap
        // rooted by `self`.
        Some(unsafe { &mut *(wrapper.0.as_mut() as *mut dyn std::any::Any) })
    }

    fn with_object_any_mut_with(
        &mut self,
        object: &JsObject,
        f: Box<dyn FnOnce(&mut dyn std::any::Any, &mut dyn ExecutionContext<BoaTypes>) + '_>,
    ) {
        let mut wrapper = match object.downcast_mut::<NativeDataWrapper<Box<dyn std::any::Any>>>() {
            Some(w) => w,
            None => return,
        };
        // SAFETY: The NativeDataWrapper lives in the JsObject's GC heap,
        // which is separate from `self` (the BoaContext stack value).
        // `wrapper` is a RefMut guard borrowing from the JsObject's GcCell,
        // not from `self`.  The `&mut dyn ExecutionContext` parameter to `f`
        // borrows `self` — these are independent memory locations.
        let data: &mut dyn std::any::Any =
            unsafe { &mut *(wrapper.0.as_mut() as *mut dyn std::any::Any) };
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
    ) -> JsFunction {
        let realm = self.current_realm();
        let name_str = match &name {
            PropertyKey::String(s) => s.clone(),
            PropertyKey::Symbol(_) => boa_engine::js_string!(""),
            _ => boa_engine::js_string!(""),
        };

        // SAFETY: BoaContext is `#[repr(transparent)]` over Context.
        let native = unsafe {
            NativeFunction::from_closure(Box::new(
                move |this: &JsValue,
                      args: &[JsValue],
                      context: &mut Context|
                      -> JsResult<JsValue> {
                    let engine: &mut BoaContext =
                        &mut *(context as *mut Context as *mut BoaContext);
                    behaviour(args, this.clone(), engine).map_err(|e| JsError::from_opaque(e))
                },
            ))
        };

        FunctionObjectBuilder::new(&realm, native)
            .name(name_str)
            .length(length as usize)
            .build()
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

    fn create_builtin_function_from_behaviour(
        &mut self,
        behaviour: Box<dyn crate::Behaviour<BoaTypes>>,
        length: u32,
        name: PropertyKey,
    ) -> JsFunction {
        let realm = self.current_realm();
        let name_str = match &name {
            PropertyKey::String(s) => s.clone(),
            PropertyKey::Symbol(_) => boa_engine::js_string!(""),
            _ => boa_engine::js_string!(""),
        };

        let native = NativeFunction::from_copy_closure_with_captures(
            move |_this: &JsValue,
                  args: &[JsValue],
                  behaviour: &Box<dyn crate::Behaviour<BoaTypes>>,
                  context: &mut Context|
                  -> JsResult<JsValue> {
                let engine: &mut BoaContext =
                    unsafe { &mut *(context as *mut Context as *mut BoaContext) };
                behaviour
                    .call(args, _this.clone(), engine)
                    .map_err(|e| JsError::from_opaque(e))
            },
            behaviour,
        );

        FunctionObjectBuilder::new(&realm, native)
            .name(name_str)
            .length(length as usize)
            .build()
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
}

/// Wrapper that implements `NativeObject` for arbitrary `'static` data.
///
/// Used by `create_object_with_data` to store Rust data inside Boa objects.
/// The GC traits are no-ops because the content process does not relocate
/// GC'd objects and the data is only accessed through the JS object's
/// internal slot (via `downcast_ref`).
struct NativeDataWrapper<T: std::any::Any + 'static>(T);

// SAFETY: The content process is single-threaded.  `NativeDataWrapper`
// only stores `'static` data that does not contain GC roots.
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

// SAFETY: The wrapped data does not contain GC roots — it is only
// accessed through the JS object's internal slot via `downcast_ref`.
unsafe impl<T: std::any::Any + 'static> boa_gc::Trace for NativeDataWrapper<T> {
    unsafe fn trace(&self, _: &mut boa_gc::Tracer) {}
    unsafe fn trace_non_roots(&self) {}
    fn run_finalizer(&self) {}
}

impl<T: std::any::Any + 'static> boa_gc::Finalize for NativeDataWrapper<T> {}

impl<T: std::any::Any + 'static> boa_engine::JsData for NativeDataWrapper<T> {}

// Note: `NativeDataWrapper<T>` implements `NativeObject` via the blanket
// `impl<T: Any + Trace + JsData> NativeObject for T` in boa_engine.

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
        let double_fn = engine.create_builtin_function(
            Box::new(|args, _this, host| {
                let n = host.to_number(args.first().cloned().unwrap_or(JsValue::undefined()))?;
                Ok(host.value_from_number(n * 2.0))
            }),
            1,
            PropertyKey::from(boa_engine::js_string!("double")),
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
        let result = engine.evaluate_script("40 + 2", &realm).unwrap();
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
        let fn_val = engine
            .evaluate_script("(function(x) { return x * 2; })", &realm)
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

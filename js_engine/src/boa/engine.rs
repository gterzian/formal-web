//! `BoaEngine` — the `JsEngine<BoaTypes>` + `ExecutionContext<BoaTypes>` + `EcmascriptHost<BoaTypes>` impl.
//!
//! ## Layout safety
//!
//! `BoaEngine` is `#[repr(transparent)]` over `Context`.  This enables the
//! `create_builtin_function` shim to safely cast `&mut Context` →
//! `&mut BoaEngine` → `&mut dyn ExecutionContext<BoaTypes>` inside the
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
    Context, JsError, JsNativeError, JsResult, JsSymbol, JsValue,
    builtins::array_buffer::AlignedVec,
    native_function::NativeFunction,
    object::{
        FunctionObjectBuilder, JsObject,
        builtins::{JsArrayBuffer, JsFunction, JsGenerator, JsPromise, JsSharedArrayBuffer},
    },
    property::PropertyKey,
    value::PreferredType as BoaPreferredType,
};

use crate::{
    Completion, EcmascriptHost, ExecutionContext, HostHooks, IntegrityLevel, IteratorKind,
    JsEngine, JsTypesWithRealm, Numeric, PreferredType, SharedMemoryOrder, TypedArrayElementType,
    records::{IteratorRecord, PromiseCapability, PropertyDescriptor, RealmIntrinsics},
};

use super::types::BoaTypes;

/// Boa engine wrapper.  Wraps a `boa_engine::Context` and implements
/// `JsEngine<BoaTypes>`, `ExecutionContext<BoaTypes>`, and
/// `EcmascriptHost<BoaTypes>`.
///
/// # Layout
///
/// `#[repr(transparent)]` guarantees the same memory layout as `Context`,
/// enabling safe pointer casts from `&mut Context` to `&mut BoaEngine`
/// inside the `create_builtin_function` shim.
#[repr(transparent)]
pub struct BoaEngine {
    context: Context,
}

impl BoaEngine {
    pub fn new() -> Self {
        Self {
            context: Context::default(),
        }
    }

    /// Wrap an existing `Context` into a `BoaEngine`.
    ///
    /// Used during migration from direct `Context` ownership to `BoaEngine`
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

impl Default for BoaEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn into_completion<T>(result: JsResult<T>, context: &mut Context) -> Completion<T, BoaTypes> {
    result.map_err(|e| e.into_opaque(context).unwrap_or(JsValue::undefined()))
}

// ═══════════════════════════════════════════════════════════════════════════
// JsEngine<BoaTypes> — factory operations (§9.3, §10.3, §16, §25)
// ═══════════════════════════════════════════════════════════════════════════

impl JsEngine<BoaTypes> for BoaEngine {
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
        realm: &boa_engine::realm::Realm,
    ) -> JsFunction
    where
        BoaTypes: JsTypesWithRealm,
    {
        // Extract the name string for FunctionObjectBuilder.
        let name_str = match &name {
            PropertyKey::String(s) => s.clone(),
            PropertyKey::Symbol(_) => boa_engine::js_string!(""),
            _ => boa_engine::js_string!(""),
        };

        // SAFETY: BoaEngine is `#[repr(transparent)]` over Context, so
        // a `&mut Context` pointer can be safely cast to `&mut BoaEngine`.
        // The resulting reference has the same lifetime as the `context`
        // parameter and does not alias any other mutable reference.
        //
        // The closure is `'static` — `behaviour` is an owned Box that
        // does not borrow from the engine.
        let native = unsafe {
            NativeFunction::from_closure(Box::new(
                move |this: &JsValue,
                      args: &[JsValue],
                      context: &mut Context|
                      -> JsResult<JsValue> {
                    // SAFETY: BoaEngine is repr(transparent) over Context.
                    let engine: &mut BoaEngine = &mut *(context as *mut Context as *mut BoaEngine);
                    behaviour(args, this.clone(), engine).map_err(|e| JsError::from_opaque(e))
                },
            ))
        };

        FunctionObjectBuilder::new(realm, native)
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

impl ExecutionContext<BoaTypes> for BoaEngine {
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

    fn define_property_or_throw(
        &mut self,
        object: JsObject,
        property_key: PropertyKey,
        descriptor: PropertyDescriptor<BoaTypes>,
    ) -> Completion<(), BoaTypes> {
        let boa_desc = boa_engine::property::PropertyDescriptor::builder()
            .maybe_value(descriptor.value)
            .maybe_writable(descriptor.writable)
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
            object_prototype: constructors.object().prototype(),
            function_prototype: constructors.function().prototype(),
        }
    }

    // ── §9.6 Jobs ─────────────────────────────────────────────────────────

    fn enqueue_job(&mut self, _job: Box<dyn FnOnce() + Send>) {
        // HARD: Boa's job executor model requires wrapping jobs for Boa's Job trait
    }

    fn run_jobs(&mut self) {
        let _ = self.context.run_jobs();
    }

    // ── §25 ArrayBuffer — runtime queries ─────────────────────────────────

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

    // ── Host-Defined Data Store ───────────────────────────────────────────

    // ── Error Reporting ──────────────────────────────────────────────────

    fn report_error(&mut self, message: &str) {
        log::error!("unhandled exception: {message}");
    }

    // ── Host-Defined Data Store (type-erased) ──────────────────────────

    fn store_host_any(&mut self, _id: std::any::TypeId, value: Box<dyn std::any::Any>) {
        self.context.insert_data(HostAny(value));
    }

    fn get_host_any(&self, _id: &std::any::TypeId) -> Option<&dyn std::any::Any> {
        self.context.get_data::<HostAny>().map(|h| h.0.as_ref())
    }

    fn remove_host_any(&mut self, _id: &std::any::TypeId) -> Option<Box<dyn std::any::Any>> {
        self.context.remove_data::<HostAny>().map(|h| h.0)
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
struct HostAny(Box<dyn std::any::Any>);

// SAFETY: The stored data is only accessed through type-safe downcasts.
unsafe impl boa_gc::Trace for HostAny {
    unsafe fn trace(&self, _: &mut boa_gc::Tracer) {}
    unsafe fn trace_non_roots(&self) {}
    fn run_finalizer(&self) {}
}
impl boa_gc::Finalize for HostAny {}

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

impl EcmascriptHost<BoaTypes> for BoaEngine {
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
    engine: &mut BoaEngine,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ExecutionContext;

    /// Verify that `create_builtin_function` creates a JS-callable function
    /// that can receive arguments and return values through the generic trait.
    #[test]
    fn create_builtin_function_doubles() {
        let mut engine = BoaEngine::new();
        let realm = engine.context.realm().clone();

        // Create a function that doubles its first argument using
        // generic ExecutionContext operations (to_number, value_from_number).
        let double_fn = engine.create_builtin_function(
            Box::new(|args, _this, host| {
                let n = host.to_number(args.first().cloned().unwrap_or(JsValue::undefined()))?;
                Ok(host.value_from_number(n * 2.0))
            }),
            1,
            PropertyKey::from(boa_engine::js_string!("double")),
            &realm,
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
}

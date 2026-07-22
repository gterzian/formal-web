use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell, RefMut};
use std::collections::{HashMap, VecDeque};
use std::convert::TryFrom;
use std::ffi::c_void;
use std::mem::replace;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::{NonNull, from_ref};
use std::rc::{Rc, Weak as RcWeak};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Once};

use log::error;
use rusty_v8 as v8;

use crate::enums::{
    IntegrityLevel, IteratorKind, PromiseState, SharedMemoryOrder, TypedArrayElementType,
};
use crate::gc::{JsTypesGcExt, Trace};
use crate::records::{IteratorRecord, PromiseCapability, PromiseResolvers, RealmIntrinsics};
use crate::{
    Completion, EcmascriptHost, ExecutionContext, HostHooks, JsEngine, JsTypes, JsTypesWithRealm,
    Numeric, PreferredType, PropertyDescriptor,
};

use super::types::{CachedPrimitive, ObjectProfile, V8ArrayBufferState};
use super::{
    V8ArrayBuffer, V8BigInt, V8Constructor, V8DataView, V8Function, V8Generator, V8Map, V8Object,
    V8Promise, V8PropertyKey, V8Realm, V8Set, V8SharedArrayBuffer, V8String, V8Symbol,
    V8TypedArray, V8Types, V8Value,
};

const HOST_OBJECT_TAG: u16 = 1;
static HOST_OBJECT_MARKER: u8 = 0;
static NEXT_ISOLATE_ID: AtomicU64 = AtomicU64::new(1);

type StoredBehaviour = Box<
    dyn Fn(&[V8Value], V8Value, &mut dyn ExecutionContext<V8Types>) -> Completion<V8Value, V8Types>,
>;
type RealmJob = Box<dyn FnOnce(&mut dyn ExecutionContext<V8Types>)>;
type CaptureBehaviour<T, C> = fn(
    &[<T as JsTypes>::JsValue],
    <T as JsTypes>::JsValue,
    &C,
    &mut dyn ExecutionContext<T>,
) -> Completion<<T as JsTypes>::JsValue, T>;

enum QueuedJob {
    Plain(Rc<V8RealmState>, Box<dyn FnOnce()>),
    WithRealm(Rc<V8RealmState>, RealmJob),
}

struct CallbackRecord {
    isolate_id: u64,
    creation_realm: RcWeak<V8RealmState>,
    behaviour: StoredBehaviour,
}

struct HostObjectRecord {
    data: Box<dyn Any>,
}

#[derive(Default)]
struct RealmHostData {
    values: HashMap<TypeId, Box<dyn Any>>,
    associated_objects: Vec<(V8Object, Box<dyn Any>)>,
}

#[derive(Clone)]
struct V8RealmState {
    realm: V8Realm,
    realm_global: RefCell<V8Object>,
    intrinsics: Option<RealmIntrinsics<V8Types>>,
    host_data_holder: Option<V8Object>,
}

type StoredCallbackScope = v8::PinScope<'static, 'static>;

thread_local! {
    static CURRENT_ENGINE: Cell<*mut V8Engine> = const { Cell::new(std::ptr::null_mut()) };
    static CURRENT_CALLBACK_SCOPE: Cell<*mut StoredCallbackScope> = const { Cell::new(std::ptr::null_mut()) };
    static CURRENT_CALLBACK_ISOLATE_ID: Cell<u64> = const { Cell::new(0) };
}

struct SharedIsolate {
    isolate_id: u64,
    realm_states: RefCell<Vec<RcWeak<V8RealmState>>>,
    queued_jobs: RefCell<VecDeque<QueuedJob>>,
    host_object_handles: RefCell<Vec<v8::Weak<v8::Object>>>,
    callback_handles: RefCell<Vec<v8::Weak<v8::Function>>>,
    isolate: RefCell<v8::OwnedIsolate>,
    microtask_queue: v8::UniqueRef<v8::MicrotaskQueue>,
}

impl SharedIsolate {
    fn new() -> Rc<Self> {
        initialize_v8();
        let mut isolate = v8::Isolate::new(v8::CreateParams::default());
        isolate.set_microtasks_policy(v8::MicrotasksPolicy::Explicit);
        let microtask_queue = v8::MicrotaskQueue::new(&mut isolate, v8::MicrotasksPolicy::Explicit);
        Rc::new(Self {
            isolate_id: NEXT_ISOLATE_ID.fetch_add(1, Ordering::Relaxed),
            realm_states: RefCell::new(Vec::new()),
            queued_jobs: RefCell::new(VecDeque::new()),
            host_object_handles: RefCell::new(Vec::new()),
            callback_handles: RefCell::new(Vec::new()),
            isolate: RefCell::new(isolate),
            microtask_queue,
        })
    }

    fn borrow(&self, expected_isolate_id: u64) -> RefMut<'_, v8::OwnedIsolate> {
        assert_eq!(
            self.isolate_id, expected_isolate_id,
            "V8 engine and shared isolate identities differ"
        );
        self.isolate.borrow_mut()
    }
}

macro_rules! v8_engine_scope_with_context {
    ($scope:ident, $engine:expr, $context:expr, $body:block) => {{
        let callback_scope_pointer = CURRENT_CALLBACK_SCOPE.get();
        if callback_scope_pointer.is_null() {
            let shared_isolate_for_scope = Rc::clone(&$engine.shared_isolate);
            let mut isolate_for_scope = shared_isolate_for_scope.borrow($engine.isolate_id);
            v8::scope_with_context!(let $scope, &mut *isolate_for_scope, $context);
            $body
        } else {
            assert_eq!(
                CURRENT_CALLBACK_ISOLATE_ID.get(),
                $engine.isolate_id,
                "reentrant V8 scope belongs to another isolate"
            );
            // SAFETY: `native_callback` installs this pointer from the pinned
            // scope V8 supplied for the synchronous callback. The guard clears
            // it before that scope ends, and the isolate identity is checked
            // above. Reusing the callback scope avoids creating a second
            // mutable reference to the isolate owned by the outer V8 call.
            let callback_scope = unsafe { &mut *callback_scope_pointer };
            let local_context = v8::Local::new(callback_scope, $context);
            let $scope = &mut v8::ContextScope::new(callback_scope, local_context);
            $body
        }
    }};
}

macro_rules! v8_engine_scope {
    ($scope:ident, $engine:expr, $body:block) => {{
        let callback_scope_pointer = CURRENT_CALLBACK_SCOPE.get();
        if callback_scope_pointer.is_null() {
            let shared_isolate_for_scope = Rc::clone(&$engine.shared_isolate);
            let mut isolate_for_scope = shared_isolate_for_scope.borrow($engine.isolate_id);
            v8::scope!(let $scope, &mut *isolate_for_scope);
            $body
        } else {
            assert_eq!(
                CURRENT_CALLBACK_ISOLATE_ID.get(),
                $engine.isolate_id,
                "reentrant V8 scope belongs to another isolate"
            );
            // SAFETY: The pointer and lifetime invariants are established by
            // `CurrentCallbackScopeGuard` and checked above.
            let $scope = unsafe { &mut *callback_scope_pointer };
            $body
        }
    }};
}

macro_rules! v8_shared_scope {
    ($scope:ident, $shared_isolate:expr, $isolate_id:expr, $body:block) => {{
        let callback_scope_pointer = CURRENT_CALLBACK_SCOPE.get();
        if callback_scope_pointer.is_null() {
            let mut isolate_for_scope = $shared_isolate.borrow($isolate_id);
            v8::scope!(let $scope, &mut *isolate_for_scope);
            $body
        } else {
            assert_eq!(
                CURRENT_CALLBACK_ISOLATE_ID.get(),
                $isolate_id,
                "reentrant V8 scope belongs to another isolate"
            );
            // SAFETY: The pointer and lifetime invariants are established by
            // `CurrentCallbackScopeGuard` and checked above.
            let $scope = unsafe { &mut *callback_scope_pointer };
            $body
        }
    }};
}

macro_rules! v8_shared_isolate {
    ($isolate:ident, $shared_isolate:expr, $isolate_id:expr, $body:block) => {{
        let callback_scope_pointer = CURRENT_CALLBACK_SCOPE.get();
        if callback_scope_pointer.is_null() {
            let mut isolate_for_operation = $shared_isolate.borrow($isolate_id);
            let $isolate = &mut *isolate_for_operation;
            $body
        } else {
            assert_eq!(
                CURRENT_CALLBACK_ISOLATE_ID.get(),
                $isolate_id,
                "reentrant V8 scope belongs to another isolate"
            );
            // SAFETY: The pointer and lifetime invariants are established by
            // `CurrentCallbackScopeGuard` and checked above. The isolate
            // reference is reborrowed from V8's active callback scope.
            let callback_scope = unsafe { &mut *callback_scope_pointer };
            let $isolate = &mut ***callback_scope;
            $body
        }
    }};
}

struct CurrentEngineGuard {
    previous: *mut V8Engine,
}

struct CurrentCallbackScopeGuard {
    previous_scope: *mut StoredCallbackScope,
    previous_isolate_id: u64,
}

impl CurrentCallbackScopeGuard {
    fn enter(scope: &mut v8::PinScope<'_, '_>, isolate_id: u64) -> Self {
        let scope_pointer = (scope as *mut v8::PinScope<'_, '_>).cast::<StoredCallbackScope>();
        let previous_scope = CURRENT_CALLBACK_SCOPE.replace(scope_pointer);
        let previous_isolate_id = CURRENT_CALLBACK_ISOLATE_ID.replace(isolate_id);
        Self {
            previous_scope,
            previous_isolate_id,
        }
    }
}

impl Drop for CurrentCallbackScopeGuard {
    fn drop(&mut self) {
        CURRENT_CALLBACK_SCOPE.set(self.previous_scope);
        CURRENT_CALLBACK_ISOLATE_ID.set(self.previous_isolate_id);
    }
}

impl CurrentEngineGuard {
    fn enter(engine: &mut V8Engine) -> Self {
        let engine_pointer = engine as *mut V8Engine;
        let previous = CURRENT_ENGINE.replace(engine_pointer);
        Self { previous }
    }
}

impl Drop for CurrentEngineGuard {
    fn drop(&mut self) {
        CURRENT_ENGINE.set(self.previous);
    }
}

pub struct V8Engine {
    isolate_id: u64,
    realm_state: Rc<V8RealmState>,
    host_hooks: HostHooks<V8Types>,
    shared_isolate: Rc<SharedIsolate>,
}

fn initialize_v8() {
    static INITIALIZE: Once = Once::new();
    INITIALIZE.call_once(|| {
        v8::V8::set_flags_from_string("--expose-gc");
        v8::V8::initialize_platform(v8::new_default_platform(0, false).make_shared());
        v8::V8::initialize();
    });
}

fn cache_string(scope: &v8::PinScope<'_, '_>, string: &v8::String) -> Arc<[u16]> {
    let mut utf16 = vec![0; string.length()];
    string.write_v2(scope, 0, &mut utf16, v8::WriteFlags::empty());
    Arc::from(utf16)
}

fn host_data_pointer<'scope>(
    scope: &v8::PinScope<'scope, '_>,
    object: v8::Local<'scope, v8::Object>,
) -> Option<NonNull<c_void>> {
    if object.internal_field_count() != 2 {
        return None;
    }
    let marker_data = object.get_internal_field(scope, 0)?;
    let marker = v8::Local::<v8::External>::try_from(marker_data).ok()?;
    if marker.value() != std::ptr::addr_of!(HOST_OBJECT_MARKER).cast_mut().cast() {
        return None;
    }

    // SAFETY: Field 1 is read only after field 0 proves that this object was
    // created by `create_object_with_any`. That constructor stores an aligned
    // `HostObjectRecord` pointer in field 1 with this exact tag. The weak
    // handle keeps the record alive for at least as long as the JS object.
    let pointer = unsafe { object.get_aligned_pointer_from_internal_field(1, HOST_OBJECT_TAG) }
        as *mut c_void;
    NonNull::new(pointer)
}

fn wrap_local_value(
    scope: &mut v8::PinScope<'_, '_>,
    isolate_id: u64,
    value: v8::Local<'_, v8::Value>,
) -> V8Value {
    let primitive = if value.is_undefined() {
        CachedPrimitive::Undefined
    } else if value.is_null() {
        CachedPrimitive::Null
    } else if value.is_boolean() {
        CachedPrimitive::Boolean(value.boolean_value(scope))
    } else if value.is_number() {
        CachedPrimitive::Number(value.number_value(scope).unwrap_or(f64::NAN))
    } else if value.is_string() {
        let string = v8::Local::<v8::String>::try_from(value).expect("V8 string type check failed");
        CachedPrimitive::String(cache_string(scope, &string))
    } else if value.is_big_int() {
        let canonical = value
            .to_string(scope)
            .map(|string| string.to_rust_string_lossy(scope))
            .unwrap_or_default();
        CachedPrimitive::BigInt(Arc::from(canonical))
    } else {
        CachedPrimitive::Other
    };

    let mut host_data = None;
    let object_profile = if value.is_object() {
        let object = v8::Local::<v8::Object>::try_from(value).expect("V8 object type check failed");
        host_data = host_data_pointer(scope, object);
        let array_buffer_handle = v8::Local::<v8::ArrayBuffer>::try_from(value)
            .ok()
            .map(|handle| v8::Global::new(scope, handle));
        let shared_array_buffer_handle = v8::Local::<v8::SharedArrayBuffer>::try_from(value)
            .ok()
            .map(|handle| v8::Global::new(scope, handle));
        let typed_array_handle = v8::Local::<v8::TypedArray>::try_from(value)
            .ok()
            .map(|handle| v8::Global::new(scope, handle));
        let data_view_handle = v8::Local::<v8::DataView>::try_from(value)
            .ok()
            .map(|handle| v8::Global::new(scope, handle));
        let promise_handle = v8::Local::<v8::Promise>::try_from(value)
            .ok()
            .map(|handle| v8::Global::new(scope, handle));
        let function_handle = v8::Local::<v8::Function>::try_from(value)
            .ok()
            .map(|handle| v8::Global::new(scope, handle));
        let map_handle = v8::Local::<v8::Map>::try_from(value)
            .ok()
            .map(|handle| v8::Global::new(scope, handle));
        let set_handle = v8::Local::<v8::Set>::try_from(value)
            .ok()
            .map(|handle| v8::Global::new(scope, handle));
        let array_buffer_state = if value.is_array_buffer() {
            let array_buffer = v8::Local::<v8::ArrayBuffer>::try_from(value)
                .expect("V8 ArrayBuffer type check failed");
            let backing_store = array_buffer.get_backing_store();
            Some(V8ArrayBufferState {
                resizable: backing_store.is_resizable_by_user_javascript(),
                backing_store,
                detached: Rc::new(Cell::new(array_buffer.was_detached())),
            })
        } else {
            None
        };
        let typed_array_element_type = if value.is_int8_array() {
            Some(TypedArrayElementType::Int8)
        } else if value.is_uint8_array() {
            Some(TypedArrayElementType::Uint8)
        } else if value.is_uint8_clamped_array() {
            Some(TypedArrayElementType::Uint8Clamped)
        } else if value.is_int16_array() {
            Some(TypedArrayElementType::Int16)
        } else if value.is_uint16_array() {
            Some(TypedArrayElementType::Uint16)
        } else if value.is_int32_array() {
            Some(TypedArrayElementType::Int32)
        } else if value.is_uint32_array() {
            Some(TypedArrayElementType::Uint32)
        } else if value.is_float16_array() {
            Some(TypedArrayElementType::Float16)
        } else if value.is_float32_array() {
            Some(TypedArrayElementType::Float32)
        } else if value.is_float64_array() {
            Some(TypedArrayElementType::Float64)
        } else if value.is_big_int64_array() {
            Some(TypedArrayElementType::BigInt64)
        } else if value.is_big_uint64_array() {
            Some(TypedArrayElementType::BigUint64)
        } else {
            None
        };
        Some(Box::new(ObjectProfile {
            object_handle: v8::Global::new(scope, object),
            array_buffer_handle,
            shared_array_buffer_handle,
            typed_array_handle,
            data_view_handle,
            promise_handle,
            function_handle,
            map_handle,
            set_handle,
            is_weak_map: value.is_weak_map(),
            is_weak_set: value.is_weak_set(),
            is_generator: value.is_generator_object(),
            is_boolean_wrapper: value.is_boolean_object(),
            is_number_wrapper: value.is_number_object(),
            is_string_wrapper: value.is_string_object(),
            is_bigint_wrapper: value.is_big_int_object(),
            is_date: value.is_date(),
            is_regexp: value.is_reg_exp(),
            is_error: value.is_native_error(),
            wrapper_primitive: None,
            array_buffer_state,
            typed_array_element_type,
        }))
    } else {
        None
    };

    V8Value {
        isolate_id,
        handle: v8::Global::new(scope, value),
        primitive,
        object_profile,
        host_data,
    }
}

fn object_from_wrapped_value(value: V8Value) -> V8Object {
    V8Object::from_value(value).expect("V8 object wrapper requires an object value")
}

fn local_value<'scope>(
    scope: &mut v8::PinScope<'scope, '_>,
    isolate_id: u64,
    value: &V8Value,
) -> Result<v8::Local<'scope, v8::Value>, V8Value> {
    if value.isolate_id != isolate_id {
        let message = v8::String::new(scope, "value belongs to a different V8 isolate")
            .expect("static V8 error string allocation failed");
        let exception = v8::Exception::type_error(scope, message);
        return Err(wrap_local_value(scope, isolate_id, exception));
    }
    Ok(v8::Local::new(scope, &value.handle))
}

fn local_object<'scope>(
    scope: &mut v8::PinScope<'scope, '_>,
    isolate_id: u64,
    object: &V8Object,
) -> Result<v8::Local<'scope, v8::Object>, V8Value> {
    if object.0.isolate_id != isolate_id {
        let message = v8::String::new(scope, "value is not an object")
            .expect("static V8 error string allocation failed");
        let exception = v8::Exception::type_error(scope, message);
        return Err(wrap_local_value(scope, isolate_id, exception));
    }
    Ok(v8::Local::new(scope, &object.1))
}

fn local_typed_object<'scope, T>(
    scope: &mut v8::PinScope<'scope, '_>,
    isolate_id: u64,
    object: &V8Object,
    handle: &v8::Global<T>,
) -> Result<v8::Local<'scope, T>, V8Value> {
    if object.0.isolate_id != isolate_id {
        let message = v8::String::new(scope, "value belongs to a different V8 isolate")
            .expect("static V8 error string allocation failed");
        let exception = v8::Exception::type_error(scope, message);
        return Err(wrap_local_value(scope, isolate_id, exception));
    }
    Ok(v8::Local::new(scope, handle))
}

fn local_property_key<'scope>(
    scope: &mut v8::PinScope<'scope, '_>,
    isolate_id: u64,
    key: &V8PropertyKey,
) -> Result<v8::Local<'scope, v8::Value>, V8Value> {
    match key {
        V8PropertyKey::String(string) => {
            if let Some(value) = &string.value {
                local_value(scope, isolate_id, value)
            } else {
                let string =
                    v8::String::new_from_two_byte(scope, &string.utf16, v8::NewStringType::Normal)
                        .expect("V8 property name allocation failed");
                Ok(string.into())
            }
        }
        V8PropertyKey::Symbol(symbol) => local_value(scope, isolate_id, &symbol.0),
        V8PropertyKey::Index(index) => Ok(v8::Integer::new_from_unsigned(scope, *index).into()),
    }
}

fn local_name<'scope>(
    scope: &mut v8::PinScope<'scope, '_>,
    isolate_id: u64,
    key: &V8PropertyKey,
) -> Result<v8::Local<'scope, v8::Name>, V8Value> {
    let value = match key {
        V8PropertyKey::Index(index) => {
            let string = v8::String::new(scope, &index.to_string())
                .expect("V8 array-index name allocation failed");
            return Ok(string.into());
        }
        _ => local_property_key(scope, isolate_id, key)?,
    };
    v8::Local::<v8::Name>::try_from(value).map_err(|_| {
        let message = v8::String::new(scope, "property key is not a name")
            .expect("static V8 error string allocation failed");
        let exception = v8::Exception::type_error(scope, message);
        wrap_local_value(scope, isolate_id, exception)
    })
}

fn caught_exception(
    scope: &mut v8::PinScope<'_, '_>,
    isolate_id: u64,
    exception: Option<v8::Local<'_, v8::Value>>,
    fallback: &str,
) -> V8Value {
    let exception = exception.unwrap_or_else(|| {
        let message =
            v8::String::new(scope, fallback).expect("static V8 exception string allocation failed");
        v8::Exception::error(scope, message)
    });
    wrap_local_value(scope, isolate_id, exception)
}

macro_rules! caught {
    ($scope:expr, $isolate_id:expr, $fallback:expr) => {{
        let exception = $scope.exception();
        caught_exception($scope, $isolate_id, exception, $fallback)
    }};
}

fn native_callback(
    scope: &mut v8::PinScope<'_, '_>,
    arguments: v8::FunctionCallbackArguments,
    mut return_value: v8::ReturnValue,
) {
    let data = arguments.data();
    let Ok(external) = v8::Local::<v8::External>::try_from(data) else {
        let message = v8::String::new(scope, "missing native callback record")
            .expect("static V8 callback error allocation failed");
        let exception = v8::Exception::error(scope, message);
        scope.throw_exception(exception);
        return;
    };
    let record_pointer = external.value().cast::<CallbackRecord>();
    if record_pointer.is_null() {
        let message = v8::String::new(scope, "invalid native callback record")
            .expect("static V8 callback error allocation failed");
        let exception = v8::Exception::error(scope, message);
        scope.throw_exception(exception);
        return;
    }

    let engine_pointer = CURRENT_ENGINE.get();
    if engine_pointer.is_null() {
        let message = v8::String::new(scope, "native callback entered without an active engine")
            .expect("static V8 callback error allocation failed");
        let exception = v8::Exception::error(scope, message);
        scope.throw_exception(exception);
        return;
    }

    // SAFETY: Callback records are allocated by `make_builtin_function` and
    // released only by the function's guaranteed weak finalizer. V8 invokes
    // this callback while the function is strongly reachable. CURRENT_ENGINE
    // is installed around every operation that can execute JavaScript and is
    // restricted to the isolate thread. The isolate id check below prevents a
    // record from being used by another isolate. `catch_unwind` prevents Rust
    // unwinding from crossing V8's callback boundary.
    let (result, callback_isolate_id) = unsafe {
        let record = &*record_pointer;
        let engine = &mut *engine_pointer;
        let result = if record.isolate_id != engine.isolate_id {
            Err(engine.new_type_error("native callback belongs to a different V8 isolate"))
        } else if let Some(creation_realm) = record.creation_realm.upgrade() {
            let previous_realm = replace(&mut engine.realm_state, creation_realm);
            let completion = {
                let _current_callback_scope =
                    CurrentCallbackScopeGuard::enter(scope, record.isolate_id);
                let callback_arguments: Vec<_> = (0..arguments.length())
                    .map(|index| wrap_local_value(scope, record.isolate_id, arguments.get(index)))
                    .collect();
                let this_value =
                    wrap_local_value(scope, record.isolate_id, arguments.this().into());
                match catch_unwind(AssertUnwindSafe(|| {
                    (record.behaviour)(&callback_arguments, this_value, engine)
                })) {
                    Ok(completion) => completion,
                    Err(_) => Err(engine.new_type_error("Rust panic in native callback")),
                }
            };
            engine.realm_state = previous_realm;
            completion
        } else {
            Err(engine.new_type_error("native callback creation realm no longer exists"))
        };
        (result, record.isolate_id)
    };

    match result {
        Ok(value) => match local_value(scope, callback_isolate_id, &value) {
            Ok(value) => return_value.set(value),
            Err(exception) => {
                let exception = v8::Local::new(scope, &exception.handle);
                scope.throw_exception(exception);
            }
        },
        Err(exception) => {
            if exception.isolate_id == callback_isolate_id {
                let exception = v8::Local::new(scope, &exception.handle);
                scope.throw_exception(exception);
            } else {
                let message = v8::String::new(scope, "callback returned a cross-isolate exception")
                    .expect("static V8 callback error allocation failed");
                let exception = v8::Exception::type_error(scope, message);
                scope.throw_exception(exception);
            }
        }
    }
}

fn reject_module_import<'scope>(
    context: v8::Local<'scope, v8::Context>,
    _specifier: v8::Local<'scope, v8::String>,
    _import_attributes: v8::Local<'scope, v8::FixedArray>,
    _referrer: v8::Local<'scope, v8::Module>,
) -> Option<v8::Local<'scope, v8::Module>> {
    // SAFETY: V8 invokes module-resolution callbacks with the entered context
    // and no Rust handle scope. rusty_v8 requires CallbackScope only at this
    // callback boundary; the scope is pinned for its full use below.
    v8::callback_scope!(unsafe scope, context);
    let message = v8::String::new(scope, "module imports are not enabled for the V8 backend")
        .expect("static module error allocation failed");
    let exception = v8::Exception::type_error(scope, message);
    scope.throw_exception(exception);
    None
}

impl V8Engine {
    pub fn new() -> Self {
        Self::new_with_shared_isolate(SharedIsolate::new())
    }

    fn new_with_shared_isolate(shared_isolate: Rc<SharedIsolate>) -> Self {
        let isolate_id = shared_isolate.isolate_id;

        let (context_handle, realm_global) = v8_shared_scope!(scope, shared_isolate, isolate_id, {
            let microtask_queue = from_ref(&*shared_isolate.microtask_queue).cast_mut();
            let context = v8::Context::new(
                scope,
                v8::ContextOptions {
                    microtask_queue: Some(microtask_queue),
                    ..v8::ContextOptions::default()
                },
            );
            let context_handle = v8::Global::new(scope, context);
            let context_scope = &mut v8::ContextScope::new(scope, context);
            let global = context.global(context_scope);
            let realm_global = object_from_wrapped_value(wrap_local_value(
                context_scope,
                isolate_id,
                global.into(),
            ));
            (context_handle, realm_global)
        });

        let realm = V8Realm {
            isolate_id,
            context: context_handle,
        };
        let realm_state = Rc::new(V8RealmState {
            realm,
            realm_global: RefCell::new(realm_global),
            intrinsics: None,
            host_data_holder: None,
        });
        let mut engine = Self {
            isolate_id,
            realm_state,
            host_hooks: HostHooks::empty(),
            shared_isolate,
        };
        let intrinsics = engine.load_intrinsics();
        Rc::get_mut(&mut engine.realm_state)
            .expect("new V8 realm state must not be shared while initializing intrinsics")
            .intrinsics = Some(intrinsics.clone());
        let host_data_holder = engine.create_object_with_any(
            intrinsics.object_prototype,
            Box::new(RealmHostData::default()),
        );
        Rc::get_mut(&mut engine.realm_state)
            .expect("new V8 realm state must not be shared while initializing host data")
            .host_data_holder = Some(host_data_holder);
        engine
            .shared_isolate
            .realm_states
            .borrow_mut()
            .push(Rc::downgrade(&engine.realm_state));
        engine
    }

    pub fn associate_existing_object(&mut self, object: &V8Object, data: Box<dyn Any>) {
        self.realm_host_data_mut()
            .associated_objects
            .push((object.clone(), data));
    }

    pub fn new_child_realm(&self) -> Self {
        Self::new_with_shared_isolate(Rc::clone(&self.shared_isolate))
    }

    fn realm_host_data(&self) -> &RealmHostData {
        let holder = self
            .realm_state
            .host_data_holder
            .as_ref()
            .expect("V8 realm host data is not initialized");
        self.with_object_any(holder)
            .and_then(|data| data.downcast_ref::<RealmHostData>())
            .expect("V8 realm host-data holder contains an unexpected value")
    }

    fn realm_host_data_mut(&mut self) -> &mut RealmHostData {
        let holder = self
            .realm_state
            .host_data_holder
            .as_ref()
            .expect("V8 realm host data is not initialized")
            .clone();
        self.with_object_any_mut(&holder)
            .and_then(|data| data.downcast_mut::<RealmHostData>())
            .expect("V8 realm host-data holder contains an unexpected value")
    }

    fn state_for_realm(&self, realm: &V8Realm) -> Option<Rc<V8RealmState>> {
        if realm.isolate_id != self.isolate_id {
            return None;
        }
        let mut realm_states = self.shared_isolate.realm_states.borrow_mut();
        realm_states.retain(|state| state.strong_count() != 0);
        realm_states
            .iter()
            .filter_map(RcWeak::upgrade)
            .find(|state| state.realm.context == realm.context)
    }

    pub(crate) fn create_weak_object(&mut self, object: &V8Object) -> Rc<v8::Weak<v8::Object>> {
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let local = local_object(scope, isolate_id, object)
                .expect("reflector creation received a non-object or cross-isolate handle");
            Rc::new(v8::Weak::new(scope, local))
        })
    }

    pub(crate) fn upgrade_weak_object(&mut self, weak: &v8::Weak<v8::Object>) -> Option<V8Object> {
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let global = weak.to_global(scope)?;
            let local = v8::Local::new(scope, global);
            Some(object_from_wrapped_value(wrap_local_value(
                scope,
                isolate_id,
                local.into(),
            )))
        })
    }

    fn call_js_helper(
        &mut self,
        source: &str,
        arguments: &[V8Value],
    ) -> Completion<V8Value, V8Types> {
        for argument in arguments {
            if argument.isolate_id != self.isolate_id {
                return Err(self.new_type_error("value belongs to a different V8 isolate"));
            }
        }
        let _current_engine = CurrentEngineGuard::enter(self);
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            v8::tc_scope!(let try_catch, scope);
            let Some(source) = v8::String::new(try_catch, source) else {
                return Err(caught_exception(
                    try_catch,
                    isolate_id,
                    None,
                    "failed to allocate helper source",
                ));
            };
            let Some(script) = v8::Script::compile(try_catch, source, None) else {
                return Err(caught!(try_catch, isolate_id, "failed to compile helper"));
            };
            let Some(function_value) = script.run(try_catch) else {
                return Err(caught!(try_catch, isolate_id, "failed to evaluate helper"));
            };
            let Ok(function) = v8::Local::<v8::Function>::try_from(function_value) else {
                return Err(caught_exception(
                    try_catch,
                    isolate_id,
                    None,
                    "helper is not callable",
                ));
            };
            let local_arguments: Result<Vec<_>, _> = arguments
                .iter()
                .map(|argument| local_value(try_catch, isolate_id, argument))
                .collect();
            let local_arguments = local_arguments?;
            let receiver = v8::undefined(try_catch).into();
            let Some(result) = function.call(try_catch, receiver, &local_arguments) else {
                return Err(caught!(try_catch, isolate_id, "helper call failed"));
            };
            Ok(wrap_local_value(try_catch, isolate_id, result))
        })
    }

    fn intrinsic_object(&mut self, source: &str) -> V8Object {
        match <Self as ExecutionContext<V8Types>>::evaluate_script(self, source) {
            Ok(value) => V8Types::value_as_object(&value)
                .unwrap_or_else(|| panic!("V8 intrinsic `{source}` is not an object")),
            Err(_) => panic!("failed to load V8 intrinsic `{source}`"),
        }
    }

    fn intrinsic_constructor(&mut self, source: &str) -> V8Constructor {
        let object = self.intrinsic_object(source);
        V8Types::object_as_constructor(&object)
            .unwrap_or_else(|| panic!("V8 intrinsic `{source}` is not a constructor"))
    }

    fn load_intrinsics(&mut self) -> RealmIntrinsics<V8Types> {
        RealmIntrinsics {
            array_buffer: self.intrinsic_constructor("ArrayBuffer"),
            shared_array_buffer: self.intrinsic_constructor("SharedArrayBuffer"),
            promise: self.intrinsic_constructor("Promise"),
            object: self.intrinsic_constructor("Object"),
            function: self.intrinsic_constructor("Function"),
            error: self.intrinsic_constructor("Error"),
            type_error: self.intrinsic_constructor("TypeError"),
            range_error: self.intrinsic_constructor("RangeError"),
            syntax_error: self.intrinsic_constructor("SyntaxError"),
            reference_error: self.intrinsic_constructor("ReferenceError"),
            uri_error: self.intrinsic_constructor("URIError"),
            eval_error: self.intrinsic_constructor("EvalError"),
            array: self.intrinsic_constructor("Array"),
            uint8_array: self.intrinsic_constructor("Uint8Array"),
            boolean: self.intrinsic_constructor("Boolean"),
            number: self.intrinsic_constructor("Number"),
            string: self.intrinsic_constructor("String"),
            bigint: self.intrinsic_constructor("BigInt"),
            date: self.intrinsic_constructor("Date"),
            regexp: self.intrinsic_constructor("RegExp"),
            map: self.intrinsic_constructor("Map"),
            set: self.intrinsic_constructor("Set"),
            boolean_prototype: self.intrinsic_object("Boolean.prototype"),
            number_prototype: self.intrinsic_object("Number.prototype"),
            string_prototype: self.intrinsic_object("String.prototype"),
            bigint_prototype: self.intrinsic_object("BigInt.prototype"),
            date_prototype: self.intrinsic_object("Date.prototype"),
            regexp_prototype: self.intrinsic_object("RegExp.prototype"),
            map_prototype: self.intrinsic_object("Map.prototype"),
            set_prototype: self.intrinsic_object("Set.prototype"),
            error_prototype: self.intrinsic_object("Error.prototype"),
            type_error_prototype: self.intrinsic_object("TypeError.prototype"),
            range_error_prototype: self.intrinsic_object("RangeError.prototype"),
            syntax_error_prototype: self.intrinsic_object("SyntaxError.prototype"),
            reference_error_prototype: self.intrinsic_object("ReferenceError.prototype"),
            uri_error_prototype: self.intrinsic_object("URIError.prototype"),
            eval_error_prototype: self.intrinsic_object("EvalError.prototype"),
            object_prototype: self.intrinsic_object("Object.prototype"),
            function_prototype: self.intrinsic_object("Function.prototype"),
            async_iterator_prototype: self.intrinsic_object(
                "Object.getPrototypeOf(Object.getPrototypeOf(async function*(){}()))",
            ),
        }
    }

    fn make_builtin_function(
        &mut self,
        behaviour: StoredBehaviour,
        length: u32,
        name: V8PropertyKey,
        is_constructor: bool,
    ) -> V8Function {
        let record = Box::new(CallbackRecord {
            isolate_id: self.isolate_id,
            creation_realm: Rc::downgrade(&self.realm_state),
            behaviour,
        });
        let record_pointer = Box::into_raw(record);
        let isolate_id = self.isolate_id;
        let function_name = self.property_key_to_rust_string(&name);

        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let external = v8::External::new(scope, record_pointer.cast());
            let constructor_behavior = if is_constructor {
                v8::ConstructorBehavior::Allow
            } else {
                v8::ConstructorBehavior::Throw
            };
            let function = v8::Function::builder(native_callback)
                .data(external.into())
                .length(length as i32)
                .constructor_behavior(constructor_behavior)
                .build(scope)
                .expect("V8 failed to create native function");
            if let Some(name) = v8::String::new(scope, &function_name) {
                function.set_name(name);
            }
            let function_value = wrap_local_value(scope, isolate_id, function.into());

            // The weak handle is retained by the engine. The finalizer owns the
            // callback record and is guaranteed to release it before isolate
            // destruction, even if no collection happens first.
            let callback_handle = v8::Weak::with_guaranteed_finalizer(
                scope,
                function,
                Box::new(move || {
                    // SAFETY: `record_pointer` came from one Box::into_raw above
                    // and this guaranteed finalizer is its sole owner. The weak
                    // handle invokes this closure at most once.
                    unsafe {
                        drop(Box::from_raw(record_pointer));
                    }
                }),
            );
            self.shared_isolate
                .callback_handles
                .borrow_mut()
                .push(callback_handle);
            let object = object_from_wrapped_value(function_value);
            V8Types::object_as_function(&object)
                .expect("V8 native function wrapper is not a function")
        })
    }
}

impl Default for V8Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl JsEngine<V8Types> for V8Engine {
    fn create_realm(&mut self) -> V8Realm {
        let isolate_id = self.isolate_id;
        v8_engine_scope!(scope, self, {
            let microtask_queue = from_ref(&*self.shared_isolate.microtask_queue).cast_mut();
            let context = v8::Context::new(
                scope,
                v8::ContextOptions {
                    microtask_queue: Some(microtask_queue),
                    ..v8::ContextOptions::default()
                },
            );
            V8Realm {
                isolate_id,
                context: v8::Global::new(scope, context),
            }
        })
    }

    fn set_realm_global_object(
        &mut self,
        realm: &V8Realm,
        global: V8Object,
        _this_value: Option<V8Object>,
    ) {
        if realm.isolate_id != self.isolate_id || global.0.isolate_id != self.isolate_id {
            return;
        }
        if realm.context == self.realm_state.realm.context {
            self.realm_state.realm_global.replace(global);
        }
    }

    fn set_default_global_bindings(&mut self, realm: &V8Realm) -> Completion<(), V8Types> {
        if realm.isolate_id != self.isolate_id {
            Err(self.new_type_error("realm belongs to a different V8 isolate"))
        } else {
            Ok(())
        }
    }

    fn evaluate_script(&mut self, source: &str, realm: &V8Realm) -> Completion<V8Value, V8Types> {
        if realm.isolate_id != self.isolate_id {
            return Err(self.new_type_error("realm belongs to a different V8 isolate"));
        }
        let _current_engine = CurrentEngineGuard::enter(self);
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &realm.context, {
            v8::tc_scope!(let try_catch, scope);
            let Some(source) = v8::String::new(try_catch, source) else {
                return Err(caught_exception(
                    try_catch,
                    isolate_id,
                    None,
                    "script source allocation failed",
                ));
            };
            let Some(script) = v8::Script::compile(try_catch, source, None) else {
                return Err(caught!(try_catch, isolate_id, "script compilation failed"));
            };
            let Some(value) = script.run(try_catch) else {
                return Err(caught!(try_catch, isolate_id, "script evaluation failed"));
            };
            Ok(wrap_local_value(try_catch, isolate_id, value))
        })
    }

    fn evaluate_module(&mut self, source: &str, realm: &V8Realm) -> Completion<V8Object, V8Types> {
        if realm.isolate_id != self.isolate_id {
            return Err(self.new_type_error("realm belongs to a different V8 isolate"));
        }
        let _current_engine = CurrentEngineGuard::enter(self);
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &realm.context, {
            v8::tc_scope!(let try_catch, scope);
            let Some(source_string) = v8::String::new(try_catch, source) else {
                return Err(caught_exception(
                    try_catch,
                    isolate_id,
                    None,
                    "module source allocation failed",
                ));
            };
            let mut source = v8::script_compiler::Source::new(source_string, None);
            let Some(module) = v8::script_compiler::compile_module(try_catch, &mut source) else {
                return Err(caught!(try_catch, isolate_id, "module compilation failed"));
            };
            if module.instantiate_module(try_catch, reject_module_import) != Some(true) {
                return Err(caught!(
                    try_catch,
                    isolate_id,
                    "module instantiation failed"
                ));
            }
            if module.evaluate(try_catch).is_none() {
                return Err(caught!(try_catch, isolate_id, "module evaluation failed"));
            }
            let namespace = module.get_module_namespace();
            let namespace = wrap_local_value(try_catch, isolate_id, namespace);
            V8Types::value_as_object(&namespace).ok_or_else(|| {
                caught_exception(
                    try_catch,
                    isolate_id,
                    None,
                    "module namespace is not an object",
                )
            })
        })
    }

    fn allocate_array_buffer(
        &mut self,
        constructor: V8Constructor,
        byte_length: u64,
        max_byte_length: Option<u64>,
    ) -> Completion<V8ArrayBuffer, V8Types> {
        ExecutionContext::allocate_array_buffer(self, constructor, byte_length, max_byte_length)
    }

    fn detach_array_buffer(
        &mut self,
        array_buffer: V8ArrayBuffer,
        key: Option<V8Value>,
    ) -> Completion<(), V8Types> {
        ExecutionContext::detach_array_buffer(self, array_buffer, key)
    }

    fn clone_array_buffer(
        &mut self,
        source: V8ArrayBuffer,
        source_byte_offset: u64,
        source_length: u64,
        constructor: V8Constructor,
    ) -> Completion<V8ArrayBuffer, V8Types> {
        ExecutionContext::clone_array_buffer(
            self,
            source,
            source_byte_offset,
            source_length,
            constructor,
        )
    }

    fn allocate_shared_array_buffer(
        &mut self,
        _constructor: V8Constructor,
        byte_length: u64,
    ) -> Completion<V8SharedArrayBuffer, V8Types> {
        let byte_length = self.value_from_number(byte_length as f64);
        let value =
            self.call_js_helper("length => new SharedArrayBuffer(length)", &[byte_length])?;
        let object = V8Types::value_as_object(&value)
            .ok_or_else(|| self.new_type_error("SharedArrayBuffer allocation failed"))?;
        V8Types::object_as_shared_array_buffer(&object)
            .ok_or_else(|| self.new_type_error("SharedArrayBuffer allocation failed"))
    }

    fn set_host_hooks(&mut self, hooks: HostHooks<V8Types>) {
        self.host_hooks = hooks;
    }
}

impl JsTypesGcExt for V8Types {
    type Reflector = Rc<v8::Weak<v8::Object>>;
    type Context = V8Engine;

    fn create_reflector(context: &mut Self::Context, object: &V8Object) -> Self::Reflector {
        context.create_weak_object(object)
    }

    fn upgrade_reflector(
        context: &mut Self::Context,
        reflector: &Self::Reflector,
    ) -> Option<V8Object> {
        context.upgrade_weak_object(reflector)
    }
}

// SAFETY: Each V8 wrapper owns a `v8::Global` handle, so its JavaScript value
// remains rooted independently of Rust-side tracing. The marker implementation
// therefore cannot hide an unrooted V8 reference from the collector.
unsafe impl Trace for V8Value {}
unsafe impl Trace for V8Object {}
unsafe impl Trace for V8String {}
unsafe impl Trace for V8Symbol {}
unsafe impl Trace for V8BigInt {}

pub fn create_builtin_fn_with_captures<T, C>(
    execution_context: &mut dyn ExecutionContext<T>,
    captures: C,
    behaviour: CaptureBehaviour<T, C>,
    length: u32,
    name: T::PropertyKey,
    is_constructor: bool,
) -> T::Function
where
    T: JsTypes + JsTypesWithRealm,
    C: Trace + 'static,
{
    let engine = execution_context
        .as_any_mut()
        .downcast_mut::<V8Engine>()
        .expect("create_builtin_fn_with_captures called with a non-V8 engine");

    // SAFETY: This function is exported only by the V8-selected content
    // build, where T is V8Types. Function pointers have identical pointer
    // representation; the callback trampoline validates the active isolate
    // before invoking the converted behaviour.
    let behaviour: CaptureBehaviour<V8Types, C> = unsafe { std::mem::transmute_copy(&behaviour) };

    // SAFETY: In a V8-selected build T::PropertyKey is V8PropertyKey. Moving
    // through MaybeUninit preserves ownership without creating a second drop.
    let name: V8PropertyKey = unsafe {
        let mut destination = std::mem::MaybeUninit::<V8PropertyKey>::uninit();
        std::ptr::copy_nonoverlapping(
            std::ptr::addr_of!(name).cast::<u8>(),
            destination.as_mut_ptr().cast::<u8>(),
            std::mem::size_of::<V8PropertyKey>(),
        );
        std::mem::forget(name);
        destination.assume_init()
    };
    let stored = Box::new(
        move |arguments: &[V8Value],
              this_value,
              execution_context: &mut dyn ExecutionContext<V8Types>| {
            behaviour(arguments, this_value, &captures, execution_context)
        },
    );
    let result = engine.make_builtin_function(stored, length, name, is_constructor);

    // SAFETY: In a V8-selected build T::Function is V8Function. The result is
    // moved into its associated-type spelling without duplicating ownership.
    unsafe {
        let mut destination = std::mem::MaybeUninit::<T::Function>::uninit();
        std::ptr::copy_nonoverlapping(
            std::ptr::addr_of!(result).cast::<u8>(),
            destination.as_mut_ptr().cast::<u8>(),
            std::mem::size_of::<V8Function>(),
        );
        std::mem::forget(result);
        destination.assume_init()
    }
}

impl EcmascriptHost<V8Types> for V8Engine {
    fn get(&mut self, object: &V8Object, property: &str) -> Completion<V8Value, V8Types> {
        let key = self.property_key_from_str(property);
        ExecutionContext::get(self, object.clone(), key)
    }

    fn is_callable(&self, value: &V8Value) -> bool {
        value
            .object_profile
            .as_ref()
            .is_some_and(|profile| profile.function_handle.is_some())
    }

    fn call(
        &mut self,
        callable: &V8Object,
        this_argument: &V8Value,
        arguments: &[V8Value],
    ) -> Completion<V8Value, V8Types> {
        if callable.0.isolate_id != self.isolate_id
            || this_argument.isolate_id != self.isolate_id
            || arguments
                .iter()
                .any(|value| value.isolate_id != self.isolate_id)
        {
            return Err(self.new_type_error("value belongs to a different V8 isolate"));
        }
        let _current_engine = CurrentEngineGuard::enter(self);
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            v8::tc_scope!(let try_catch, scope);
            let callable = local_value(try_catch, isolate_id, &callable.0)?;
            let function = v8::Local::<v8::Function>::try_from(callable).map_err(|_| {
                caught_exception(try_catch, isolate_id, None, "callback is not callable")
            })?;
            let this_argument = local_value(try_catch, isolate_id, this_argument)?;
            let local_arguments: Result<Vec<_>, _> = arguments
                .iter()
                .map(|argument| local_value(try_catch, isolate_id, argument))
                .collect();
            let Some(result) = function.call(try_catch, this_argument, &local_arguments?) else {
                return Err(caught!(try_catch, isolate_id, "callback call failed"));
            };
            Ok(wrap_local_value(try_catch, isolate_id, result))
        })
    }

    fn perform_a_microtask_checkpoint(&mut self) -> Completion<(), V8Types> {
        let _current_engine = CurrentEngineGuard::enter(self);
        loop {
            loop {
                let queued_job = self.shared_isolate.queued_jobs.borrow_mut().pop_front();
                let Some(queued_job) = queued_job else {
                    break;
                };
                let (realm_state, job_result) = match queued_job {
                    QueuedJob::Plain(realm_state, job) => {
                        let previous_realm =
                            replace(&mut self.realm_state, Rc::clone(&realm_state));
                        let result = catch_unwind(AssertUnwindSafe(job));
                        self.realm_state = previous_realm;
                        (realm_state, result)
                    }
                    QueuedJob::WithRealm(realm_state, job) => {
                        let previous_realm =
                            replace(&mut self.realm_state, Rc::clone(&realm_state));
                        let result = catch_unwind(AssertUnwindSafe(|| job(self)));
                        self.realm_state = previous_realm;
                        (realm_state, result)
                    }
                };
                if job_result.is_err() {
                    let previous_realm = replace(&mut self.realm_state, realm_state);
                    let exception = self.new_type_error("Rust panic in queued V8 job");
                    self.realm_state = previous_realm;
                    return Err(exception);
                }
            }
            let shared_isolate = Rc::clone(&self.shared_isolate);
            v8_shared_isolate!(isolate, shared_isolate, self.isolate_id, {
                shared_isolate.microtask_queue.perform_checkpoint(isolate);
            });
            if self.shared_isolate.queued_jobs.borrow().is_empty() {
                break;
            }
        }
        Ok(())
    }

    fn report_exception(&mut self, exception: V8Value) {
        let message = self
            .to_rust_string(exception)
            .unwrap_or_else(|_| "unknown V8 exception".to_owned());
        error!("unhandled V8 exception: {message}");
    }

    fn gc(&mut self) {
        let shared_isolate = Rc::clone(&self.shared_isolate);
        v8_shared_isolate!(isolate, shared_isolate, self.isolate_id, {
            isolate.request_garbage_collection_for_testing(v8::GarbageCollectionType::Full);
        });
    }

    fn value_undefined(&mut self) -> V8Value {
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let value = v8::undefined(scope).into();
            wrap_local_value(scope, isolate_id, value)
        })
    }

    fn value_null(&mut self) -> V8Value {
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let value = v8::null(scope).into();
            wrap_local_value(scope, isolate_id, value)
        })
    }

    fn value_from_bool(&mut self, boolean: bool) -> V8Value {
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let value = v8::Boolean::new(scope, boolean).into();
            wrap_local_value(scope, isolate_id, value)
        })
    }

    fn value_from_number(&mut self, number: f64) -> V8Value {
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let value = v8::Number::new(scope, number).into();
            wrap_local_value(scope, isolate_id, value)
        })
    }

    fn value_from_string(&mut self, string: V8String) -> V8Value {
        if let Some(value) = string.value {
            return value;
        }
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let local =
                v8::String::new_from_two_byte(scope, &string.utf16, v8::NewStringType::Normal)
                    .expect("V8 string allocation failed");
            wrap_local_value(scope, isolate_id, local.into())
        })
    }

    fn js_string_from_str(&self, string: &str) -> V8String {
        V8String {
            value: None,
            utf16: Arc::from(string.encode_utf16().collect::<Vec<_>>()),
        }
    }
}

impl ExecutionContext<V8Types> for V8Engine {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn to_primitive(
        &mut self,
        input: V8Value,
        preferred_type: Option<PreferredType>,
    ) -> Completion<V8Value, V8Types> {
        if input.object_profile.is_none() {
            return Ok(input);
        }
        let hint = match preferred_type {
            Some(PreferredType::String) => "string",
            Some(PreferredType::Number) => "number",
            None => "default",
        };
        let hint = self.value_from_string(self.js_string_from_str(hint));
        self.call_js_helper(
            "(value, hint) => { const exotic = value[Symbol.toPrimitive]; if (exotic !== undefined) { const result = exotic.call(value, hint); if (Object(result) === result) throw new TypeError('cannot convert object to primitive'); return result; } const methods = hint === 'string' ? ['toString', 'valueOf'] : ['valueOf', 'toString']; for (const name of methods) { const method = value[name]; if (typeof method === 'function') { const result = method.call(value); if (Object(result) !== result) return result; } } throw new TypeError('cannot convert object to primitive'); }",
            &[input, hint],
        )
    }

    fn to_boolean(&self, value: &V8Value) -> bool {
        match &value.primitive {
            CachedPrimitive::Undefined | CachedPrimitive::Null => false,
            CachedPrimitive::Boolean(boolean) => *boolean,
            CachedPrimitive::Number(number) => *number != 0.0 && !number.is_nan(),
            CachedPrimitive::String(string) => !string.is_empty(),
            CachedPrimitive::BigInt(canonical) => canonical.as_ref() != "0",
            CachedPrimitive::Other => true,
        }
    }

    fn to_number(&mut self, value: V8Value) -> Completion<f64, V8Types> {
        let result = self.call_js_helper("value => Number(value)", &[value])?;
        V8Types::value_as_number(&result)
            .ok_or_else(|| self.new_type_error("ToNumber did not produce a number"))
    }

    fn to_numeric(&mut self, value: V8Value) -> Completion<Numeric<V8Types>, V8Types> {
        if let Some(bigint) = V8Types::value_as_bigint(&value) {
            return Ok(Numeric::BigInt(bigint));
        }
        self.to_number(value).map(Numeric::Number)
    }

    fn to_int32(&mut self, value: V8Value) -> Completion<i32, V8Types> {
        let result = self.call_js_helper("value => value | 0", &[value])?;
        Ok(V8Types::value_as_number(&result).unwrap_or(0.0) as i32)
    }

    fn to_uint32(&mut self, value: V8Value) -> Completion<u32, V8Types> {
        let result = self.call_js_helper("value => value >>> 0", &[value])?;
        Ok(V8Types::value_as_number(&result).unwrap_or(0.0) as u32)
    }

    fn to_int16(&mut self, value: V8Value) -> Completion<i16, V8Types> {
        let result = self.call_js_helper("value => new Int16Array([value])[0]", &[value])?;
        Ok(V8Types::value_as_number(&result).unwrap_or(0.0) as i16)
    }

    fn to_uint16(&mut self, value: V8Value) -> Completion<u16, V8Types> {
        let result = self.call_js_helper("value => new Uint16Array([value])[0]", &[value])?;
        Ok(V8Types::value_as_number(&result).unwrap_or(0.0) as u16)
    }

    fn to_int8(&mut self, value: V8Value) -> Completion<i8, V8Types> {
        let result = self.call_js_helper("value => new Int8Array([value])[0]", &[value])?;
        Ok(V8Types::value_as_number(&result).unwrap_or(0.0) as i8)
    }

    fn to_uint8(&mut self, value: V8Value) -> Completion<u8, V8Types> {
        let result = self.call_js_helper("value => new Uint8Array([value])[0]", &[value])?;
        Ok(V8Types::value_as_number(&result).unwrap_or(0.0) as u8)
    }

    fn to_uint8_clamp(&mut self, value: V8Value) -> Completion<u8, V8Types> {
        let result = self.call_js_helper("value => new Uint8ClampedArray([value])[0]", &[value])?;
        Ok(V8Types::value_as_number(&result).unwrap_or(0.0) as u8)
    }

    fn to_bigint(&mut self, value: V8Value) -> Completion<V8BigInt, V8Types> {
        let result = self.call_js_helper("value => BigInt(value)", &[value])?;
        V8Types::value_as_bigint(&result)
            .ok_or_else(|| self.new_type_error("ToBigInt did not produce a BigInt"))
    }

    fn string_to_bigint(&mut self, string: V8String) -> Option<V8BigInt> {
        let value = self.value_from_string(string);
        self.call_js_helper("value => BigInt(value)", &[value])
            .ok()
            .and_then(|value| V8Types::value_as_bigint(&value))
    }

    fn to_js_string(&mut self, value: V8Value) -> Completion<V8String, V8Types> {
        let result = self.call_js_helper(
            "value => { if (typeof value === 'symbol') throw new TypeError('cannot convert Symbol to string'); return `${value}`; }",
            &[value],
        )?;
        V8Types::value_as_string(&result)
            .ok_or_else(|| self.new_type_error("ToString did not produce a string"))
    }

    fn to_object(&mut self, value: V8Value) -> Completion<V8Object, V8Types> {
        let result = self.call_js_helper(
            "value => { if (value == null) throw new TypeError('cannot convert null or undefined to object'); return Object(value); }",
            &[value],
        )?;
        V8Types::value_as_object(&result)
            .ok_or_else(|| self.new_type_error("ToObject did not produce an object"))
    }

    fn to_property_key(&mut self, value: V8Value) -> Completion<V8PropertyKey, V8Types> {
        let primitive = self.to_primitive(value, Some(PreferredType::String))?;
        if let Some(symbol) = V8Types::value_as_symbol(&primitive) {
            Ok(V8PropertyKey::Symbol(symbol))
        } else {
            self.to_js_string(primitive).map(V8PropertyKey::String)
        }
    }

    fn to_length(&mut self, value: V8Value) -> Completion<u64, V8Types> {
        let result = self.call_js_helper(
            "value => Math.min(Math.max(Math.trunc(Number(value)) || 0, 0), Number.MAX_SAFE_INTEGER)",
            &[value],
        )?;
        Ok(V8Types::value_as_number(&result).unwrap_or(0.0) as u64)
    }

    fn canonical_numeric_index_string(&self, argument: &V8String) -> Option<f64> {
        let string = String::from_utf16_lossy(&argument.utf16);
        if string == "-0" {
            return Some(-0.0);
        }
        let number = string.parse::<f64>().ok()?;
        let canonical = if number.is_nan() {
            "NaN".to_owned()
        } else if number == f64::INFINITY {
            "Infinity".to_owned()
        } else if number == f64::NEG_INFINITY {
            "-Infinity".to_owned()
        } else {
            number.to_string()
        };
        (canonical == string).then_some(number)
    }

    fn to_index(&mut self, value: V8Value) -> Completion<u64, V8Types> {
        let result = self.call_js_helper(
            "value => { if (value === undefined) return 0; const integer = Math.trunc(Number(value)); if (integer < 0 || integer > Number.MAX_SAFE_INTEGER || !Number.isFinite(integer)) throw new RangeError('invalid index'); return integer || 0; }",
            &[value],
        )?;
        Ok(V8Types::value_as_number(&result).unwrap_or(0.0) as u64)
    }

    fn require_object_coercible(&mut self, value: V8Value) -> Completion<V8Value, V8Types> {
        if V8Types::value_is_null(&value) || V8Types::value_is_undefined(&value) {
            Err(self.new_type_error("value is null or undefined"))
        } else {
            Ok(value)
        }
    }

    fn is_array(&mut self, value: &V8Value) -> Completion<bool, V8Types> {
        let result =
            self.call_js_helper("value => Array.isArray(value)", std::slice::from_ref(value))?;
        Ok(V8Types::value_as_bool(&result).unwrap_or(false))
    }

    fn is_constructor(&self, value: &V8Value) -> bool {
        value
            .object_profile
            .as_ref()
            .is_some_and(|profile| profile.function_handle.is_some())
    }

    fn is_extensible(&mut self, object: &V8Object) -> Completion<bool, V8Types> {
        let result = self.call_js_helper(
            "value => Object.isExtensible(value)",
            std::slice::from_ref(&object.0),
        )?;
        Ok(V8Types::value_as_bool(&result).unwrap_or(false))
    }

    fn is_integral_number(&self, value: &V8Value) -> bool {
        V8Types::value_as_number(value)
            .is_some_and(|number| number.is_finite() && number.fract() == 0.0)
    }

    fn is_property_key(&self, value: &V8Value) -> bool {
        V8Types::value_as_string(value).is_some() || V8Types::value_as_symbol(value).is_some()
    }

    fn same_value(&self, left: &V8Value, right: &V8Value) -> bool {
        if left.isolate_id != right.isolate_id {
            return false;
        }
        match (&left.primitive, &right.primitive) {
            (CachedPrimitive::Undefined, CachedPrimitive::Undefined)
            | (CachedPrimitive::Null, CachedPrimitive::Null) => true,
            (CachedPrimitive::Boolean(left), CachedPrimitive::Boolean(right)) => left == right,
            (CachedPrimitive::Number(left), CachedPrimitive::Number(right)) => {
                (left.is_nan() && right.is_nan())
                    || (left == right
                        && (left != &0.0 || left.is_sign_positive() == right.is_sign_positive()))
            }
            (CachedPrimitive::String(left), CachedPrimitive::String(right)) => left == right,
            (CachedPrimitive::BigInt(left), CachedPrimitive::BigInt(right)) => left == right,
            (CachedPrimitive::Other, CachedPrimitive::Other) => left.handle == right.handle,
            _ => false,
        }
    }

    fn same_value_zero(&self, left: &V8Value, right: &V8Value) -> bool {
        if let (Some(left_number), Some(right_number)) = (
            V8Types::value_as_number(left),
            V8Types::value_as_number(right),
        ) {
            return left_number == right_number || (left_number.is_nan() && right_number.is_nan());
        }
        self.is_strictly_equal(left, right)
    }

    fn is_loosely_equal(&mut self, left: V8Value, right: V8Value) -> Completion<bool, V8Types> {
        let result = self.call_js_helper("(left, right) => left == right", &[left, right])?;
        Ok(V8Types::value_as_bool(&result).unwrap_or(false))
    }

    fn is_strictly_equal(&self, left: &V8Value, right: &V8Value) -> bool {
        if left.isolate_id != right.isolate_id {
            return false;
        }
        match (&left.primitive, &right.primitive) {
            (CachedPrimitive::Number(left), CachedPrimitive::Number(right)) => {
                !left.is_nan() && !right.is_nan() && left == right
            }
            _ => self.same_value(left, right),
        }
    }

    fn get(
        &mut self,
        object: V8Object,
        property_key: V8PropertyKey,
    ) -> Completion<V8Value, V8Types> {
        let _current_engine = CurrentEngineGuard::enter(self);
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            v8::tc_scope!(let try_catch, scope);
            let object = local_object(try_catch, isolate_id, &object)?;
            let key = local_property_key(try_catch, isolate_id, &property_key)?;
            let Some(value) = object.get(try_catch, key) else {
                return Err(caught!(try_catch, isolate_id, "property get failed"));
            };
            Ok(wrap_local_value(try_catch, isolate_id, value))
        })
    }

    fn get_v(
        &mut self,
        value: V8Value,
        property_key: V8PropertyKey,
    ) -> Completion<V8Value, V8Types> {
        let object = self.to_object(value)?;
        ExecutionContext::get(self, object, property_key)
    }

    fn set(
        &mut self,
        object: V8Object,
        property_key: V8PropertyKey,
        value: V8Value,
        throw: bool,
    ) -> Completion<(), V8Types> {
        let _current_engine = CurrentEngineGuard::enter(self);
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            v8::tc_scope!(let try_catch, scope);
            let object = local_object(try_catch, isolate_id, &object)?;
            let key = local_property_key(try_catch, isolate_id, &property_key)?;
            let value = local_value(try_catch, isolate_id, &value)?;
            match object.set(try_catch, key, value) {
                Some(true) => Ok(()),
                Some(false) if !throw => Ok(()),
                Some(false) => Err(caught_exception(
                    try_catch,
                    isolate_id,
                    None,
                    "property assignment was rejected",
                )),
                None => Err(caught!(try_catch, isolate_id, "property assignment failed")),
            }
        })
    }

    fn create_data_property(
        &mut self,
        object: V8Object,
        property_key: V8PropertyKey,
        value: V8Value,
    ) -> Completion<bool, V8Types> {
        let _current_engine = CurrentEngineGuard::enter(self);
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            v8::tc_scope!(let try_catch, scope);
            let object = local_object(try_catch, isolate_id, &object)?;
            let key = local_name(try_catch, isolate_id, &property_key)?;
            let value = local_value(try_catch, isolate_id, &value)?;
            object
                .create_data_property(try_catch, key, value)
                .ok_or_else(|| caught!(try_catch, isolate_id, "CreateDataProperty failed"))
        })
    }

    fn to_property_descriptor(
        &mut self,
        descriptor_object: V8Object,
    ) -> Completion<PropertyDescriptor<V8Types>, V8Types> {
        let mut descriptor = PropertyDescriptor {
            value: None,
            writable: None,
            get: None,
            set: None,
            enumerable: None,
            configurable: None,
        };
        for property in [
            "enumerable",
            "configurable",
            "value",
            "writable",
            "get",
            "set",
        ] {
            let key = self.property_key_from_str(property);
            if !self.has_property(descriptor_object.clone(), key.clone())? {
                continue;
            }
            let value = ExecutionContext::get(self, descriptor_object.clone(), key)?;
            match property {
                "enumerable" => descriptor.enumerable = Some(self.to_boolean(&value)),
                "configurable" => descriptor.configurable = Some(self.to_boolean(&value)),
                "value" => descriptor.value = Some(value),
                "writable" => descriptor.writable = Some(self.to_boolean(&value)),
                "get" => {
                    if !V8Types::value_is_undefined(&value) {
                        let object = V8Types::value_as_object(&value)
                            .and_then(|object| V8Types::object_as_function(&object))
                            .ok_or_else(|| {
                                self.new_type_error("descriptor getter is not callable")
                            })?;
                        descriptor.get = Some(object);
                    }
                }
                "set" => {
                    if !V8Types::value_is_undefined(&value) {
                        let object = V8Types::value_as_object(&value)
                            .and_then(|object| V8Types::object_as_function(&object))
                            .ok_or_else(|| {
                                self.new_type_error("descriptor setter is not callable")
                            })?;
                        descriptor.set = Some(object);
                    }
                }
                _ => unreachable!(),
            }
        }
        if (descriptor.get.is_some() || descriptor.set.is_some())
            && (descriptor.value.is_some() || descriptor.writable.is_some())
        {
            return Err(self.new_type_error("invalid mixed property descriptor"));
        }
        Ok(descriptor)
    }

    fn define_property_or_throw(
        &mut self,
        object: V8Object,
        property_key: V8PropertyKey,
        descriptor: PropertyDescriptor<V8Types>,
    ) -> Completion<(), V8Types> {
        let _current_engine = CurrentEngineGuard::enter(self);
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            v8::tc_scope!(let try_catch, scope);
            let object = local_object(try_catch, isolate_id, &object)?;
            let key = local_name(try_catch, isolate_id, &property_key)?;

            let undefined = v8::undefined(try_catch).into();
            let mut v8_descriptor = if descriptor.get.is_some() || descriptor.set.is_some() {
                let getter = match &descriptor.get {
                    Some(getter) => {
                        local_typed_object(try_catch, isolate_id, &getter.0, &getter.1)?.into()
                    }
                    None => undefined,
                };
                let setter = match &descriptor.set {
                    Some(setter) => {
                        local_typed_object(try_catch, isolate_id, &setter.0, &setter.1)?.into()
                    }
                    None => undefined,
                };
                v8::PropertyDescriptor::new_from_get_set(getter, setter)
            } else {
                let value = match &descriptor.value {
                    Some(value) => local_value(try_catch, isolate_id, value)?,
                    None => undefined,
                };
                v8::PropertyDescriptor::new_from_value_writable(
                    value,
                    descriptor.writable.unwrap_or(false),
                )
            };
            if let Some(enumerable) = descriptor.enumerable {
                v8_descriptor.set_enumerable(enumerable);
            }
            if let Some(configurable) = descriptor.configurable {
                v8_descriptor.set_configurable(configurable);
            }
            match object.define_property(try_catch, key, &v8_descriptor) {
                Some(true) => Ok(()),
                Some(false) => Err(caught_exception(
                    try_catch,
                    isolate_id,
                    None,
                    "DefinePropertyOrThrow was rejected",
                )),
                None => Err(caught!(
                    try_catch,
                    isolate_id,
                    "DefinePropertyOrThrow failed"
                )),
            }
        })
    }

    fn delete_property_or_throw(
        &mut self,
        object: V8Object,
        property_key: V8PropertyKey,
    ) -> Completion<(), V8Types> {
        let _current_engine = CurrentEngineGuard::enter(self);
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            v8::tc_scope!(let try_catch, scope);
            let object = local_object(try_catch, isolate_id, &object)?;
            let key = local_property_key(try_catch, isolate_id, &property_key)?;
            match object.delete(try_catch, key) {
                Some(true) => Ok(()),
                Some(false) => Err(caught_exception(
                    try_catch,
                    isolate_id,
                    None,
                    "DeletePropertyOrThrow was rejected",
                )),
                None => Err(caught!(
                    try_catch,
                    isolate_id,
                    "DeletePropertyOrThrow failed"
                )),
            }
        })
    }

    fn get_prototype_of(&mut self, object: V8Object) -> Completion<Option<V8Object>, V8Types> {
        let prototype =
            self.call_js_helper("object => Object.getPrototypeOf(object)", &[object.0])?;
        if V8Types::value_is_null(&prototype) {
            Ok(None)
        } else {
            V8Types::value_as_object(&prototype)
                .map(Some)
                .ok_or_else(|| self.new_type_error("GetPrototypeOf did not return an object"))
        }
    }

    fn set_prototype(
        &mut self,
        object: V8Object,
        prototype: Option<V8Object>,
    ) -> Completion<bool, V8Types> {
        let prototype = prototype.map_or_else(|| self.value_null(), |prototype| prototype.0);
        let result = self.call_js_helper(
            "(object, prototype) => Reflect.setPrototypeOf(object, prototype)",
            &[object.0, prototype],
        )?;
        V8Types::value_as_bool(&result)
            .ok_or_else(|| self.new_type_error("SetPrototypeOf did not return a boolean"))
    }

    fn get_method(
        &mut self,
        value: V8Value,
        property_key: V8PropertyKey,
    ) -> Completion<Option<V8Function>, V8Types> {
        let method = self.get_v(value, property_key)?;
        if V8Types::value_is_null(&method) || V8Types::value_is_undefined(&method) {
            return Ok(None);
        }
        let function = V8Types::value_as_object(&method)
            .and_then(|object| V8Types::object_as_function(&object))
            .ok_or_else(|| self.new_type_error("property is not callable"))?;
        Ok(Some(function))
    }

    fn has_property(
        &mut self,
        object: V8Object,
        property_key: V8PropertyKey,
    ) -> Completion<bool, V8Types> {
        let _current_engine = CurrentEngineGuard::enter(self);
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            v8::tc_scope!(let try_catch, scope);
            let object = local_object(try_catch, isolate_id, &object)?;
            let key = local_property_key(try_catch, isolate_id, &property_key)?;
            object
                .has(try_catch, key)
                .ok_or_else(|| caught!(try_catch, isolate_id, "HasProperty failed"))
        })
    }

    fn has_own_property(
        &mut self,
        object: V8Object,
        property_key: V8PropertyKey,
    ) -> Completion<bool, V8Types> {
        let _current_engine = CurrentEngineGuard::enter(self);
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            v8::tc_scope!(let try_catch, scope);
            let object = local_object(try_catch, isolate_id, &object)?;
            let key = local_name(try_catch, isolate_id, &property_key)?;
            object
                .has_own_property(try_catch, key)
                .ok_or_else(|| caught!(try_catch, isolate_id, "HasOwnProperty failed"))
        })
    }

    fn own_property_keys(&mut self, object: V8Object) -> Completion<Vec<V8PropertyKey>, V8Types> {
        let array = self.call_js_helper("object => Reflect.ownKeys(object)", &[object.0])?;
        let array = V8Types::value_as_object(&array)
            .ok_or_else(|| self.new_type_error("Reflect.ownKeys did not return an array"))?;
        let length_value =
            ExecutionContext::get(self, array.clone(), self.property_key_from_str("length"))?;
        let length = V8Types::value_as_number(&length_value).unwrap_or(0.0) as u32;
        let mut keys = Vec::with_capacity(length as usize);
        for index in 0..length {
            let value = ExecutionContext::get(self, array.clone(), V8PropertyKey::Index(index))?;
            keys.push(self.to_property_key(value)?);
        }
        Ok(keys)
    }

    fn get_own_property(
        &mut self,
        object: V8Object,
        property_key: V8PropertyKey,
    ) -> Completion<Option<PropertyDescriptor<V8Types>>, V8Types> {
        let descriptor = {
            let _current_engine = CurrentEngineGuard::enter(self);
            let isolate_id = self.isolate_id;
            v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
                v8::tc_scope!(let try_catch, scope);
                let object = local_object(try_catch, isolate_id, &object)?;
                let key = local_name(try_catch, isolate_id, &property_key)?;
                let Some(descriptor) = object.get_own_property_descriptor(try_catch, key) else {
                    if let Some(exception) = try_catch.exception() {
                        return Err(caught_exception(
                            try_catch,
                            isolate_id,
                            Some(exception),
                            "GetOwnProperty failed",
                        ));
                    }
                    return Ok(None);
                };
                if descriptor.is_undefined() {
                    return Ok(None);
                }
                V8Types::value_as_object(&wrap_local_value(try_catch, isolate_id, descriptor))
                    .ok_or_else(|| {
                        caught_exception(try_catch, isolate_id, None, "invalid property descriptor")
                    })
            })?
        };
        self.to_property_descriptor(descriptor).map(Some)
    }

    fn construct(
        &mut self,
        function: V8Constructor,
        arguments: &[V8Value],
        new_target: Option<V8Constructor>,
    ) -> Completion<V8Object, V8Types> {
        if let Some(new_target) = new_target {
            let mut helper_arguments = vec![function.0.0];
            helper_arguments.push(new_target.0.0);
            helper_arguments.extend_from_slice(arguments);
            let result = self.call_js_helper(
                "(constructor, newTarget, ...args) => Reflect.construct(constructor, args, newTarget)",
                &helper_arguments,
            )?;
            return V8Types::value_as_object(&result)
                .ok_or_else(|| self.new_type_error("constructor did not return an object"));
        }

        let wrapper_primitive = self.realm_state.intrinsics.as_ref().and_then(|intrinsics| {
            let argument = arguments.first().map(|value| value.primitive.clone());
            if function.0 == intrinsics.boolean.0 {
                Some(argument.unwrap_or(CachedPrimitive::Boolean(false)))
            } else if function.0 == intrinsics.number.0 {
                Some(argument.unwrap_or(CachedPrimitive::Number(0.0)))
            } else if function.0 == intrinsics.string.0 {
                Some(argument.unwrap_or_else(|| CachedPrimitive::String(Arc::from([]))))
            } else if function.0 == intrinsics.bigint.0 {
                argument
            } else {
                None
            }
        });
        let _current_engine = CurrentEngineGuard::enter(self);
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            v8::tc_scope!(let try_catch, scope);
            let function = local_typed_object(try_catch, isolate_id, &function.0, &function.1)?;
            let local_arguments: Result<Vec<_>, _> = arguments
                .iter()
                .map(|argument| local_value(try_catch, isolate_id, argument))
                .collect();
            let Some(object) = function.new_instance(try_catch, &local_arguments?) else {
                return Err(caught!(try_catch, isolate_id, "constructor call failed"));
            };
            let mut object =
                object_from_wrapped_value(wrap_local_value(try_catch, isolate_id, object.into()));
            if let Some(wrapper_primitive) = wrapper_primitive
                && let Some(profile) = object.0.object_profile.as_mut()
            {
                profile.wrapper_primitive = Some(wrapper_primitive);
            }
            Ok(object)
        })
    }

    fn set_integrity_level(
        &mut self,
        object: V8Object,
        level: IntegrityLevel,
    ) -> Completion<bool, V8Types> {
        let _current_engine = CurrentEngineGuard::enter(self);
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            v8::tc_scope!(let try_catch, scope);
            let object = local_object(try_catch, isolate_id, &object)?;
            let level = match level {
                IntegrityLevel::Sealed => v8::IntegrityLevel::Sealed,
                IntegrityLevel::Frozen => v8::IntegrityLevel::Frozen,
            };
            object
                .set_integrity_level(try_catch, level)
                .ok_or_else(|| caught!(try_catch, isolate_id, "SetIntegrityLevel failed"))
        })
    }

    fn test_integrity_level(
        &mut self,
        object: V8Object,
        level: IntegrityLevel,
    ) -> Completion<bool, V8Types> {
        let source = match level {
            IntegrityLevel::Sealed => "object => Object.isSealed(object)",
            IntegrityLevel::Frozen => "object => Object.isFrozen(object)",
        };
        let result = self.call_js_helper(source, &[object.0])?;
        Ok(V8Types::value_as_bool(&result).unwrap_or(false))
    }

    fn species_constructor(
        &mut self,
        object: V8Object,
        default_constructor: V8Constructor,
    ) -> Completion<V8Constructor, V8Types> {
        let result = self.call_js_helper(
            "(object, defaultConstructor) => { const constructor = object.constructor; if (constructor === undefined) return defaultConstructor; if (Object(constructor) !== constructor) throw new TypeError('constructor is not an object'); const species = constructor[Symbol.species]; if (species == null) return defaultConstructor; if (typeof species !== 'function') throw new TypeError('species is not a constructor'); return species; }",
            &[object.0, default_constructor.0.0],
        )?;
        let result = V8Types::value_as_object(&result)
            .ok_or_else(|| self.new_type_error("SpeciesConstructor did not return an object"))?;
        V8Types::object_as_constructor(&result)
            .ok_or_else(|| self.new_type_error("SpeciesConstructor did not return a constructor"))
    }

    fn get_function_realm(&mut self, function: &V8Object) -> Completion<V8Realm, V8Types> {
        if function.0.isolate_id != self.isolate_id {
            return Err(self.new_type_error("function belongs to a different V8 isolate"));
        }
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let function = local_object(scope, isolate_id, function)?;
            let context = function.get_creation_context(scope).ok_or_else(|| {
                caught_exception(scope, isolate_id, None, "function has no creation context")
            })?;
            Ok(V8Realm {
                isolate_id,
                context: v8::Global::new(scope, context),
            })
        })
    }

    fn get_iterator(
        &mut self,
        object: V8Value,
        kind: IteratorKind,
        method: Option<V8Function>,
    ) -> Completion<IteratorRecord<V8Types>, V8Types> {
        let method = match method {
            Some(method) => method,
            None => {
                let symbol_name = match kind {
                    IteratorKind::Sync => "iterator",
                    IteratorKind::Async => "asyncIterator",
                };
                let key = self.property_key_from_well_known_symbol(symbol_name);
                self.get_method(object.clone(), key)?
                    .ok_or_else(|| self.new_type_error("value is not iterable"))?
            }
        };
        let iterator_value = EcmascriptHost::call(self, &method.0, &object, &[])?;
        let iterator = V8Types::value_as_object(&iterator_value)
            .ok_or_else(|| self.new_type_error("iterator method did not return an object"))?;
        let next = self
            .get_method(iterator_value, self.property_key_from_str("next"))?
            .ok_or_else(|| self.new_type_error("iterator has no next method"))?;
        Ok(IteratorRecord {
            iterator,
            next_method: next,
            done: false,
        })
    }

    fn iterator_step_value(
        &mut self,
        iterator: &mut IteratorRecord<V8Types>,
    ) -> Completion<Option<V8Value>, V8Types> {
        let this_value = iterator.iterator.0.clone();
        let result = EcmascriptHost::call(self, &iterator.next_method.0, &this_value, &[])?;
        let result_object = V8Types::value_as_object(&result)
            .ok_or_else(|| self.new_type_error("iterator result is not an object"))?;
        let done = ExecutionContext::get(
            self,
            result_object.clone(),
            self.property_key_from_str("done"),
        )?;
        if self.to_boolean(&done) {
            iterator.done = true;
            return Ok(None);
        }
        ExecutionContext::get(self, result_object, self.property_key_from_str("value")).map(Some)
    }

    fn iterator_close(
        &mut self,
        iterator: IteratorRecord<V8Types>,
        completion: Completion<V8Value, V8Types>,
    ) -> Completion<V8Value, V8Types> {
        let iterator_value = iterator.iterator.0.clone();
        if let Some(return_method) =
            self.get_method(iterator_value.clone(), self.property_key_from_str("return"))?
        {
            let close_result = EcmascriptHost::call(self, &return_method.0, &iterator_value, &[])?;
            if V8Types::value_as_object(&close_result).is_none() {
                return Err(self.new_type_error("iterator return method did not return an object"));
            }
        }
        completion
    }

    fn async_iterator_close(
        &mut self,
        iterator: IteratorRecord<V8Types>,
        completion: Completion<V8Value, V8Types>,
    ) -> Completion<V8Value, V8Types> {
        self.iterator_close(iterator, completion)
    }

    fn current_realm(&self) -> V8Realm {
        self.realm_state.realm.clone()
    }

    fn realm_global_object(&self) -> V8Object {
        self.realm_state.realm_global.borrow().clone()
    }

    fn realm_intrinsics(&self, realm: &V8Realm) -> RealmIntrinsics<V8Types> {
        assert_eq!(
            realm.isolate_id, self.isolate_id,
            "realm belongs to a different V8 isolate"
        );
        self.state_for_realm(realm)
            .unwrap_or_else(|| Rc::clone(&self.realm_state))
            .intrinsics
            .as_ref()
            .expect("V8 intrinsics are not initialized")
            .clone()
    }

    fn enqueue_job(&mut self, job: Box<dyn FnOnce()>) {
        self.shared_isolate
            .queued_jobs
            .borrow_mut()
            .push_back(QueuedJob::Plain(Rc::clone(&self.realm_state), job));
    }

    fn enqueue_job_with_realm(
        &mut self,
        realm: V8Realm,
        job: Box<dyn FnOnce(&mut dyn ExecutionContext<V8Types>)>,
    ) {
        assert_eq!(
            realm.isolate_id, self.isolate_id,
            "queued job realm belongs to a different V8 isolate"
        );
        let realm_state = self
            .state_for_realm(&realm)
            .unwrap_or_else(|| Rc::clone(&self.realm_state));
        self.shared_isolate
            .queued_jobs
            .borrow_mut()
            .push_back(QueuedJob::WithRealm(realm_state, job));
    }

    fn run_jobs(&mut self) {
        if let Err(exception) = self.perform_a_microtask_checkpoint() {
            self.report_exception(exception);
        }
    }

    fn is_detached_buffer(&self, array_buffer: &V8ArrayBuffer) -> bool {
        array_buffer
            .0
            .0
            .object_profile
            .as_ref()
            .and_then(|profile| profile.array_buffer_state.as_ref())
            .is_some_and(|state| state.detached.get())
    }

    fn is_fixed_length_array_buffer(&self, array_buffer: &V8ArrayBuffer) -> bool {
        array_buffer
            .0
            .0
            .object_profile
            .as_ref()
            .and_then(|profile| profile.array_buffer_state.as_ref())
            .is_some_and(|state| !state.resizable)
    }

    fn allocate_array_buffer(
        &mut self,
        _constructor: V8Constructor,
        byte_length: u64,
        max_byte_length: Option<u64>,
    ) -> Completion<V8ArrayBuffer, V8Types> {
        if let Some(max_byte_length) = max_byte_length {
            let byte_length = self.value_from_number(byte_length as f64);
            let max_byte_length = self.value_from_number(max_byte_length as f64);
            let value = self.call_js_helper(
                "(length, maxByteLength) => new ArrayBuffer(length, { maxByteLength })",
                &[byte_length, max_byte_length],
            )?;
            let object = V8Types::value_as_object(&value)
                .ok_or_else(|| self.new_type_error("ArrayBuffer allocation failed"))?;
            return V8Types::object_as_array_buffer(&object)
                .ok_or_else(|| self.new_type_error("ArrayBuffer allocation failed"));
        }
        let isolate_id = self.isolate_id;
        let byte_length = usize::try_from(byte_length)
            .map_err(|_| self.new_range_error("ArrayBuffer length is too large"))?;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let array_buffer = v8::ArrayBuffer::new(scope, byte_length);
            let object =
                object_from_wrapped_value(wrap_local_value(scope, isolate_id, array_buffer.into()));
            V8Types::object_as_array_buffer(&object)
                .ok_or_else(|| self.new_type_error("ArrayBuffer allocation failed"))
        })
    }

    fn clone_array_buffer(
        &mut self,
        source: V8ArrayBuffer,
        source_byte_offset: u64,
        source_length: u64,
        constructor: V8Constructor,
    ) -> Completion<V8ArrayBuffer, V8Types> {
        let bytes = self
            .array_buffer_data(&source)
            .ok_or_else(|| self.new_type_error("source ArrayBuffer is detached"))?;
        let start = usize::try_from(source_byte_offset)
            .map_err(|_| self.new_range_error("source byte offset is too large"))?;
        let length = usize::try_from(source_length)
            .map_err(|_| self.new_range_error("source length is too large"))?;
        let end = start
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| self.new_range_error("ArrayBuffer clone range is out of bounds"))?;
        let clone =
            ExecutionContext::allocate_array_buffer(self, constructor, source_length, None)?;
        let state = clone
            .0
            .0
            .object_profile
            .as_ref()
            .and_then(|profile| profile.array_buffer_state.as_ref())
            .expect("new ArrayBuffer has no backing store");
        for (destination, source) in state.backing_store.iter().zip(&bytes[start..end]) {
            Cell::set(destination, *source);
        }
        Ok(clone)
    }

    fn detach_array_buffer(
        &mut self,
        array_buffer: V8ArrayBuffer,
        key: Option<V8Value>,
    ) -> Completion<(), V8Types> {
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let local = local_typed_object(scope, isolate_id, &array_buffer.0, &array_buffer.1)?;
            let key = key
                .as_ref()
                .map(|key| local_value(scope, isolate_id, key))
                .transpose()?;
            match local.detach(key) {
                Some(true) => {
                    if let Some(state) = array_buffer
                        .0
                        .0
                        .object_profile
                        .as_ref()
                        .and_then(|profile| profile.array_buffer_state.as_ref())
                    {
                        state.detached.set(true);
                    }
                    Ok(())
                }
                Some(false) | None => Err(caught_exception(
                    scope,
                    isolate_id,
                    None,
                    "ArrayBuffer detach key did not match",
                )),
            }
        })
    }

    fn get_value_from_buffer(
        &mut self,
        array_buffer: &V8ArrayBuffer,
        byte_index: u64,
        element_type: TypedArrayElementType,
        _is_typed_array: bool,
        _order: SharedMemoryOrder,
    ) -> V8Value {
        let constructor = match element_type {
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
        let byte_index = self.value_from_number(byte_index as f64);
        self.call_js_helper(
            &format!("(buffer, offset) => new {constructor}(buffer, offset, 1)[0]"),
            &[array_buffer.0.0.clone(), byte_index],
        )
        .unwrap_or_else(|exception| exception)
    }

    fn set_value_in_buffer(
        &mut self,
        array_buffer: &V8ArrayBuffer,
        byte_index: u64,
        element_type: TypedArrayElementType,
        value: V8Value,
        _is_typed_array: bool,
        _order: SharedMemoryOrder,
    ) -> Completion<(), V8Types> {
        let constructor = match element_type {
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
        let byte_index = self.value_from_number(byte_index as f64);
        self.call_js_helper(
            &format!(
                "(buffer, offset, value) => {{ new {constructor}(buffer, offset, 1)[0] = value; }}"
            ),
            &[array_buffer.0.0.clone(), byte_index, value],
        )?;
        Ok(())
    }

    fn typed_array_buffer(
        &mut self,
        typed_array: &V8TypedArray,
    ) -> Completion<V8ArrayBuffer, V8Types> {
        let value = self.call_js_helper(
            "view => view.buffer",
            std::slice::from_ref(&typed_array.0.0),
        )?;
        let object = V8Types::value_as_object(&value)
            .ok_or_else(|| self.new_type_error("typed array has no ArrayBuffer"))?;
        V8Types::object_as_array_buffer(&object)
            .ok_or_else(|| self.new_type_error("typed array has no ArrayBuffer"))
    }

    fn typed_array_byte_offset(&mut self, typed_array: &V8TypedArray) -> Completion<u64, V8Types> {
        let value = self.call_js_helper(
            "view => view.byteOffset",
            std::slice::from_ref(&typed_array.0.0),
        )?;
        Ok(V8Types::value_as_number(&value).unwrap_or(0.0) as u64)
    }

    fn typed_array_byte_length(&mut self, typed_array: &V8TypedArray) -> Completion<u64, V8Types> {
        let value = self.call_js_helper(
            "view => view.byteLength",
            std::slice::from_ref(&typed_array.0.0),
        )?;
        Ok(V8Types::value_as_number(&value).unwrap_or(0.0) as u64)
    }

    fn typed_array_element_type(
        &self,
        typed_array: &V8TypedArray,
    ) -> Option<TypedArrayElementType> {
        typed_array
            .0
            .0
            .object_profile
            .as_ref()
            .and_then(|profile| profile.typed_array_element_type)
    }

    fn construct_typed_array_view(
        &mut self,
        element_type: TypedArrayElementType,
        buffer: V8ArrayBuffer,
        byte_offset: u64,
        byte_length: u64,
    ) -> Completion<V8TypedArray, V8Types> {
        let isolate_id = self.isolate_id;
        let byte_offset = usize::try_from(byte_offset)
            .map_err(|_| self.new_range_error("typed array byte offset is too large"))?;
        let element_size = match element_type {
            TypedArrayElementType::Int8
            | TypedArrayElementType::Uint8
            | TypedArrayElementType::Uint8Clamped => 1,
            TypedArrayElementType::Int16
            | TypedArrayElementType::Uint16
            | TypedArrayElementType::Float16 => 2,
            TypedArrayElementType::Int32
            | TypedArrayElementType::Uint32
            | TypedArrayElementType::Float32 => 4,
            TypedArrayElementType::Float64
            | TypedArrayElementType::BigInt64
            | TypedArrayElementType::BigUint64 => 8,
        };
        if !byte_length.is_multiple_of(element_size) {
            return Err(self.new_range_error("typed array byte length is not element-aligned"));
        }
        let length = usize::try_from(byte_length / element_size)
            .map_err(|_| self.new_range_error("typed array length is too large"))?;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let buffer = local_typed_object(scope, isolate_id, &buffer.0, &buffer.1)?;
            if element_type == TypedArrayElementType::Float16 {
                return Err(caught_exception(
                    scope,
                    isolate_id,
                    None,
                    "Float16Array is not exposed by this rusty_v8 build",
                ));
            }
            let view: Option<v8::Local<v8::Value>> = match element_type {
                TypedArrayElementType::Int8 => {
                    v8::Int8Array::new(scope, buffer, byte_offset, length).map(Into::into)
                }
                TypedArrayElementType::Uint8 => {
                    v8::Uint8Array::new(scope, buffer, byte_offset, length).map(Into::into)
                }
                TypedArrayElementType::Uint8Clamped => {
                    v8::Uint8ClampedArray::new(scope, buffer, byte_offset, length).map(Into::into)
                }
                TypedArrayElementType::Int16 => {
                    v8::Int16Array::new(scope, buffer, byte_offset, length).map(Into::into)
                }
                TypedArrayElementType::Uint16 => {
                    v8::Uint16Array::new(scope, buffer, byte_offset, length).map(Into::into)
                }
                TypedArrayElementType::Int32 => {
                    v8::Int32Array::new(scope, buffer, byte_offset, length).map(Into::into)
                }
                TypedArrayElementType::Uint32 => {
                    v8::Uint32Array::new(scope, buffer, byte_offset, length).map(Into::into)
                }
                TypedArrayElementType::Float16 => unreachable!(),
                TypedArrayElementType::Float32 => {
                    v8::Float32Array::new(scope, buffer, byte_offset, length).map(Into::into)
                }
                TypedArrayElementType::Float64 => {
                    v8::Float64Array::new(scope, buffer, byte_offset, length).map(Into::into)
                }
                TypedArrayElementType::BigInt64 => {
                    v8::BigInt64Array::new(scope, buffer, byte_offset, length).map(Into::into)
                }
                TypedArrayElementType::BigUint64 => {
                    v8::BigUint64Array::new(scope, buffer, byte_offset, length).map(Into::into)
                }
            };
            let view = view.ok_or_else(|| {
                caught_exception(scope, isolate_id, None, "typed array construction failed")
            })?;
            let object = object_from_wrapped_value(wrap_local_value(scope, isolate_id, view));
            V8Types::object_as_typed_array(&object)
                .ok_or_else(|| self.new_type_error("typed array construction failed"))
        })
    }

    fn data_view_buffer(&mut self, data_view: &V8DataView) -> Completion<V8ArrayBuffer, V8Types> {
        let value =
            self.call_js_helper("view => view.buffer", std::slice::from_ref(&data_view.0.0))?;
        let object = V8Types::value_as_object(&value)
            .ok_or_else(|| self.new_type_error("DataView has no ArrayBuffer"))?;
        V8Types::object_as_array_buffer(&object)
            .ok_or_else(|| self.new_type_error("DataView has no ArrayBuffer"))
    }

    fn data_view_byte_offset(&mut self, data_view: &V8DataView) -> Completion<u64, V8Types> {
        let value = self.call_js_helper(
            "view => view.byteOffset",
            std::slice::from_ref(&data_view.0.0),
        )?;
        Ok(V8Types::value_as_number(&value).unwrap_or(0.0) as u64)
    }

    fn data_view_byte_length(&mut self, data_view: &V8DataView) -> Completion<u64, V8Types> {
        let value = self.call_js_helper(
            "view => view.byteLength",
            std::slice::from_ref(&data_view.0.0),
        )?;
        Ok(V8Types::value_as_number(&value).unwrap_or(0.0) as u64)
    }

    fn construct_data_view_from_buffer(
        &mut self,
        buffer: V8ArrayBuffer,
        byte_offset: u64,
        byte_length: u64,
    ) -> Completion<V8DataView, V8Types> {
        let isolate_id = self.isolate_id;
        let byte_offset = usize::try_from(byte_offset)
            .map_err(|_| self.new_range_error("DataView byte offset is too large"))?;
        let byte_length = usize::try_from(byte_length)
            .map_err(|_| self.new_range_error("DataView byte length is too large"))?;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let buffer = local_typed_object(scope, isolate_id, &buffer.0, &buffer.1)?;
            let data_view = v8::DataView::new(scope, buffer, byte_offset, byte_length);
            let object =
                object_from_wrapped_value(wrap_local_value(scope, isolate_id, data_view.into()));
            V8Types::object_as_data_view(&object)
                .ok_or_else(|| self.new_type_error("DataView construction failed"))
        })
    }

    fn array_buffer_data(&self, array_buffer: &V8ArrayBuffer) -> Option<Vec<u8>> {
        let state = array_buffer
            .0
            .0
            .object_profile
            .as_ref()?
            .array_buffer_state
            .as_ref()?;
        if state.detached.get() {
            return None;
        }
        Some(state.backing_store.iter().map(Cell::get).collect())
    }

    fn get_date_value(&mut self, date: &V8Object) -> Completion<f64, V8Types> {
        let value = self.call_js_helper(
            "date => Date.prototype.getTime.call(date)",
            std::slice::from_ref(&date.0),
        )?;
        V8Types::value_as_number(&value)
            .ok_or_else(|| self.new_type_error("Date value is not a number"))
    }

    fn get_regexp_source(&mut self, regexp: &V8Object) -> Completion<String, V8Types> {
        let value =
            self.call_js_helper("regexp => regexp.source", std::slice::from_ref(&regexp.0))?;
        self.to_rust_string(value)
    }

    fn get_regexp_flags(&mut self, regexp: &V8Object) -> Completion<String, V8Types> {
        let value =
            self.call_js_helper("regexp => regexp.flags", std::slice::from_ref(&regexp.0))?;
        self.to_rust_string(value)
    }

    fn map_get_entries(&mut self, map: &V8Map) -> Completion<Vec<(V8Value, V8Value)>, V8Types> {
        let entries = self.call_js_helper(
            "map => Array.from(map.entries()).flat()",
            std::slice::from_ref(&map.0.0),
        )?;
        let entries = V8Types::value_as_object(&entries)
            .ok_or_else(|| self.new_type_error("Map entries did not produce an array"))?;
        let length =
            ExecutionContext::get(self, entries.clone(), self.property_key_from_str("length"))?;
        let length = V8Types::value_as_number(&length).unwrap_or(0.0) as u32;
        let mut result = Vec::with_capacity((length / 2) as usize);
        for index in (0..length).step_by(2) {
            let key = ExecutionContext::get(self, entries.clone(), V8PropertyKey::Index(index))?;
            let value =
                ExecutionContext::get(self, entries.clone(), V8PropertyKey::Index(index + 1))?;
            result.push((key, value));
        }
        Ok(result)
    }

    fn map_set_entry(
        &mut self,
        map: &V8Map,
        key: V8Value,
        value: V8Value,
    ) -> Completion<(), V8Types> {
        self.call_js_helper(
            "(map, key, value) => { map.set(key, value); }",
            &[map.0.0.clone(), key, value],
        )?;
        Ok(())
    }

    fn set_get_values(&mut self, set: &V8Set) -> Completion<Vec<V8Value>, V8Types> {
        let values = self.call_js_helper(
            "set => Array.from(set.values())",
            std::slice::from_ref(&set.0.0),
        )?;
        let values = V8Types::value_as_object(&values)
            .ok_or_else(|| self.new_type_error("Set values did not produce an array"))?;
        let length =
            ExecutionContext::get(self, values.clone(), self.property_key_from_str("length"))?;
        let length = V8Types::value_as_number(&length).unwrap_or(0.0) as u32;
        let mut result = Vec::with_capacity(length as usize);
        for index in 0..length {
            result.push(ExecutionContext::get(
                self,
                values.clone(),
                V8PropertyKey::Index(index),
            )?);
        }
        Ok(result)
    }

    fn set_add_entry(&mut self, set: &V8Set, value: V8Value) -> Completion<(), V8Types> {
        self.call_js_helper(
            "(set, value) => { set.add(value); }",
            &[set.0.0.clone(), value],
        )?;
        Ok(())
    }

    fn promise_resolve(
        &mut self,
        constructor: V8Constructor,
        value: V8Value,
    ) -> Completion<V8Promise, V8Types> {
        let promise = self.call_js_helper(
            "(constructor, value) => Promise.resolve.call(constructor, value)",
            &[constructor.0.0, value],
        )?;
        let object = V8Types::value_as_object(&promise)
            .ok_or_else(|| self.new_type_error("PromiseResolve did not return a promise"))?;
        V8Types::object_as_promise(&object)
            .ok_or_else(|| self.new_type_error("PromiseResolve did not return a promise"))
    }

    fn new_promise_capability(
        &mut self,
        constructor: V8Constructor,
    ) -> Completion<PromiseCapability<V8Types>, V8Types> {
        let parts = self.call_js_helper(
            "constructor => { let resolve, reject; const promise = new constructor((res, rej) => { resolve = res; reject = rej; }); return [promise, resolve, reject]; }",
            &[constructor.0.0],
        )?;
        let parts = V8Types::value_as_object(&parts)
            .ok_or_else(|| self.new_type_error("promise capability did not produce an array"))?;
        let promise = ExecutionContext::get(self, parts.clone(), V8PropertyKey::Index(0))?;
        let resolve = ExecutionContext::get(self, parts.clone(), V8PropertyKey::Index(1))?;
        let reject = ExecutionContext::get(self, parts, V8PropertyKey::Index(2))?;
        let resolve = V8Types::value_as_object(&resolve)
            .and_then(|object| V8Types::object_as_function(&object))
            .ok_or_else(|| self.new_type_error("promise resolve is not callable"))?;
        let reject = V8Types::value_as_object(&reject)
            .and_then(|object| V8Types::object_as_function(&object))
            .ok_or_else(|| self.new_type_error("promise reject is not callable"))?;
        Ok(PromiseCapability {
            promise,
            resolve,
            reject,
        })
    }

    fn new_promise_pending(&mut self) -> Completion<(V8Value, PromiseResolvers<V8Types>), V8Types> {
        let realm = self.realm_state.realm.clone();
        let promise_constructor = self.realm_intrinsics(&realm).promise;
        let capability = self.new_promise_capability(promise_constructor)?;
        let promise = capability.promise;
        let resolve = capability.resolve;
        let reject = capability.reject;
        let resolvers = PromiseResolvers::new(resolve.0, reject.0, self);
        Ok((promise, resolvers))
    }

    fn perform_promise_then(
        &mut self,
        promise: V8Promise,
        on_fulfilled: Option<V8Function>,
        on_rejected: Option<V8Function>,
        result_capability: Option<PromiseCapability<V8Types>>,
    ) -> Completion<V8Value, V8Types> {
        let returned_promise = result_capability
            .as_ref()
            .map(|capability| capability.promise.clone());
        let fulfilled_capability = result_capability
            .as_ref()
            .map(|capability| (capability.resolve.clone(), capability.reject.clone()));
        let rejected_capability = result_capability
            .as_ref()
            .map(|capability| (capability.resolve.clone(), capability.reject.clone()));
        let empty_name = self.property_key_from_str("");

        let fulfilled_handler = self.make_builtin_function(
            Box::new(move |arguments, _this, execution_context| {
                let value = arguments
                    .first()
                    .cloned()
                    .unwrap_or_else(|| execution_context.value_undefined());
                let completion = if let Some(on_fulfilled) = &on_fulfilled {
                    let undefined = execution_context.value_undefined();
                    execution_context.call(&on_fulfilled.0, &undefined, &[value])
                } else {
                    Ok(value)
                };
                let Some((resolve, reject)) = &fulfilled_capability else {
                    return completion;
                };
                let undefined = execution_context.value_undefined();
                match completion {
                    Ok(value) => match execution_context.call(&resolve.0, &undefined, &[value]) {
                        Ok(_) => Ok(undefined),
                        Err(exception) => {
                            execution_context.call(&reject.0, &undefined, &[exception])?;
                            Ok(undefined)
                        }
                    },
                    Err(exception) => {
                        execution_context.call(&reject.0, &undefined, &[exception])?;
                        Ok(undefined)
                    }
                }
            }),
            1,
            empty_name.clone(),
            false,
        );
        let rejected_handler = self.make_builtin_function(
            Box::new(move |arguments, _this, execution_context| {
                let reason = arguments
                    .first()
                    .cloned()
                    .unwrap_or_else(|| execution_context.value_undefined());
                let completion = if let Some(on_rejected) = &on_rejected {
                    let undefined = execution_context.value_undefined();
                    execution_context.call(&on_rejected.0, &undefined, &[reason])
                } else {
                    Err(reason)
                };
                let Some((resolve, reject)) = &rejected_capability else {
                    return completion;
                };
                let undefined = execution_context.value_undefined();
                match completion {
                    Ok(value) => match execution_context.call(&resolve.0, &undefined, &[value]) {
                        Ok(_) => Ok(undefined),
                        Err(exception) => {
                            execution_context.call(&reject.0, &undefined, &[exception])?;
                            Ok(undefined)
                        }
                    },
                    Err(exception) => {
                        execution_context.call(&reject.0, &undefined, &[exception])?;
                        Ok(undefined)
                    }
                }
            }),
            1,
            empty_name,
            false,
        );

        let _current_engine = CurrentEngineGuard::enter(self);
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            v8::tc_scope!(let try_catch, scope);
            let promise = local_typed_object(try_catch, isolate_id, &promise.0, &promise.1)?;
            let fulfilled_handler = local_typed_object(
                try_catch,
                isolate_id,
                &fulfilled_handler.0,
                &fulfilled_handler.1,
            )?;
            let rejected_handler = local_typed_object(
                try_catch,
                isolate_id,
                &rejected_handler.0,
                &rejected_handler.1,
            )?;
            let Some(derived_promise) =
                promise.then2(try_catch, fulfilled_handler, rejected_handler)
            else {
                return Err(caught!(
                    try_catch,
                    isolate_id,
                    "failed to register promise reactions"
                ));
            };
            Ok(returned_promise
                .unwrap_or_else(|| wrap_local_value(try_catch, isolate_id, derived_promise.into())))
        })
    }

    fn promise_state(&mut self, promise: &V8Object) -> Completion<PromiseState<V8Types>, V8Types> {
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let promise = V8Types::object_as_promise(promise).ok_or_else(|| {
                caught_exception(scope, isolate_id, None, "value is not a Promise")
            })?;
            let promise = local_typed_object(scope, isolate_id, &promise.0, &promise.1)?;
            match promise.state() {
                v8::PromiseState::Pending => Ok(PromiseState::Pending),
                v8::PromiseState::Fulfilled => {
                    let result = promise.result(scope);
                    Ok(PromiseState::Fulfilled(wrap_local_value(
                        scope, isolate_id, result,
                    )))
                }
                v8::PromiseState::Rejected => {
                    let result = promise.result(scope);
                    Ok(PromiseState::Rejected(wrap_local_value(
                        scope, isolate_id, result,
                    )))
                }
            }
        })
    }

    fn generator_start(
        &mut self,
        _generator: V8Generator,
        _closure: V8Function,
    ) -> Completion<(), V8Types> {
        Ok(())
    }

    fn global_object(&self) -> V8Object {
        self.realm_state.realm_global.borrow().clone()
    }

    fn property_key_from_str(&self, string: &str) -> V8PropertyKey {
        V8PropertyKey::String(self.js_string_from_str(string))
    }

    fn property_key_from_index(&self, index: u32) -> V8PropertyKey {
        V8PropertyKey::Index(index)
    }

    fn property_key_from_symbol(&self, symbol: &V8Symbol) -> V8PropertyKey {
        V8PropertyKey::Symbol(symbol.clone())
    }

    fn value_from_property_key(&mut self, key: V8PropertyKey) -> V8Value {
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            match local_property_key(scope, isolate_id, &key) {
                Ok(value) => wrap_local_value(scope, isolate_id, value),
                Err(error) => error,
            }
        })
    }

    fn property_key_from_well_known_symbol(&mut self, name: &str) -> V8PropertyKey {
        let name = self.value_from_string(self.js_string_from_str(name));
        let symbol = self
            .call_js_helper("name => Symbol[name]", &[name])
            .ok()
            .and_then(|value| V8Types::value_as_symbol(&value))
            .unwrap_or_else(|| panic!("unknown well-known Symbol name"));
        V8PropertyKey::Symbol(symbol)
    }

    fn property_key_to_rust_string(&self, key: &V8PropertyKey) -> String {
        match key {
            V8PropertyKey::String(string) => String::from_utf16_lossy(&string.utf16),
            V8PropertyKey::Symbol(symbol) => {
                let _ = symbol;
                "Symbol".to_owned()
            }
            V8PropertyKey::Index(index) => index.to_string(),
        }
    }

    fn store_host_any(&mut self, id: TypeId, value: Box<dyn Any>) {
        self.realm_host_data_mut().values.insert(id, value);
    }

    fn get_host_any(&self, id: &TypeId) -> Option<&dyn Any> {
        self.realm_host_data().values.get(id).map(Box::as_ref)
    }

    fn remove_host_any(&mut self, id: &TypeId) -> Option<Box<dyn Any>> {
        self.realm_host_data_mut().values.remove(id)
    }

    fn create_object_with_any(
        &mut self,
        prototype: V8Object,
        data: Box<dyn Any + 'static>,
    ) -> V8Object {
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let object_template = v8::ObjectTemplate::new(scope);
            object_template.set_internal_field_count(2);
            let object = object_template
                .new_instance(scope)
                .expect("V8 failed to create a platform object");
            let prototype = local_value(scope, isolate_id, &prototype.0)
                .expect("platform-object prototype belongs to another isolate");
            object
                .set_prototype(scope, prototype)
                .expect("V8 failed to set a platform-object prototype");

            let record = Box::new(HostObjectRecord { data });
            let record_pointer = Box::into_raw(record);
            let marker = v8::External::new(
                scope,
                std::ptr::addr_of!(HOST_OBJECT_MARKER).cast_mut().cast(),
            );
            assert!(object.set_internal_field(0, marker.into()));
            object.set_aligned_pointer_in_internal_field(1, record_pointer.cast(), HOST_OBJECT_TAG);

            let value =
                object_from_wrapped_value(wrap_local_value(scope, isolate_id, object.into()));
            let weak = v8::Weak::with_guaranteed_finalizer(
                scope,
                object,
                Box::new(move || {
                    // SAFETY: `record_pointer` is created by the single
                    // Box::into_raw above. This guaranteed finalizer is its sole
                    // owner and V8 invokes it at most once after the JS object can
                    // no longer expose its internal field.
                    unsafe {
                        drop(Box::from_raw(record_pointer));
                    }
                }),
            );
            self.shared_isolate
                .host_object_handles
                .borrow_mut()
                .push(weak);
            value
        })
    }

    fn with_object_any(&self, object: &V8Object) -> Option<&dyn Any> {
        if let Some(pointer) = object.0.host_data {
            // SAFETY: `host_data_pointer` validates the marker and tag before
            // placing this pointer in a V8Value. A Global handle in `object`
            // keeps the JS object reachable, so its guaranteed finalizer cannot
            // release the record during this borrow.
            let record = unsafe { &*pointer.cast::<HostObjectRecord>().as_ptr() };
            return Some(record.data.as_ref());
        }
        self.realm_host_data()
            .associated_objects
            .iter()
            .find(|(candidate, _)| candidate == object)
            .map(|(_, data)| data.as_ref())
    }

    fn with_object_any_mut(&mut self, object: &V8Object) -> Option<&mut dyn Any> {
        if let Some(pointer) = object.0.host_data {
            // SAFETY: The marker, reachability, and finalization invariants are
            // the same as in `with_object_any`. The `&mut self` receiver makes
            // this the exclusive host-data access path for the duration of the
            // returned borrow.
            let record = unsafe { &mut *pointer.cast::<HostObjectRecord>().as_ptr() };
            return Some(record.data.as_mut());
        }
        self.realm_host_data_mut()
            .associated_objects
            .iter_mut()
            .find(|(candidate, _)| candidate == object)
            .map(|(_, data)| data.as_mut())
    }

    fn with_object_any_mut_with(
        &mut self,
        object: &V8Object,
        operation: Box<dyn FnOnce(&mut dyn Any, &mut dyn ExecutionContext<V8Types>) + '_>,
    ) {
        let data_pointer: Option<*mut dyn Any> = if let Some(pointer) = object.0.host_data {
            // SAFETY: The validated host pointer remains alive because
            // `object` owns a Global handle. Converting to a raw trait-object
            // pointer lets the callback borrow both the record and the engine;
            // no other access to the record occurs until the callback returns.
            let record = unsafe { &mut *pointer.cast::<HostObjectRecord>().as_ptr() };
            Some(record.data.as_mut())
        } else {
            self.realm_host_data_mut()
                .associated_objects
                .iter_mut()
                .find(|(candidate, _)| candidate == object)
                .map(|(_, data)| data.as_mut() as *mut dyn Any)
        };
        if let Some(data_pointer) = data_pointer {
            // SAFETY: `data_pointer` was obtained from storage exclusively
            // borrowed above and is used only for this call.
            unsafe {
                operation(&mut *data_pointer, self);
            }
        }
    }

    fn new_type_error(&mut self, message: &str) -> V8Value {
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let message =
                v8::String::new(scope, message).expect("V8 TypeError message allocation failed");
            let exception = v8::Exception::type_error(scope, message);
            wrap_local_value(scope, isolate_id, exception)
        })
    }

    fn new_range_error(&mut self, message: &str) -> V8Value {
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let message =
                v8::String::new(scope, message).expect("V8 RangeError message allocation failed");
            let exception = v8::Exception::range_error(scope, message);
            wrap_local_value(scope, isolate_id, exception)
        })
    }

    fn new_syntax_error(&mut self, message: &str) -> V8Value {
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let message =
                v8::String::new(scope, message).expect("V8 SyntaxError message allocation failed");
            let exception = v8::Exception::syntax_error(scope, message);
            wrap_local_value(scope, isolate_id, exception)
        })
    }

    fn create_proxy(
        &mut self,
        target: V8Object,
        handler: V8Object,
    ) -> Completion<V8Object, V8Types> {
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let target = local_object(scope, isolate_id, &target)?;
            let handler = local_object(scope, isolate_id, &handler)?;
            let proxy = v8::Proxy::new(scope, target, handler).ok_or_else(|| {
                caught_exception(scope, isolate_id, None, "Proxy creation failed")
            })?;
            Ok(object_from_wrapped_value(wrap_local_value(
                scope,
                isolate_id,
                proxy.into(),
            )))
        })
    }

    fn js_string_to_rust_string(&self, string: &V8String) -> String {
        String::from_utf16_lossy(&string.utf16)
    }

    fn create_empty_array(&mut self) -> V8Object {
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let array = v8::Array::new(scope, 0);
            object_from_wrapped_value(wrap_local_value(scope, isolate_id, array.into()))
        })
    }

    fn array_push(&mut self, array: &V8Object, value: V8Value) -> Completion<(), V8Types> {
        self.call_js_helper(
            "(array, value) => { array.push(value); }",
            &[array.0.clone(), value],
        )?;
        Ok(())
    }

    fn create_plain_object(&mut self, prototype: Option<&V8Object>) -> V8Object {
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let object = v8::Object::new(scope);
            if let Some(prototype) = prototype {
                let prototype = local_value(scope, isolate_id, &prototype.0)
                    .expect("plain-object prototype belongs to another isolate");
                object
                    .set_prototype(scope, prototype)
                    .expect("V8 failed to set plain-object prototype");
            }
            object_from_wrapped_value(wrap_local_value(scope, isolate_id, object.into()))
        })
    }

    fn json_stringify(&mut self, value: V8Value) -> Completion<String, V8Types> {
        let result = self.call_js_helper("value => JSON.stringify(value)", &[value])?;
        if V8Types::value_is_undefined(&result) {
            return Ok("null".to_owned());
        }
        self.to_rust_string(result)
    }

    fn evaluate_script(&mut self, source: &str) -> Completion<V8Value, V8Types> {
        let realm = self.realm_state.realm.clone();
        JsEngine::evaluate_script(self, source, &realm)
    }

    fn value_from_bigint(&mut self, number: i64) -> V8Value {
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let bigint = v8::BigInt::new_from_i64(scope, number);
            wrap_local_value(scope, isolate_id, bigint.into())
        })
    }

    fn create_builtin_fn_static(
        &mut self,
        behaviour: fn(
            &[V8Value],
            V8Value,
            &mut dyn ExecutionContext<V8Types>,
        ) -> Completion<V8Value, V8Types>,
        length: u32,
        name: V8PropertyKey,
    ) -> V8Function {
        self.make_builtin_function(Box::new(behaviour), length, name, false)
    }

    fn create_builtin_fn(
        &mut self,
        behaviour: StoredBehaviour,
        length: u32,
        name: V8PropertyKey,
    ) -> V8Function {
        self.make_builtin_function(behaviour, length, name, false)
    }

    fn create_builtin_function(
        &mut self,
        behaviour: StoredBehaviour,
        length: u32,
        name: V8PropertyKey,
        is_constructor: bool,
    ) -> V8Function {
        self.make_builtin_function(behaviour, length, name, is_constructor)
    }
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;
    use std::cell::{Cell, RefCell};
    use std::ptr::from_ref;
    use std::rc::Rc;

    use rusty_v8 as v8;

    use crate::{EcmascriptHost, ExecutionContext, JsTypes};

    use super::super::{V8AsyncGenerator, V8WeakMap, V8WeakRef, V8WeakSet};
    use super::{
        CURRENT_CALLBACK_ISOLATE_ID, CURRENT_CALLBACK_SCOPE, V8ArrayBuffer, V8Constructor,
        V8DataView, V8Engine, V8Function, V8Generator, V8Map, V8Object, V8Promise, V8Set,
        V8SharedArrayBuffer, V8TypedArray, V8Types,
    };

    struct DropFlag(Rc<Cell<bool>>);

    struct RealmMarker(&'static str);

    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }

    #[test]
    fn object_categories_keep_distinct_strong_v8_handles() {
        let type_ids = [
            TypeId::of::<V8Object>(),
            TypeId::of::<V8ArrayBuffer>(),
            TypeId::of::<V8SharedArrayBuffer>(),
            TypeId::of::<V8TypedArray>(),
            TypeId::of::<V8DataView>(),
            TypeId::of::<V8Promise>(),
            TypeId::of::<V8Map>(),
            TypeId::of::<V8Set>(),
            TypeId::of::<V8WeakMap>(),
            TypeId::of::<V8WeakSet>(),
            TypeId::of::<V8WeakRef>(),
            TypeId::of::<V8Generator>(),
            TypeId::of::<V8AsyncGenerator>(),
            TypeId::of::<V8Function>(),
            TypeId::of::<V8Constructor>(),
        ];
        for (index, type_id) in type_ids.iter().enumerate() {
            assert!(!type_ids[..index].contains(type_id));
        }

        let mut engine = V8Engine::new();
        let mut evaluate_object = |source| {
            let value = ExecutionContext::evaluate_script(&mut engine, source)
                .expect("the typed V8 object must evaluate");
            V8Types::value_as_object(&value).expect("the evaluated value must be an object")
        };

        let array_buffer = evaluate_object("new ArrayBuffer(8)");
        let array_buffer = V8Types::object_as_array_buffer(&array_buffer)
            .expect("ArrayBuffer must retain a Global<ArrayBuffer>");
        let _: &v8::Global<v8::ArrayBuffer> = &array_buffer.1;

        let shared_array_buffer = evaluate_object("new SharedArrayBuffer(8)");
        let shared_array_buffer = V8Types::object_as_shared_array_buffer(&shared_array_buffer)
            .expect("SharedArrayBuffer must retain a Global<SharedArrayBuffer>");
        let _: &v8::Global<v8::SharedArrayBuffer> = &shared_array_buffer.1;

        let typed_array = evaluate_object("new Uint8Array(8)");
        let typed_array = V8Types::object_as_typed_array(&typed_array)
            .expect("TypedArray must retain a Global<TypedArray>");
        let _: &v8::Global<v8::TypedArray> = &typed_array.1;

        let data_view = evaluate_object("new DataView(new ArrayBuffer(8))");
        let data_view = V8Types::object_as_data_view(&data_view)
            .expect("DataView must retain a Global<DataView>");
        let _: &v8::Global<v8::DataView> = &data_view.1;

        let promise = evaluate_object("Promise.resolve(1)");
        let promise =
            V8Types::object_as_promise(&promise).expect("Promise must retain a Global<Promise>");
        let _: &v8::Global<v8::Promise> = &promise.1;

        let map = evaluate_object("new Map()");
        let map = V8Types::object_as_map(&map).expect("Map must retain a Global<Map>");
        let _: &v8::Global<v8::Map> = &map.1;

        let set = evaluate_object("new Set()");
        let set = V8Types::object_as_set(&set).expect("Set must retain a Global<Set>");
        let _: &v8::Global<v8::Set> = &set.1;

        let function = evaluate_object("(function typedFunction() {})");
        let function = V8Types::object_as_function(&function)
            .expect("Function must retain a Global<Function>");
        let _: &v8::Global<v8::Function> = &function.1;
    }

    #[test]
    fn cross_isolate_handles_return_an_exception() {
        let mut first_engine = V8Engine::new();
        let value = first_engine.value_from_number(42.0);
        let mut second_engine = V8Engine::new();

        assert!(second_engine.to_number(value).is_err());
    }

    #[test]
    fn thrown_javascript_becomes_a_completion_error() {
        let mut engine = V8Engine::new();
        let exception = ExecutionContext::evaluate_script(
            &mut engine,
            "throw new TypeError('expected exception')",
        )
        .expect_err("throwing script must produce a completion error");
        let object = V8Types::value_as_object(&exception)
            .expect("the caught TypeError must be represented as an object");

        assert!(V8Types::object_is_error(&object));
    }

    #[test]
    fn child_realms_share_an_isolate_and_survive_parent_drop() {
        let parent_engine = V8Engine::new();
        let mut first_document_engine = parent_engine.new_child_realm();
        let mut second_document_engine = parent_engine.new_child_realm();

        let first_value = ExecutionContext::evaluate_script(&mut first_document_engine, "21 * 2")
            .expect("the first child realm must evaluate JavaScript");
        assert_eq!(
            first_document_engine
                .to_number(first_value)
                .expect("the first result must be numeric"),
            42.0
        );

        drop(first_document_engine);
        drop(parent_engine);

        let callback_name = second_document_engine.property_key_from_str("sharedCallback");
        let callback = second_document_engine.create_builtin_function(
            Box::new(|_arguments, _this, execution_context| {
                Ok(execution_context.value_from_number(7.0))
            }),
            0,
            callback_name.clone(),
            false,
        );
        let callback_value = V8Types::value_from_object(callback.0);
        let global = second_document_engine.realm_global_object();
        second_document_engine
            .create_data_property(global, callback_name, callback_value)
            .expect("the callback must be installed in the second child realm");

        let second_value =
            ExecutionContext::evaluate_script(&mut second_document_engine, "sharedCallback()")
                .expect("the remaining child realm must invoke native callbacks");
        assert_eq!(
            second_document_engine
                .to_number(second_value)
                .expect("the callback result must be numeric"),
            7.0
        );
    }

    #[test]
    fn sibling_contexts_use_the_shared_explicit_microtask_queue() {
        let parent_engine = V8Engine::new();
        let first_engine = parent_engine.new_child_realm();
        let second_engine = parent_engine.new_child_realm();

        assert!(Rc::ptr_eq(
            &first_engine.shared_isolate,
            &second_engine.shared_isolate
        ));

        let shared_queue = from_ref(&*first_engine.shared_isolate.microtask_queue);
        let first_realm = first_engine.current_realm();
        let first_queue =
            v8_engine_scope_with_context!(scope, first_engine, &first_realm.context, {
                let context = v8::Local::new(scope, &first_realm.context);
                from_ref(context.get_microtask_queue())
            });
        let second_realm = second_engine.current_realm();
        let second_queue =
            v8_engine_scope_with_context!(scope, second_engine, &second_realm.context, {
                let context = v8::Local::new(scope, &second_realm.context);
                from_ref(context.get_microtask_queue())
            });

        assert_eq!(first_queue, shared_queue);
        assert_eq!(second_queue, shared_queue);
    }

    #[test]
    fn promise_callback_runs_with_its_creation_realm() {
        let parent_engine = V8Engine::new();
        let mut checkpoint_engine = parent_engine.new_child_realm();
        let mut promise_engine = parent_engine.new_child_realm();
        checkpoint_engine.store_host_any(
            TypeId::of::<RealmMarker>(),
            Box::new(RealmMarker("checkpoint")),
        );
        promise_engine.store_host_any(
            TypeId::of::<RealmMarker>(),
            Box::new(RealmMarker("promise")),
        );

        let promise_global = promise_engine.realm_global_object();
        let expected_global = promise_global.clone();
        let callback_used_creation_realm = Rc::new(Cell::new(false));
        let callback_result = Rc::clone(&callback_used_creation_realm);
        let callback_name = promise_engine.property_key_from_str("creationRealmCallback");
        let callback = promise_engine.create_builtin_function(
            Box::new(move |_arguments, _this, execution_context| {
                let marker = execution_context
                    .get_host_any(&TypeId::of::<RealmMarker>())
                    .and_then(|data| data.downcast_ref::<RealmMarker>())
                    .map(|marker| marker.0);
                callback_result.set(
                    execution_context.realm_global_object() == expected_global
                        && marker == Some("promise"),
                );
                Ok(execution_context.value_undefined())
            }),
            0,
            callback_name.clone(),
            false,
        );
        promise_engine
            .create_data_property(
                promise_global,
                callback_name,
                V8Types::value_from_object(callback.0),
            )
            .expect("the promise callback must be installed");
        ExecutionContext::evaluate_script(
            &mut promise_engine,
            "Promise.resolve().then(creationRealmCallback)",
        )
        .expect("the promise callback must be queued");

        checkpoint_engine
            .perform_a_microtask_checkpoint()
            .expect("the sibling checkpoint must drain the shared queue");

        assert!(callback_used_creation_realm.get());
        assert_eq!(
            checkpoint_engine
                .get_host_any(&TypeId::of::<RealmMarker>())
                .and_then(|data| data.downcast_ref::<RealmMarker>())
                .map(|marker| marker.0),
            Some("checkpoint")
        );
    }

    #[test]
    fn perform_promise_then_bypasses_a_patched_then_method() {
        let mut engine = V8Engine::new();
        let promise = ExecutionContext::evaluate_script(
            &mut engine,
            "globalThis.thenCalls = 0; const originalThen = Promise.prototype.then; Promise.prototype.then = function(...arguments) { thenCalls += 1; return originalThen.apply(this, arguments); }; Promise.resolve(42)",
        )
        .expect("the patched Promise and source promise must be created");
        let promise =
            V8Types::value_as_object(&promise).expect("the source value must be a Promise");
        let promise =
            V8Types::object_as_promise(&promise).expect("the source value must be a Promise");
        let callback_called = Rc::new(Cell::new(false));
        let callback_result = Rc::clone(&callback_called);
        let callback_name = engine.property_key_from_str("promiseReaction");
        let callback = engine.create_builtin_function(
            Box::new(move |_arguments, _this, execution_context| {
                callback_result.set(true);
                Ok(execution_context.value_undefined())
            }),
            1,
            callback_name,
            false,
        );

        engine
            .perform_promise_then(promise, Some(callback), None, None)
            .expect("the direct V8 promise reaction must be registered");
        engine
            .perform_a_microtask_checkpoint()
            .expect("the direct V8 promise reaction must run");
        let then_calls = ExecutionContext::evaluate_script(&mut engine, "thenCalls")
            .expect("the patched then call count must be readable");

        assert!(callback_called.get());
        assert_eq!(
            engine
                .to_number(then_calls)
                .expect("the patched then call count must be numeric"),
            0.0
        );
    }

    #[test]
    fn nested_cross_realm_callbacks_restore_each_previous_realm() {
        let parent_engine = V8Engine::new();
        let mut first_engine = parent_engine.new_child_realm();
        let mut second_engine = parent_engine.new_child_realm();
        first_engine.store_host_any(TypeId::of::<RealmMarker>(), Box::new(RealmMarker("first")));
        second_engine.store_host_any(TypeId::of::<RealmMarker>(), Box::new(RealmMarker("second")));

        let first_global = first_engine.realm_global_object();
        let first_callback_global = first_global.clone();
        let first_callback_used_first_realm = Rc::new(Cell::new(false));
        let first_callback_result = Rc::clone(&first_callback_used_first_realm);
        let first_callback_name = first_engine.property_key_from_str("firstRealmCallback");
        let first_callback = first_engine.create_builtin_function(
            Box::new(move |_arguments, _this, execution_context| {
                let marker = execution_context
                    .get_host_any(&TypeId::of::<RealmMarker>())
                    .and_then(|data| data.downcast_ref::<RealmMarker>())
                    .map(|marker| marker.0);
                first_callback_result.set(
                    execution_context.realm_global_object() == first_callback_global
                        && marker == Some("first"),
                );
                panic!("intentional nested callback panic")
            }),
            0,
            first_callback_name,
            false,
        );

        let second_global = second_engine.realm_global_object();
        let second_callback_global = second_global.clone();
        let second_callback_restored = Rc::new(Cell::new(false));
        let second_callback_result = Rc::clone(&second_callback_restored);
        let second_callback_name = second_engine.property_key_from_str("secondRealmCallback");
        let second_callback = second_engine.create_builtin_function(
            Box::new(move |_arguments, _this, execution_context| {
                let undefined = execution_context.value_undefined();
                let inner_result = execution_context.call(&first_callback.0, &undefined, &[]);
                assert!(inner_result.is_err());
                let marker = execution_context
                    .get_host_any(&TypeId::of::<RealmMarker>())
                    .and_then(|data| data.downcast_ref::<RealmMarker>())
                    .map(|marker| marker.0);
                second_callback_result.set(
                    execution_context.realm_global_object() == second_callback_global
                        && marker == Some("second"),
                );
                Ok(execution_context.value_undefined())
            }),
            0,
            second_callback_name.clone(),
            false,
        );
        second_engine
            .create_data_property(
                second_global,
                second_callback_name,
                V8Types::value_from_object(second_callback.0),
            )
            .expect("the second callback must be installed");
        ExecutionContext::evaluate_script(
            &mut second_engine,
            "Promise.resolve().then(secondRealmCallback)",
        )
        .expect("the nested callback must be queued");

        first_engine
            .perform_a_microtask_checkpoint()
            .expect("the nested callbacks must complete");

        assert!(first_callback_used_first_realm.get());
        assert!(second_callback_restored.get());
        assert_eq!(first_engine.realm_global_object(), first_global);
    }

    #[test]
    fn rust_jobs_use_their_creation_realm_and_drain_until_stable() {
        let parent_engine = V8Engine::new();
        let mut checkpoint_engine = parent_engine.new_child_realm();
        let mut promise_engine = parent_engine.new_child_realm();
        promise_engine.store_host_any(
            TypeId::of::<RealmMarker>(),
            Box::new(RealmMarker("promise")),
        );

        let execution_steps = Rc::new(RefCell::new(Vec::new()));
        let callback_steps = Rc::clone(&execution_steps);
        let promise_global = promise_engine.realm_global_object();
        let expected_global = promise_global.clone();
        let callback_name = promise_engine.property_key_from_str("enqueueRealmJobs");
        let callback = promise_engine.create_builtin_function(
            Box::new(move |_arguments, _this, execution_context| {
                callback_steps.borrow_mut().push("promise");
                let first_job_steps = Rc::clone(&callback_steps);
                let first_job_global = expected_global.clone();
                let realm = execution_context.current_realm();
                execution_context.enqueue_job_with_realm(
                    realm,
                    Box::new(move |job_context| {
                        let marker = job_context
                            .get_host_any(&TypeId::of::<RealmMarker>())
                            .and_then(|data| data.downcast_ref::<RealmMarker>())
                            .map(|marker| marker.0);
                        assert_eq!(job_context.realm_global_object(), first_job_global);
                        assert_eq!(marker, Some("promise"));
                        first_job_steps.borrow_mut().push("first job");

                        let second_job_steps = Rc::clone(&first_job_steps);
                        let second_realm = job_context.current_realm();
                        job_context.enqueue_job_with_realm(
                            second_realm,
                            Box::new(move |nested_job_context| {
                                let marker = nested_job_context
                                    .get_host_any(&TypeId::of::<RealmMarker>())
                                    .and_then(|data| data.downcast_ref::<RealmMarker>())
                                    .map(|marker| marker.0);
                                assert_eq!(marker, Some("promise"));
                                second_job_steps.borrow_mut().push("second job");
                            }),
                        );
                    }),
                );
                Ok(execution_context.value_undefined())
            }),
            0,
            callback_name.clone(),
            false,
        );
        promise_engine
            .create_data_property(
                promise_global,
                callback_name,
                V8Types::value_from_object(callback.0),
            )
            .expect("the job callback must be installed");
        ExecutionContext::evaluate_script(
            &mut promise_engine,
            "Promise.resolve().then(enqueueRealmJobs)",
        )
        .expect("the promise job must be queued");

        checkpoint_engine
            .perform_a_microtask_checkpoint()
            .expect("the shared queues must drain until stable");

        assert_eq!(
            execution_steps.borrow().as_slice(),
            ["promise", "first job", "second job"]
        );
    }

    #[test]
    fn queued_realm_state_survives_context_destruction_and_forced_gc() {
        let mut parent_engine = V8Engine::new();
        let mut child_engine = parent_engine.new_child_realm();
        let realm_data_dropped = Rc::new(Cell::new(false));
        child_engine.store_host_any(
            TypeId::of::<DropFlag>(),
            Box::new(DropFlag(Rc::clone(&realm_data_dropped))),
        );
        let job_ran = Rc::new(Cell::new(false));
        let job_result = Rc::clone(&job_ran);
        let child_realm = child_engine.current_realm();
        child_engine.enqueue_job_with_realm(
            child_realm,
            Box::new(move |execution_context| {
                assert!(
                    execution_context
                        .get_host_any(&TypeId::of::<DropFlag>())
                        .and_then(|data| data.downcast_ref::<DropFlag>())
                        .is_some()
                );
                job_result.set(true);
            }),
        );

        drop(child_engine);
        assert!(!realm_data_dropped.get());

        parent_engine
            .perform_a_microtask_checkpoint()
            .expect("the queued child-realm job must remain valid");
        parent_engine.gc();

        assert!(job_ran.get());
        assert!(realm_data_dropped.get());
    }

    #[test]
    fn native_callback_can_create_a_child_realm() {
        let mut engine = V8Engine::new();
        let callback_name = engine.property_key_from_str("createChildRealm");
        let callback = engine.create_builtin_function(
            Box::new(|_arguments, _this, execution_context| {
                let engine = execution_context
                    .as_any_mut()
                    .downcast_mut::<V8Engine>()
                    .expect("the callback context must be a V8 engine");
                let mut child_engine = engine.new_child_realm();
                let child_value = ExecutionContext::evaluate_script(&mut child_engine, "6 * 7")?;
                let number = child_engine.to_number(child_value)?;
                drop(child_engine);
                Ok(engine.value_from_number(number))
            }),
            0,
            callback_name.clone(),
            false,
        );
        let callback_value = V8Types::value_from_object(callback.0);
        let global = engine.realm_global_object();
        engine
            .create_data_property(global, callback_name, callback_value)
            .expect("the callback must be installed in the parent realm");

        let result = ExecutionContext::evaluate_script(&mut engine, "createChildRealm()")
            .expect("a native callback must create and evaluate a child realm");
        assert_eq!(
            engine
                .to_number(result)
                .expect("the callback result must be numeric"),
            42.0
        );
    }

    #[test]
    fn prototype_proxy_traps_can_call_native_functions_and_throw() {
        let mut engine = V8Engine::new();
        let callback_called = Rc::new(Cell::new(false));
        let callback_called_by_function = Rc::clone(&callback_called);
        let callback_name = engine.property_key_from_str("prototypeTrap");
        let callback = engine.create_builtin_function(
            Box::new(move |_arguments, _this, execution_context| {
                callback_called_by_function.set(true);
                Ok(execution_context.value_undefined())
            }),
            0,
            callback_name.clone(),
            false,
        );
        let callback_value = V8Types::value_from_object(callback.0);
        let global = engine.realm_global_object();
        engine
            .create_data_property(global, callback_name, callback_value)
            .expect("the callback must be installed in the realm");

        let proxy = ExecutionContext::evaluate_script(
            &mut engine,
            "new Proxy({}, { getPrototypeOf() { prototypeTrap(); throw new Error('prototype failure'); } })",
        )
        .expect("the proxy must be created");
        let proxy = V8Types::value_as_object(&proxy).expect("the proxy must be an object");

        assert!(engine.get_prototype_of(proxy).is_err());
        assert!(callback_called.get());

        callback_called.set(false);
        let proxy = ExecutionContext::evaluate_script(
            &mut engine,
            "new Proxy({}, { setPrototypeOf() { prototypeTrap(); return true; } })",
        )
        .expect("the second proxy must be created");
        let proxy = V8Types::value_as_object(&proxy).expect("the second proxy must be an object");

        assert!(
            engine
                .set_prototype(proxy, None)
                .expect("the setPrototypeOf trap must succeed")
        );
        assert!(callback_called.get());
    }

    #[test]
    fn get_function_realm_returns_the_creation_realm() {
        let mut parent_engine = V8Engine::new();
        let mut child_engine = parent_engine.new_child_realm();
        let function = ExecutionContext::evaluate_script(&mut child_engine, "(() => 42)")
            .expect("the child realm must create a function");
        let function = V8Types::value_as_object(&function).expect("the value must be a function");

        let function_realm = parent_engine
            .get_function_realm(&function)
            .expect("the function realm must be found");

        let child_realm = child_engine.current_realm();
        assert_eq!(function_realm.isolate_id, child_realm.isolate_id);
        assert_eq!(function_realm.context, child_realm.context);
    }

    #[test]
    fn weak_finalizers_release_host_objects_and_callbacks() {
        let mut engine = V8Engine::new();

        let host_object_dropped = Rc::new(Cell::new(false));
        let prototype = engine.create_plain_object(None);
        let host_object = engine.create_object_with_any(
            prototype,
            Box::new(DropFlag(Rc::clone(&host_object_dropped))),
        );
        drop(host_object);

        let callback_dropped = Rc::new(Cell::new(false));
        let callback_drop_flag = DropFlag(Rc::clone(&callback_dropped));
        let callback_name = engine.property_key_from_str("finalizedCallback");
        let callback = engine.create_builtin_function(
            Box::new(move |_arguments, _this, execution_context| {
                let _drop_flag = &callback_drop_flag;
                Ok(execution_context.value_undefined())
            }),
            0,
            callback_name,
            false,
        );
        drop(callback);

        engine.gc();

        assert!(host_object_dropped.get());
        assert!(callback_dropped.get());
    }
}

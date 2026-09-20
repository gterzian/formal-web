use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell, RefMut};
use std::collections::{HashMap, HashSet, VecDeque};
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
use crate::records::{
    IteratorRecord, ModuleRequest, PromiseCapability, PromiseResolvers, RealmIntrinsics,
};
use crate::{
    Completion, EcmascriptHost, ExecutionContext, HostHooks, JsEngine, JsTypes, JsTypesWithRealm,
    Numeric, PreferredType, PropertyDescriptor,
};

use super::gc::V8PlatformData;
use super::trace::take_roots_reached_during_trace;
use super::types::{CachedPrimitive, ObjectProfile, V8ArrayBufferState, V8Handle};
use super::{
    V8ArrayBuffer, V8BigInt, V8Constructor, V8DataView, V8Function, V8Generator, V8Map, V8Object,
    V8Promise, V8PropertyKey, V8Realm, V8Set, V8SharedArrayBuffer, V8String, V8Symbol,
    V8TypedArray, V8Types, V8Value,
};

const HOST_OBJECT_TAG: u16 = 1;
static HOST_OBJECT_MARKER: u8 = 0;
static NEXT_ISOLATE_ID: AtomicU64 = AtomicU64::new(1);

/// Private-symbol key under which a native function holds the API-object
/// holder for its cppgc capture payload. `v8::Private` keys are invisible to
/// JavaScript.
const CAPTURE_HOLDER_PRIVATE_KEY: &str = "formal-web#callback-captures";

/// The registry of native-function weak handles is compacted opportunistically
/// once it reaches this many entries (see `make_builtin_function`).
const CALLBACK_HANDLE_COMPACTION_THRESHOLD: usize = 64;

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

/// The ECMA-262 wrapper-object kinds whose constructor the engine may be
/// asked to invoke with a primitive argument.
enum WrapperKind {
    Boolean,
    Number,
    String,
}

struct CallbackRecord {
    isolate_id: u64,
    creation_realm: RcWeak<V8RealmState>,
    // `None` after the record's creation realm has been dropped: the
    // behaviour closure may capture strong JS handles that would keep the
    // dead realm's objects (and thus the realm itself) alive, so teardown
    // clears it to release those captures. The record memory stays valid so
    // a stale `External` data pointer is never dereferenced after a free.
    behaviour: RefCell<Option<StoredBehaviour>>,
}

/// A registered native-function weak handle paired with the callback record
/// it keeps alive.
///
/// `weak` must be retained for the function's lifetime: dropping a
/// `v8::Weak` cancels its guaranteed finalizer, which would leak the record.
/// `record` is the shared ownership slot: the guaranteed finalizer and the
/// compaction pass each take the boxed record when the function dies, so the
/// record is freed exactly once by whichever owner gets there first (and at
/// isolate teardown by the slot itself when both owners drop).
struct CallbackHandle {
    weak: v8::Weak<v8::Function>,
    record: Rc<RefCell<Option<Box<CallbackRecord>>>>,
}

/// A platform object associated with an existing JS object (the realm
/// global), kept alive and traced through a cppgc [`Member`] edge so its
/// cells and JS edges participate in unified-heap collection. The raw
/// pointer mirrors the `host_data` internal-field pointer of
/// [`V8Engine::create_object_with_any`]: cppgc is non-moving and the Member
/// keeps the platform alive for as long as this record is traced.
struct AssociatedPlatform {
    object: V8Object,
    member: v8::cppgc::Member<V8PlatformData>,
    platform_pointer: *mut V8PlatformData,
}

#[derive(Default)]
struct RealmHostData {
    values: HashMap<TypeId, Box<dyn Any>>,
    associated_objects: Vec<AssociatedPlatform>,
}

// SAFETY: The trace visits every cppgc edge the host data holds: each
// associated platform, whose trace walks its cells and JS edges. The
// associated JS objects themselves are context-rooted realm globals, so they
// need no cppgc edge. Host `values` hold strong roots (`store_host_any` never
// converts them), so they are over-retained rather than traced.
unsafe impl Trace for RealmHostData {
    unsafe fn trace(&self, visitor: &mut crate::v8_gc::Visitor) {
        for associated in &self.associated_objects {
            // SAFETY: Delegated to the cppgc Member trace, which visits the
            // platform object and through it the platform's cells.
            visitor.trace(&associated.member);
        }
    }

    fn store(&mut self, _ec: &mut dyn ExecutionContext<V8Types>) {
        // The associated objects are the realm global (context-rooted); the
        // platform Members are traced by this type, so no root-to-edge
        // conversion is needed here.
    }
}

#[derive(Clone)]
struct V8RealmState {
    realm: V8Realm,
    realm_global: RefCell<V8Object>,
    // RefCell so realm initialization can populate the fields through a
    // shared `Rc` (the engine's current realm and the realm being
    // initialized may both hold strong references while intrinsics load).
    intrinsics: RefCell<Option<RealmIntrinsics<V8Types>>>,
    host_data_holder: RefCell<Option<V8Object>>,
    // Realm-bound builtin functions captured at realm creation, when the
    // global is pristine. The generic abstract operations that need JS
    // (`Object.getPrototypeOf`, `Reflect.ownKeys`, `Object.defineProperty`,
    // ...) invoke these captured functions so page code overwriting the
    // realm's globals cannot change their ECMA-262 semantics.
    captured: RefCell<Option<CapturedIntrinsics>>,
}

/// Realm-bound builtin functions captured at realm creation, used by the
/// generic abstract operations instead of the realm's (possibly patched)
/// globals.
#[derive(Clone)]
struct CapturedIntrinsics {
    get_prototype_of: V8Function,
    own_keys: V8Function,
    set_prototype_of: V8Function,
    is_extensible: V8Function,
    is_sealed: V8Function,
    is_frozen: V8Function,
    define_property: V8Function,
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
    callback_handles: RefCell<Vec<CallbackHandle>>,
    isolate: RefCell<v8::OwnedIsolate>,
    microtask_queue: v8::UniqueRef<v8::MicrotaskQueue>,
    /// Shared security token installed on every context created in this
    /// isolate.  V8 gates cross-context access to another context's global
    /// object on security-token equality; each `Context::new` otherwise gets
    /// its own token, so same-origin same-process window access (the
    /// WindowProxy same-origin path) would throw `TypeError: no access`.
    security_token: RefCell<Option<v8::Global<v8::Value>>>,
}

impl SharedIsolate {
    fn new() -> Rc<Self> {
        initialize_v8();
        // The cppgc heap is created with atomic marking and sweeping: the
        // trace callbacks run stop-the-world on the isolate thread (so Rust
        // never mutates a cell concurrently with the marker) and the
        // destructors of the traced values (which drop V8 handles) also run
        // on the isolate thread.
        let platform = V8_PLATFORM
            .get()
            .expect("V8 platform must be initialized before the isolate")
            .clone();
        let heap = v8::cppgc::Heap::create(
            platform,
            v8::cppgc::HeapCreateParams {
                marking_support: v8::cppgc::MarkingType::Atomic,
                sweeping_support: v8::cppgc::SweepingType::Atomic,
            },
        );
        let mut isolate = v8::Isolate::new(v8::CreateParams::default().cpp_heap(heap));
        isolate.set_microtasks_policy(v8::MicrotasksPolicy::Explicit);
        let microtask_queue = v8::MicrotaskQueue::new(&mut isolate, v8::MicrotasksPolicy::Explicit);
        let shared = Rc::new(Self {
            isolate_id: NEXT_ISOLATE_ID.fetch_add(1, Ordering::Relaxed),
            realm_states: RefCell::new(Vec::new()),
            queued_jobs: RefCell::new(VecDeque::new()),
            callback_handles: RefCell::new(Vec::new()),
            isolate: RefCell::new(isolate),
            microtask_queue,
            security_token: RefCell::new(None),
        });
        // Create the shared security token once, inside a scope on the
        // isolate.  The same Local is installed on every context below.
        {
            let mut isolate_for_scope = shared.isolate.borrow_mut();
            v8::scope!(let scope, &mut *isolate_for_scope);
            let token: v8::Local<v8::Value> =
                v8::String::new(scope, "formal-web-isolate-security-token")
                    .expect("create security token string")
                    .into();
            *shared.security_token.borrow_mut() = Some(v8::Global::<v8::Value>::new(scope, token));
        }
        shared
    }

    /// Install the isolate's shared security token on `context`.
    fn install_security_token<'s, 'i>(
        &self,
        scope: &mut v8::PinScope<'s, 'i, ()>,
        context: &v8::Context,
    ) {
        let token = self
            .security_token
            .borrow()
            .clone()
            .expect("security token must be initialized in SharedIsolate::new");
        let token = v8::Local::new(scope, &token);
        context.set_security_token(token);
    }

    fn borrow(&self, expected_isolate_id: u64) -> RefMut<'_, v8::OwnedIsolate> {
        assert_eq!(
            self.isolate_id, expected_isolate_id,
            "V8 engine and shared isolate identities differ"
        );
        self.isolate.borrow_mut()
    }
}

impl V8Engine {
    /// Run a closure with the isolate borrowed mutably, mirroring the scope
    /// macros: inside a native callback the isolate is reborrowed from the
    /// pinned callback scope, otherwise the shared isolate RefCell is used.
    pub(crate) fn with_isolate_mut<R>(&self, f: impl FnOnce(&mut v8::Isolate) -> R) -> R {
        let callback_scope_pointer = CURRENT_CALLBACK_SCOPE.get();
        if callback_scope_pointer.is_null() {
            let mut isolate_for_operation = self.shared_isolate.borrow(self.isolate_id);
            f(&mut isolate_for_operation)
        } else {
            assert_eq!(
                CURRENT_CALLBACK_ISOLATE_ID.get(),
                self.isolate_id,
                "reentrant V8 scope belongs to another isolate"
            );
            // SAFETY: The pointer and lifetime invariants are established by
            // `CurrentCallbackScopeGuard` and checked above.
            let callback_scope = unsafe { &mut *callback_scope_pointer };
            let isolate = &mut ***callback_scope;
            f(isolate)
        }
    }

    /// Run a closure with the isolate's cppgc heap.
    pub(crate) fn with_cpp_heap<R>(&self, f: impl FnOnce(&v8::cppgc::Heap) -> R) -> R {
        self.with_isolate_mut(|isolate| {
            let heap = isolate
                .get_cpp_heap()
                .expect("V8 isolate has no cppgc heap");
            f(heap)
        })
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

impl V8Engine {
    /// Run a closure with a scope over the realm context, for operations that
    /// need V8 locals (edge conversion, handle materialization).
    pub(crate) fn with_value_scope<R>(
        &mut self,
        f: impl FnOnce(&mut v8::PinScope<'_, '_>) -> R,
    ) -> R {
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, { f(scope) })
    }
}

struct CurrentEngineGuard {
    previous: *mut V8Engine,
}

/// Removes a platform address from the engine's `mutably_borrowed_platforms`
/// set when dropped, including on panic during the operation.
struct PlatformBorrowGuard {
    set: *mut RefCell<HashSet<usize>>,
    address: usize,
}

impl Drop for PlatformBorrowGuard {
    fn drop(&mut self) {
        // SAFETY: The guard is created and dropped inside
        // `with_object_any_mut_with`, which owns `&mut self` for the whole
        // guard lifetime, so the engine outlives the guard.
        unsafe {
            (*self.set).borrow_mut().remove(&self.address);
        }
    }
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
    // Addresses of `V8PlatformData` instances currently handed out as
    // `&mut dyn Any` through `with_object_any_mut_with`. `with_object_any`
    // and `with_object_any_mut` panic on a re-entrant access to a platform
    // that is already mutably borrowed, turning a would-be aliasing
    // violation (two live references to the same platform data) into a loud
    // bug report.
    mutably_borrowed_platforms: RefCell<HashSet<usize>>,
    // Strong references to realm states created through `create_realm`: the
    // shared-isolate registry holds weak refs (pruned when the last owner
    // drops), and the engine keeps the created realms alive so
    // `realm_intrinsics`/`set_realm_global_object` can find them.
    created_realm_states: RefCell<Vec<Rc<V8RealmState>>>,
}

static V8_PLATFORM: std::sync::OnceLock<v8::SharedRef<v8::Platform>> = std::sync::OnceLock::new();

fn initialize_v8() {
    static INITIALIZE: Once = Once::new();
    INITIALIZE.call_once(|| {
        v8::V8::set_flags_from_string("--expose-gc");
        let platform = v8::new_default_platform(0, false).make_shared();
        v8::V8::initialize_platform(platform.clone());
        // cppgc needs the platform page allocator before any heap is created.
        v8::cppgc::initialize_process(platform.clone());
        let _ = V8_PLATFORM.set(platform);
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
    // `V8PlatformData` pointer in field 1 with this exact tag. The cppgc
    // platform object is traced from the JS wrapper, so it stays alive for at
    // least as long as the JS object.
    let pointer = unsafe { object.get_aligned_pointer_from_internal_field(1, HOST_OBJECT_TAG) }
        as *mut c_void;
    NonNull::new(pointer)
}

fn root_handle<T>(scope: &mut v8::PinScope<'_, '_>, handle: v8::Local<'_, T>) -> V8Handle<T> {
    V8Handle::Root(v8::Global::new(scope, handle))
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
    } else if value.is_symbol() {
        CachedPrimitive::Symbol
    } else {
        CachedPrimitive::Other
    };

    let mut host_data = None;
    let object_profile = if value.is_object() {
        let object = v8::Local::<v8::Object>::try_from(value).expect("V8 object type check failed");
        host_data = host_data_pointer(scope, object);
        let array_buffer_handle = v8::Local::<v8::ArrayBuffer>::try_from(value)
            .ok()
            .map(|handle| root_handle(scope, handle));
        let shared_array_buffer_handle = v8::Local::<v8::SharedArrayBuffer>::try_from(value)
            .ok()
            .map(|handle| root_handle(scope, handle));
        let typed_array_handle = v8::Local::<v8::TypedArray>::try_from(value)
            .ok()
            .map(|handle| root_handle(scope, handle));
        let data_view_handle = v8::Local::<v8::DataView>::try_from(value)
            .ok()
            .map(|handle| root_handle(scope, handle));
        let promise_handle = v8::Local::<v8::Promise>::try_from(value)
            .ok()
            .map(|handle| root_handle(scope, handle));
        let function_handle = v8::Local::<v8::Function>::try_from(value)
            .ok()
            .map(|handle| root_handle(scope, handle));
        let map_handle = v8::Local::<v8::Map>::try_from(value)
            .ok()
            .map(|handle| root_handle(scope, handle));
        let set_handle = v8::Local::<v8::Set>::try_from(value)
            .ok()
            .map(|handle| root_handle(scope, handle));
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
        // ECMA-262 IsConstructor (§7.2.4) is approximated here by an own
        // `prototype` property with generator/async functions excluded. It is
        // exact for ordinary functions and for arrows, methods, and async
        // functions (no own `prototype`), but it is wrong for bound functions
        // (constructible when the target is, with no own `prototype`) and for
        // arrows given a manual `prototype` assignment. `rusty_v8` 150.1.0
        // binds only `StackFrame::IsConstructor`; the exact
        // `v8::Object::IsConstructor` needs an upstream binding. The
        // `has_own_property` probe can also run a Proxy trap at wrap time.
        // `make_builtin_function` overwrites the cached bit for native
        // functions, where the constructor behavior is known exactly.
        let is_constructor = if value.is_function()
            && !value.is_generator_function()
            && !value.is_async_function()
        {
            let prototype_name = v8::String::new(scope, "prototype")
                .expect("static V8 property name allocation failed");
            object
                .has_own_property(scope, prototype_name.into())
                .unwrap_or(false)
        } else {
            false
        };
        // Wrapper objects created by JavaScript (`new Number(42)`) unbox
        // through the native NumberValue fast path; the profile caches the
        // wrapped [[NumberData]]. Boolean, String, and BigInt wrappers have
        // no native unboxing API, so their data is only available through the
        // `construct` path (see `ExecutionContext::construct`).
        let wrapper_primitive = if value.is_number_object() {
            value.number_value(scope).map(CachedPrimitive::Number)
        } else {
            None
        };
        Some(Box::new(ObjectProfile {
            object_handle: root_handle(scope, object),
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
            is_constructor,
            wrapper_primitive,
            array_buffer_state,
            typed_array_element_type,
        }))
    } else {
        None
    };

    V8Value {
        isolate_id,
        handle: root_handle(scope, value),
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
    value.handle.to_local(scope).ok_or_else(|| {
        let message = v8::String::new(scope, "value has been reclaimed by the garbage collector")
            .expect("static V8 error string allocation failed");
        let exception = v8::Exception::type_error(scope, message);
        wrap_local_value(scope, isolate_id, exception)
    })
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
    object.1.to_local(scope).ok_or_else(|| {
        let message = v8::String::new(scope, "value has been reclaimed by the garbage collector")
            .expect("static V8 error string allocation failed");
        let exception = v8::Exception::type_error(scope, message);
        wrap_local_value(scope, isolate_id, exception)
    })
}

fn local_typed_object<'scope, T>(
    scope: &mut v8::PinScope<'scope, '_>,
    isolate_id: u64,
    object: &V8Object,
    handle: &V8Handle<T>,
) -> Result<v8::Local<'scope, T>, V8Value> {
    if object.0.isolate_id != isolate_id {
        let message = v8::String::new(scope, "value belongs to a different V8 isolate")
            .expect("static V8 error string allocation failed");
        let exception = v8::Exception::type_error(scope, message);
        return Err(wrap_local_value(scope, isolate_id, exception));
    }
    handle.to_local(scope).ok_or_else(|| {
        let message = v8::String::new(scope, "value has been reclaimed by the garbage collector")
            .expect("static V8 error string allocation failed");
        let exception = v8::Exception::type_error(scope, message);
        wrap_local_value(scope, isolate_id, exception)
    })
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

    // SAFETY: Callback records are created by `make_builtin_function` and
    // released once the function dies (by the guaranteed weak finalizer, by
    // the registry compaction, or at isolate teardown); V8 invokes this
    // callback only while the function is strongly reachable, so the record
    // is still in its slot. CURRENT_ENGINE is installed around every
    // operation that can execute JavaScript and is restricted to the isolate
    // thread. The isolate id check below prevents a record from being used
    // by another isolate. `catch_unwind` prevents Rust unwinding from
    // crossing V8's callback boundary.
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
                // For construct calls, V8's `this` is the newly created
                // receiver; the Web IDL constructor logic expects `this` to be
                // `new.target` (matching Boa's [[Construct]] convention), so
                // pass `new_target` instead.
                let this_value = if arguments.is_construct_call() {
                    wrap_local_value(scope, record.isolate_id, arguments.new_target())
                } else {
                    wrap_local_value(scope, record.isolate_id, arguments.this().into())
                };
                match catch_unwind(AssertUnwindSafe(|| {
                    let behaviour = record.behaviour.borrow();
                    let Some(behaviour) = behaviour.as_ref() else {
                        return Err(engine.new_type_error(
                            "native callback behaviour released after its realm was destroyed",
                        ));
                    };
                    (behaviour)(&callback_arguments, this_value, engine)
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
                let exception = exception
                    .handle
                    .to_local(scope)
                    .expect("callback exception handle must be valid");
                scope.throw_exception(exception);
            }
        },
        Err(exception) => {
            if exception.isolate_id == callback_isolate_id {
                let exception = exception
                    .handle
                    .to_local(scope)
                    .expect("callback exception handle must be valid");
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

fn resolve_module_import<'scope>(
    context: v8::Local<'scope, v8::Context>,
    specifier: v8::Local<'scope, v8::String>,
    import_attributes: v8::Local<'scope, v8::FixedArray>,
    _referrer: v8::Local<'scope, v8::Module>,
) -> Option<v8::Local<'scope, v8::Module>> {
    // SAFETY: V8 invokes module-resolution callbacks with the entered context
    // and no Rust handle scope. rusty_v8 requires CallbackScope only at this
    // callback boundary; the scope is pinned for its full use below.
    v8::callback_scope!(unsafe scope, context);

    let engine_pointer = CURRENT_ENGINE.get();
    if engine_pointer.is_null() {
        let message = v8::String::new(scope, "module resolution entered without an active engine")
            .expect("static module error allocation failed");
        let exception = v8::Exception::type_error(scope, message);
        scope.throw_exception(exception);
        return None;
    }

    // SAFETY: CURRENT_ENGINE is installed around every operation that can
    // execute JavaScript and is restricted to the isolate thread. The module
    // resolver runs synchronously inside `evaluate_module`, which already
    // holds the isolate scope; the callback-scope guard lets engine methods
    // below reborrow the pinned callback scope instead of the (already
    // borrowed) shared isolate.
    unsafe {
        let engine = &mut *engine_pointer;
        if engine.host_hooks.load_imported_module.is_none() {
            let message =
                v8::String::new(scope, "module imports are not enabled for the V8 backend")
                    .expect("static module error allocation failed");
            let exception = v8::Exception::type_error(scope, message);
            scope.throw_exception(exception);
            return None;
        }

        let _current_callback_scope = CurrentCallbackScopeGuard::enter(scope, engine.isolate_id);

        let specifier_string = V8String {
            value: None,
            utf16: cache_string(scope, &specifier),
        };
        let mut attributes = Vec::new();
        for index in 0..import_attributes.length() / 2 {
            if let (Some(key), Some(value)) = (
                import_attributes.get(scope, index * 2),
                import_attributes.get(scope, index * 2 + 1),
            ) && let Ok(key) = v8::Local::<v8::String>::try_from(key)
                && let Ok(value) = v8::Local::<v8::Value>::try_from(value)
            {
                attributes.push((
                    V8String {
                        value: None,
                        utf16: cache_string(scope, &key),
                    },
                    wrap_local_value(scope, engine.isolate_id, value),
                ));
            }
        }
        let module_request = ModuleRequest {
            specifier: specifier_string,
            attributes,
        };
        let realm = engine.realm_state.realm.clone();
        let constructor = engine.realm_intrinsics(&realm).promise;
        let capability = match catch_unwind(AssertUnwindSafe(|| {
            engine.new_promise_capability(constructor)
        })) {
            Ok(Ok(capability)) => capability,
            Ok(Err(error)) => {
                error!(
                    "module resolver failed to create the load capability: {}",
                    error.display()
                );
                let message = v8::String::new(scope, "module load capability creation failed")
                    .expect("static module error allocation failed");
                let exception = v8::Exception::type_error(scope, message);
                scope.throw_exception(exception);
                return None;
            }
            Err(_panic) => {
                let message = v8::String::new(scope, "module load capability creation panicked")
                    .expect("static module error allocation failed");
                let exception = v8::Exception::type_error(scope, message);
                scope.throw_exception(exception);
                return None;
            }
        };
        if let Some(hook) = &engine.host_hooks.load_imported_module {
            // The hook is embedder-supplied Rust code invoked from inside a
            // callback V8's module resolver calls synchronously; like the
            // capability creation above and `native_callback`, it must not
            // unwind across the V8 C++ boundary, so a panic is converted into
            // a thrown TypeError.
            let result = catch_unwind(AssertUnwindSafe(|| {
                hook(module_request, capability);
            }));
            if result.is_err() {
                error!("load_imported_module hook panicked during module resolution");
                let message = v8::String::new(scope, "module import hook panicked")
                    .expect("static module error allocation failed");
                let exception = v8::Exception::type_error(scope, message);
                scope.throw_exception(exception);
                return None;
            }
        }
        // V8's module resolver is synchronous, while host module loading
        // resolves its promise capability asynchronously, so no Module can be
        // produced here; instantiation fails after the hook has been invoked.
        let message = v8::String::new(
            scope,
            "host module loading is asynchronous; the V8 backend cannot instantiate imported modules",
        )
        .expect("static module error allocation failed");
        let exception = v8::Exception::type_error(scope, message);
        scope.throw_exception(exception);
        None
    }
}

impl V8Engine {
    pub fn new() -> Self {
        Self::new_with_shared_isolate(SharedIsolate::new())
    }

    /// Drop callback records whose creation realm has been dropped.
    ///
    /// This is defensive housekeeping, not a correctness mechanism: a
    /// capture-holding record keeps only a raw pointer into a cppgc payload
    /// attached to its function, so it no longer roots the realm (see
    /// `create_builtin_fn_with_captures`). Pruning still releases the record
    /// and its behaviour eagerly instead of waiting for the function to be
    /// collected with the dead realm; the `debug!` count lets a teardown
    /// investigation tell whether it is still removing anything meaningful.
    pub fn prune_dead_realm_callbacks(&self) {
        let mut pruned = 0usize;
        let mut callback_handles = self.shared_isolate.callback_handles.borrow_mut();
        callback_handles.retain(|handle| {
            let mut record_slot = handle.record.borrow_mut();
            let Some(record) = record_slot.as_mut() else {
                return false;
            };
            if record.creation_realm.strong_count() == 0 {
                // The function's creation realm is gone: no queued job, timer,
                // or event can invoke it anymore.
                if let Some(behaviour) = record.behaviour.get_mut().take() {
                    drop(behaviour);
                }
                pruned += 1;
                false
            } else {
                true
            }
        });
        if pruned > 0 {
            log::debug!("pruned {pruned} native callback record(s) for dropped realms");
        }
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
            // Same-origin windows in one agent cluster must be able to access
            // each other's globals (the WindowProxy same-origin path); share
            // the isolate-wide security token across every context.
            shared_isolate.install_security_token(scope, &context);
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
            intrinsics: RefCell::new(None),
            host_data_holder: RefCell::new(None),
            captured: RefCell::new(None),
        });
        let mut engine = Self {
            isolate_id,
            realm_state,
            host_hooks: HostHooks::empty(),
            shared_isolate,
            mutably_borrowed_platforms: RefCell::new(HashSet::new()),
            created_realm_states: RefCell::new(Vec::new()),
        };
        engine.initialize_realm_state(Rc::clone(&engine.realm_state));
        engine
    }

    /// Load the intrinsics, create the host-data holder, and register the
    /// realm state with the shared isolate. The engine's current realm is
    /// temporarily switched to `realm_state` so intrinsics are sourced from
    /// the realm being initialized.
    fn initialize_realm_state(&mut self, realm_state: Rc<V8RealmState>) {
        let previous_realm = replace(&mut self.realm_state, Rc::clone(&realm_state));
        let intrinsics = self.load_intrinsics();
        realm_state.intrinsics.replace(Some(intrinsics.clone()));
        let captured = CapturedIntrinsics {
            get_prototype_of: self.intrinsic_function("Object.getPrototypeOf"),
            own_keys: self.intrinsic_function("Reflect.ownKeys"),
            set_prototype_of: self.intrinsic_function("Reflect.setPrototypeOf"),
            is_extensible: self.intrinsic_function("Object.isExtensible"),
            is_sealed: self.intrinsic_function("Object.isSealed"),
            is_frozen: self.intrinsic_function("Object.isFrozen"),
            define_property: self.intrinsic_function("Object.defineProperty"),
        };
        realm_state.captured.replace(Some(captured));
        let host_data_holder = self.create_object_with_any(
            intrinsics.object_prototype,
            // The holder traces the associated platform objects (and through
            // them their cells and JS edges) while the realm lives.
            Box::new(V8PlatformData::new(RealmHostData::default())),
        );
        realm_state.host_data_holder.replace(Some(host_data_holder));
        self.realm_state = previous_realm;
        self.shared_isolate
            .realm_states
            .borrow_mut()
            .push(Rc::downgrade(&realm_state));
    }

    pub fn associate_existing_object(&mut self, object: &V8Object, data: Box<dyn Any>) {
        // The platform lives on the cppgc heap, traced from the realm host
        // data: `associate_existing_object` is used for the realm global
        // (whose JS wrapper cannot carry an `Object::wrap` platform link), so
        // the cppgc Member here is the platform's tracing owner.
        let platform = match V8PlatformData::try_recover(data) {
            Ok(platform) => platform,
            Err(raw) => V8PlatformData::noop(raw),
        };
        let (member, platform_pointer) = self.with_cpp_heap(|heap| {
            // SAFETY: The returned `UnsafePtr` is immediately moved into the
            // `Member` edge below and into the raw access pointer — the
            // required destination for a stack-created pointer. cppgc is
            // non-moving, so the raw pointer stays valid while the Member
            // keeps the platform alive.
            let pointer = unsafe { v8::cppgc::make_garbage_collected(heap, platform) };
            let member = v8::cppgc::Member::new(&pointer);
            let platform_pointer =
                unsafe { pointer.as_ref() } as *const V8PlatformData as *mut V8PlatformData;
            (member, platform_pointer)
        });
        self.realm_host_data_mut()
            .associated_objects
            .push(AssociatedPlatform {
                object: object.clone(),
                member,
                platform_pointer,
            });
    }

    pub fn new_child_realm(&self) -> Self {
        Self::new_with_shared_isolate(Rc::clone(&self.shared_isolate))
    }

    fn realm_host_data(&self) -> &RealmHostData {
        let holder = self
            .realm_state
            .host_data_holder
            .borrow()
            .as_ref()
            .expect("V8 realm host data is not initialized")
            .clone();
        self.with_object_any(&holder)
            .and_then(|data| data.downcast_ref::<RealmHostData>())
            .expect("V8 realm host-data holder contains an unexpected value")
    }

    fn realm_host_data_mut(&mut self) -> &mut RealmHostData {
        let holder = self
            .realm_state
            .host_data_holder
            .borrow()
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

    fn intrinsic_function(&mut self, source: &str) -> V8Function {
        let object = self.intrinsic_object(source);
        V8Types::object_as_function(&object)
            .unwrap_or_else(|| panic!("V8 intrinsic `{source}` is not a function"))
    }

    /// Clone one of the realm-bound captured builtins used by the generic
    /// abstract operations.
    fn captured_intrinsic(
        &self,
        select: impl FnOnce(&CapturedIntrinsics) -> V8Function,
    ) -> V8Function {
        self.realm_state
            .captured
            .borrow()
            .as_ref()
            .map(select)
            .expect("V8 captured intrinsics are not initialized")
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
                "Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}).prototype)",
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
            behaviour: RefCell::new(Some(behaviour)),
        });
        // The record lives in a shared slot owned by both the guaranteed
        // finalizer and the callback-handle registry entry; whichever drops
        // the function first frees it. The raw pointer handed to V8's
        // `External` data is derived from the box inside the slot and stays
        // valid while the function is alive (the slot is only taken once the
        // function has been collected, at which point V8 can no longer invoke
        // the callback).
        let record_slot = Rc::new(RefCell::new(Some(record)));
        let record_pointer = record_slot
            .borrow()
            .as_ref()
            .expect("callback record slot is empty before the function exists")
            .as_ref() as *const CallbackRecord;
        let isolate_id = self.isolate_id;
        let function_name = self.property_key_to_rust_string(&name);

        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let external = v8::External::new(scope, record_pointer.cast_mut().cast());
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
            let mut function_value = wrap_local_value(scope, isolate_id, function.into());
            // The profile's constructor bit is computed from the `prototype`
            // property heuristic at wrap time; for native functions the
            // constructor behavior is known exactly, so it overwrites the
            // heuristic.
            if let Some(profile) = function_value.object_profile.as_mut() {
                profile.is_constructor = is_constructor;
            }

            // The weak handle is retained by the engine; the finalizer owns a
            // share of the callback record and releases it before isolate
            // destruction, even if no collection happens first.
            let finalizer_slot = Rc::clone(&record_slot);
            let callback_handle = v8::Weak::with_guaranteed_finalizer(
                scope,
                function,
                Box::new(move || {
                    if let Some(record) = finalizer_slot.borrow_mut().take() {
                        drop(record);
                    }
                }),
            );
            let mut callback_handles = self.shared_isolate.callback_handles.borrow_mut();
            // Opportunistic compaction: stale entries (functions already
            // collected) are removed so the registry does not grow for the
            // lifetime of the isolate. The record is taken from the slot
            // first — the guaranteed finalizer may not have run yet, and
            // dropping the weak handle below would cancel it, so the slot
            // hand-off is what prevents a leaked or double-freed record.
            if callback_handles.len() >= CALLBACK_HANDLE_COMPACTION_THRESHOLD {
                callback_handles.retain(|handle| {
                    if handle.weak.is_empty() {
                        if let Some(record) = handle.record.borrow_mut().take() {
                            drop(record);
                        }
                        false
                    } else {
                        true
                    }
                });
            }
            callback_handles.push(CallbackHandle {
                weak: callback_handle,
                record: record_slot,
            });
            let object = object_from_wrapped_value(function_value);
            V8Types::object_as_function(&object)
                .expect("V8 native function wrapper is not a function")
        })
    }

    /// Attach a capture holder to a native function as a private property, so
    /// the function's traced reference keeps the cppgc capture payload alive.
    ///
    /// A `v8::Function` is not an API wrapper, so `v8::Object::wrap` on the
    /// function itself stores the pointer without V8 ever marking it (the
    /// payload would be swept while the function is alive). The holder is an
    /// API object created by `create_object_with_any`; marking the function
    /// reaches the private property, the holder, and through the wrapper link
    /// the payload and its edges.
    fn attach_capture_holder(&mut self, function: &V8Function, holder: &V8Object) {
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let function_local = local_typed_object(scope, isolate_id, &function.0, &function.1)
                .expect("capture holder target is not a live function");
            let holder_local = local_object(scope, isolate_id, holder)
                .expect("capture holder is not a live object");
            let key_name = v8::String::new(scope, CAPTURE_HOLDER_PRIVATE_KEY)
                .expect("capture holder private-key string allocation failed");
            let key = v8::Private::for_api(scope, Some(key_name));
            function_local
                .set_private(scope, key, holder_local.into())
                .expect("attaching the native callback capture holder threw");
        });
    }

    /// Create a built-in function from a type-erased closure.
    ///
    /// This is deliberately **not** a `JsEngine` trait method: a closure's
    /// captures cannot be walked by cppgc, so generic domain code must use
    /// `create_builtin_fn_static` or `create_builtin_fn_with_captures`. It
    /// remains an inherent method for the engine's own tests and the JSC
    /// backend, whose closures capture only non-JS values.
    pub fn create_builtin_fn(
        &mut self,
        behaviour: StoredBehaviour,
        length: u32,
        name: V8PropertyKey,
    ) -> V8Function {
        self.make_builtin_function(behaviour, length, name, false)
    }

    /// Constructable variant of [`Self::create_builtin_fn`].
    pub fn create_builtin_function(
        &mut self,
        behaviour: StoredBehaviour,
        length: u32,
        name: V8PropertyKey,
        is_constructor: bool,
    ) -> V8Function {
        self.make_builtin_function(behaviour, length, name, is_constructor)
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
        let (context_handle, realm_global) =
            v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
                let microtask_queue = from_ref(&*self.shared_isolate.microtask_queue).cast_mut();
                let context = v8::Context::new(
                    scope,
                    v8::ContextOptions {
                        microtask_queue: Some(microtask_queue),
                        ..v8::ContextOptions::default()
                    },
                );
                // Same-origin windows in one agent cluster must be able to
                // access each other's globals (the WindowProxy same-origin
                // path); share the isolate-wide security token.
                self.shared_isolate.install_security_token(scope, &context);
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
            realm: realm.clone(),
            realm_global: RefCell::new(realm_global),
            intrinsics: RefCell::new(None),
            host_data_holder: RefCell::new(None),
            captured: RefCell::new(None),
        });
        // A realm created through the generic API gets its own realm state,
        // intrinsics, and host-data holder, so `realm_intrinsics` and the
        // host-data methods source from the new realm rather than falling
        // back to the caller's current realm. The engine retains a strong
        // reference so the state survives while the realm handle is usable.
        self.initialize_realm_state(Rc::clone(&realm_state));
        self.created_realm_states.borrow_mut().push(realm_state);
        realm
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
        // V8 context globals are fixed at `Context::new`, so the installed
        // object cannot replace the context's global proxy; it is recorded in
        // the realm state so `realm_global_object` reports it while the realm
        // is current.
        if let Some(state) = self.state_for_realm(realm) {
            state.realm_global.replace(global);
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
            // V8 requires the ScriptOrigin of a compiled module to carry the
            // module flag.
            let resource_name = v8::String::new(try_catch, "module")
                .expect("static module resource name allocation failed");
            let origin = v8::ScriptOrigin::new(
                try_catch,
                resource_name.into(),
                0,
                0,
                false,
                -1,
                None,
                false,
                false,
                true,
                None,
            );
            let mut source = v8::script_compiler::Source::new(source_string, Some(&origin));
            let Some(module) = v8::script_compiler::compile_module(try_catch, &mut source) else {
                return Err(caught!(try_catch, isolate_id, "module compilation failed"));
            };
            if module.instantiate_module(try_catch, resolve_module_import) != Some(true) {
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
        constructor: V8Constructor,
        byte_length: u64,
    ) -> Completion<V8SharedArrayBuffer, V8Types> {
        // AllocateSharedArrayBuffer (§25.2.1.1) constructs through the
        // supplied constructor, so subclass and cross-realm constructors are
        // honored.
        let length = self.value_from_number(byte_length as f64);
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            v8::tc_scope!(let try_catch, scope);
            let constructor =
                local_typed_object(try_catch, isolate_id, &constructor.0, &constructor.1)?;
            let length = local_value(try_catch, isolate_id, &length)?;
            let Some(object) = constructor.new_instance(try_catch, &[length]) else {
                return Err(caught!(
                    try_catch,
                    isolate_id,
                    "SharedArrayBuffer allocation failed"
                ));
            };
            let object =
                object_from_wrapped_value(wrap_local_value(try_catch, isolate_id, object.into()));
            V8Types::object_as_shared_array_buffer(&object)
                .ok_or_else(|| self.new_type_error("SharedArrayBuffer allocation failed"))
        })
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

    // The generic parameter T is the active backend's JsTypes; in a
    // V8-selected build it is always V8Types. The layout assertions below
    // turn a mismatched instantiation (e.g. a mock type with a different
    // PropertyKey/Function layout) into an immediate panic instead of the
    // byte copies silently corrupting the stack.
    assert_eq!(
        std::mem::size_of::<T::PropertyKey>(),
        std::mem::size_of::<V8PropertyKey>(),
        "create_builtin_fn_with_captures instantiated with a non-V8 PropertyKey layout"
    );
    assert_eq!(
        std::mem::size_of::<T::Function>(),
        std::mem::size_of::<V8Function>(),
        "create_builtin_fn_with_captures instantiated with a non-V8 Function layout"
    );

    // SAFETY: This function is exported only by the V8-selected content
    // build, where T is V8Types (asserted above). Function pointers have
    // identical pointer representation; the callback trampoline validates
    // the active isolate before invoking the converted behaviour.
    let behaviour: CaptureBehaviour<V8Types, C> = unsafe { std::mem::transmute_copy(&behaviour) };

    // SAFETY: In a V8-selected build T::PropertyKey is V8PropertyKey
    // (asserted above). Moving through MaybeUninit preserves ownership
    // without creating a second drop.
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

    // Move the captures into a cppgc-traced platform object attached to the
    // function itself: rooted handles are converted to edges, the platform is
    // wrapped in an API object (the holder), and the holder is attached to the
    // function as a private property. The callback record therefore holds no
    // strong root — only a raw pointer into the platform — so the realm cannot
    // be pinned through the record and the captures die with the function.
    let mut captures = captures;
    Trace::store(&mut captures, engine);
    let intrinsics = engine.realm_intrinsics(&engine.current_realm());
    let captures_holder = engine.create_object_with_any(
        intrinsics.object_prototype,
        Box::new(V8PlatformData::new(captures)),
    );
    // SAFETY: cppgc is non-moving and the holder is traced from the function
    // created below, so the payload address is stable for as long as the
    // function is callable.
    let captures_ptr = engine
        .with_object_any(&captures_holder)
        .and_then(|data| data.downcast_ref::<C>())
        .expect("captures platform data type mismatch") as *const C;

    let stored = Box::new(
        move |arguments: &[V8Value],
              this_value,
              execution_context: &mut dyn ExecutionContext<V8Types>| {
            // The captures live in a cppgc-traced platform object attached to
            // the function as a private property: marking reaches them through
            // the function, so they stay alive exactly while the function is
            // reachable and are released when it dies.
            // SAFETY: the holder keeps the platform alive for the function's
            // lifetime and cppgc never moves it; V8 invokes this callback only
            // while the function is alive.
            let captures = unsafe { &*captures_ptr };
            behaviour(arguments, this_value, captures, execution_context)
        },
    );
    let result = engine.make_builtin_function(stored, length, name, is_constructor);
    engine.attach_capture_holder(&result, &captures_holder);

    // SAFETY: In a V8-selected build T::Function is V8Function (asserted
    // above). The result is moved into its associated-type spelling without
    // duplicating ownership.
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

/// Captured state for one promise reaction handler: the user handler and the
/// result capability's resolve/reject pair.
struct PromiseReactionCaptures {
    on_fulfilled: Option<V8Function>,
    on_rejected: Option<V8Function>,
    fulfilled_capability: Option<(V8Function, V8Function)>,
    rejected_capability: Option<(V8Function, V8Function)>,
}

// SAFETY: visits each captured function edge exactly once.
unsafe impl Trace for PromiseReactionCaptures {
    unsafe fn trace(&self, visitor: &mut crate::v8_gc::Visitor) {
        if let Some(on_fulfilled) = &self.on_fulfilled {
            // SAFETY: Delegated to the field's own trace.
            unsafe { Trace::trace(on_fulfilled, visitor) }
        }
        if let Some(on_rejected) = &self.on_rejected {
            // SAFETY: Delegated to the field's own trace.
            unsafe { Trace::trace(on_rejected, visitor) }
        }
        if let Some((resolve, reject)) = &self.fulfilled_capability {
            // SAFETY: Delegated to the fields' own traces.
            unsafe {
                Trace::trace(resolve, visitor);
                Trace::trace(reject, visitor);
            }
        }
        if let Some((resolve, reject)) = &self.rejected_capability {
            // SAFETY: Delegated to the fields' own traces.
            unsafe {
                Trace::trace(resolve, visitor);
                Trace::trace(reject, visitor);
            }
        }
    }

    fn store(&mut self, ec: &mut dyn ExecutionContext<V8Types>) {
        if let Some(on_fulfilled) = &mut self.on_fulfilled {
            on_fulfilled.store(ec);
        }
        if let Some(on_rejected) = &mut self.on_rejected {
            on_rejected.store(ec);
        }
        if let Some((resolve, reject)) = &mut self.fulfilled_capability {
            resolve.store(ec);
            reject.store(ec);
        }
        if let Some((resolve, reject)) = &mut self.rejected_capability {
            resolve.store(ec);
            reject.store(ec);
        }
    }
}

fn promise_reaction_fulfilled_behaviour(
    arguments: &[V8Value],
    _this: V8Value,
    captures: &PromiseReactionCaptures,
    execution_context: &mut dyn ExecutionContext<V8Types>,
) -> Completion<V8Value, V8Types> {
    let value = arguments
        .first()
        .cloned()
        .unwrap_or_else(|| execution_context.value_undefined());
    let completion = if let Some(on_fulfilled) = &captures.on_fulfilled {
        let undefined = execution_context.value_undefined();
        execution_context.call(&on_fulfilled.0, &undefined, &[value])
    } else {
        Ok(value)
    };
    let Some((resolve, reject)) = &captures.fulfilled_capability else {
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
}

fn promise_reaction_rejected_behaviour(
    arguments: &[V8Value],
    _this: V8Value,
    captures: &PromiseReactionCaptures,
    execution_context: &mut dyn ExecutionContext<V8Types>,
) -> Completion<V8Value, V8Types> {
    let reason = arguments
        .first()
        .cloned()
        .unwrap_or_else(|| execution_context.value_undefined());
    let completion = if let Some(on_rejected) = &captures.on_rejected {
        let undefined = execution_context.value_undefined();
        execution_context.call(&on_rejected.0, &undefined, &[reason])
    } else {
        Err(reason)
    };
    let Some((resolve, reject)) = &captures.rejected_capability else {
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
        // Discard roots seen by allocation-triggered traces: this collection
        // traces the whole heap again and reports only what it finds.
        take_roots_reached_during_trace();
        let shared_isolate = Rc::clone(&self.shared_isolate);
        v8_shared_isolate!(isolate, shared_isolate, self.isolate_id, {
            isolate.request_garbage_collection_for_testing(v8::GarbageCollectionType::Full);
        });
        // Sweep the cppgc heap as well: platform objects and cells are
        // cppgc-managed, and the isolate test hook collects the JS heap only.
        self.with_cpp_heap(|heap| unsafe {
            heap.collect_garbage_for_testing(v8::cppgc::EmbedderStackState::NoHeapPointers);
        });
        let roots_reached = take_roots_reached_during_trace();
        if roots_reached > 0 {
            error!(
                "{roots_reached} rooted JS handle(s) reached cppgc tracing: the store \
                 invariant was bypassed and the referents are over-retained"
            );
        }
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

    fn as_any(&self) -> &dyn Any {
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
            CachedPrimitive::Symbol | CachedPrimitive::Other => true,
        }
    }

    fn to_number(&mut self, value: V8Value) -> Completion<f64, V8Types> {
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            v8::tc_scope!(let try_catch, scope);
            let local = local_value(try_catch, isolate_id, &value)?;
            let Some(number) = local.to_number(try_catch) else {
                return Err(caught!(try_catch, isolate_id, "ToNumber failed"));
            };
            Ok(number.value())
        })
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
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            v8::tc_scope!(let try_catch, scope);
            let local = local_value(try_catch, isolate_id, &value)?;
            let Some(object) = local.to_object(try_catch) else {
                return Err(caught!(try_catch, isolate_id, "ToObject failed"));
            };
            Ok(object_from_wrapped_value(wrap_local_value(
                try_catch,
                isolate_id,
                object.into(),
            )))
        })
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
        if value.isolate_id != self.isolate_id {
            return Err(self.new_type_error("value belongs to a different V8 isolate"));
        }
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            let local = local_value(scope, isolate_id, value)?;
            // The native `IsArray` check models ECMA-262 §7.2.2 directly;
            // it does not consult the realm's (possibly patched) Array.isArray.
            Ok(local.is_array())
        })
    }

    fn is_constructor(&self, value: &V8Value) -> bool {
        value
            .object_profile
            .as_ref()
            .is_some_and(|profile| profile.is_constructor)
    }

    fn is_extensible(&mut self, object: &V8Object) -> Completion<bool, V8Types> {
        let function = self.captured_intrinsic(|captured| captured.is_extensible.clone());
        let undefined = self.value_undefined();
        let result = EcmascriptHost::call(
            self,
            &function.0,
            &undefined,
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
            (
                CachedPrimitive::Other | CachedPrimitive::Symbol,
                CachedPrimitive::Other | CachedPrimitive::Symbol,
            ) => {
                // Root handles compare scope-free; edges (or mixed modes)
                // compare through locals in a scope.
                if left.handle.same_identity(&right.handle) {
                    return true;
                }
                let isolate_id = self.isolate_id;
                v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
                    match (
                        local_value(scope, isolate_id, left),
                        local_value(scope, isolate_id, right),
                    ) {
                        (Ok(left_local), Ok(right_local)) => left_local.same_value(right_local),
                        _ => false,
                    }
                })
            }
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
        // Build a JS descriptor object containing only the fields the caller
        // supplied. ECMA-262 DefinePropertyOrThrow treats absent descriptor
        // fields as "leave the existing attribute unchanged", so a
        // `value: None`/`writable: None`/missing accessor must not become
        // `undefined`/`false` in the descriptor. The definition itself runs
        // through the private helper context, whose pristine
        // `Object.defineProperty` cannot be patched by page code.
        let descriptor_object = self.create_plain_object(None);
        if let Some(enumerable) = descriptor.enumerable {
            let key = self.property_key_from_str("enumerable");
            let value = self.value_from_bool(enumerable);
            self.create_data_property(descriptor_object.clone(), key, value)?;
        }
        if let Some(configurable) = descriptor.configurable {
            let key = self.property_key_from_str("configurable");
            let value = self.value_from_bool(configurable);
            self.create_data_property(descriptor_object.clone(), key, value)?;
        }
        if let Some(value) = &descriptor.value {
            let key = self.property_key_from_str("value");
            self.create_data_property(descriptor_object.clone(), key, value.clone())?;
        }
        if let Some(writable) = descriptor.writable {
            let key = self.property_key_from_str("writable");
            let value = self.value_from_bool(writable);
            self.create_data_property(descriptor_object.clone(), key, value)?;
        }
        if let Some(getter) = &descriptor.get {
            let key = self.property_key_from_str("get");
            let value = V8Types::value_from_object(getter.0.clone());
            self.create_data_property(descriptor_object.clone(), key, value)?;
        }
        if let Some(setter) = &descriptor.set {
            let key = self.property_key_from_str("set");
            let value = V8Types::value_from_object(setter.0.clone());
            self.create_data_property(descriptor_object.clone(), key, value)?;
        }
        let key = self.value_from_property_key(property_key);
        let function = self.captured_intrinsic(|captured| captured.define_property.clone());
        let undefined = self.value_undefined();
        EcmascriptHost::call(
            self,
            &function.0,
            &undefined,
            &[object.0, key, descriptor_object.0],
        )?;
        Ok(())
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
        let function = self.captured_intrinsic(|captured| captured.get_prototype_of.clone());
        let undefined = self.value_undefined();
        let prototype = EcmascriptHost::call(self, &function.0, &undefined, &[object.0])?;
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
        let function = self.captured_intrinsic(|captured| captured.set_prototype_of.clone());
        let undefined = self.value_undefined();
        let prototype = prototype.map_or_else(|| self.value_null(), |prototype| prototype.0);
        let result = EcmascriptHost::call(self, &function.0, &undefined, &[object.0, prototype])?;
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
        let function = self.captured_intrinsic(|captured| captured.own_keys.clone());
        let undefined = self.value_undefined();
        let array = EcmascriptHost::call(self, &function.0, &undefined, &[object.0])?;
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

        let wrapper_kind = self
            .realm_state
            .intrinsics
            .borrow()
            .as_ref()
            .and_then(|intrinsics| {
                if function.0 == intrinsics.boolean.0 {
                    Some(WrapperKind::Boolean)
                } else if function.0 == intrinsics.number.0 {
                    Some(WrapperKind::Number)
                } else if function.0 == intrinsics.string.0 {
                    Some(WrapperKind::String)
                } else {
                    None
                }
            });
        // The wrapper's [[BooleanData]] has no native unboxing API, so the
        // cached primitive is derived from the coerced argument; Number and
        // String wrapper data are also extracted from the wrapper itself at
        // wrap time, so these entries only supplement that path. (BigInt has
        // no [[Construct]], so `new BigInt(...)` always throws and no cache
        // entry is needed.)
        let wrapper_primitive = match wrapper_kind {
            Some(kind) => match arguments.first().cloned() {
                Some(argument) => match kind {
                    WrapperKind::Boolean => {
                        Some(CachedPrimitive::Boolean(self.to_boolean(&argument)))
                    }
                    WrapperKind::Number => {
                        self.to_number(argument).ok().map(CachedPrimitive::Number)
                    }
                    WrapperKind::String => self
                        .to_js_string(argument)
                        .ok()
                        .map(|string| CachedPrimitive::String(string.utf16)),
                },
                None => match kind {
                    WrapperKind::Boolean => Some(CachedPrimitive::Boolean(false)),
                    WrapperKind::Number => Some(CachedPrimitive::Number(f64::NAN)),
                    WrapperKind::String => Some(CachedPrimitive::String(Arc::from([]))),
                },
            },
            None => None,
        };
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
        let function = self.captured_intrinsic(|captured| match level {
            IntegrityLevel::Sealed => captured.is_sealed.clone(),
            IntegrityLevel::Frozen => captured.is_frozen.clone(),
        });
        let undefined = self.value_undefined();
        let result = EcmascriptHost::call(self, &function.0, &undefined, &[object.0])?;
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
            .borrow()
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

    fn array_buffer_byte_length(&self, array_buffer: &V8ArrayBuffer) -> u64 {
        let Some(state) = array_buffer
            .0
            .0
            .object_profile
            .as_ref()
            .and_then(|profile| profile.array_buffer_state.as_ref())
        else {
            return 0;
        };
        if state.detached.get() {
            return 0;
        }
        let isolate_id = self.isolate_id;
        let live_detached =
            v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
                let local =
                    match local_typed_object(scope, isolate_id, &array_buffer.0, &array_buffer.1) {
                        Ok(local) => local,
                        Err(_) => return 0,
                    };
                local.was_detached()
            });
        if live_detached {
            return 0;
        }
        state.backing_store.byte_length() as u64
    }

    fn can_transfer_array_buffer(&self, array_buffer: &V8ArrayBuffer) -> bool {
        let state = array_buffer
            .0
            .0
            .object_profile
            .as_ref()
            .and_then(|profile| profile.array_buffer_state.as_ref());
        if state.is_some_and(|state| state.detached.get()) {
            return false;
        }
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            match local_typed_object(scope, isolate_id, &array_buffer.0, &array_buffer.1) {
                Ok(local) => local.is_detachable(),
                Err(_) => false,
            }
        })
    }

    fn allocate_array_buffer(
        &mut self,
        constructor: V8Constructor,
        byte_length: u64,
        max_byte_length: Option<u64>,
    ) -> Completion<V8ArrayBuffer, V8Types> {
        // AllocateArrayBuffer (§25.1.2.1) constructs through the supplied
        // constructor (OrdinaryCreateFromConstructor), so subclass and
        // cross-realm constructors are honored; the V8 [[Construct]] call
        // performs the CreateByteDataBlock and slot initialization.
        let length = self.value_from_number(byte_length as f64);
        let arguments = if let Some(max_byte_length) = max_byte_length {
            let options = self.create_plain_object(None);
            let key = self.property_key_from_str("maxByteLength");
            let value = self.value_from_number(max_byte_length as f64);
            self.create_data_property(options.clone(), key, value)?;
            vec![length, V8Types::value_from_object(options)]
        } else {
            vec![length]
        };
        let isolate_id = self.isolate_id;
        v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
            v8::tc_scope!(let try_catch, scope);
            let constructor =
                local_typed_object(try_catch, isolate_id, &constructor.0, &constructor.1)?;
            let local_arguments: Result<Vec<_>, _> = arguments
                .iter()
                .map(|argument| local_value(try_catch, isolate_id, argument))
                .collect();
            let Some(object) = constructor.new_instance(try_catch, &local_arguments?) else {
                return Err(caught!(
                    try_catch,
                    isolate_id,
                    "ArrayBuffer allocation failed"
                ));
            };
            let object =
                object_from_wrapped_value(wrap_local_value(try_catch, isolate_id, object.into()));
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
        // A JavaScript-initiated `ArrayBuffer.prototype.transfer()` detaches the
        // buffer without updating the cached `state.detached` cell, so check the
        // live V8 buffer state as well.
        let isolate_id = self.isolate_id;
        let live_detached =
            v8_engine_scope_with_context!(scope, self, &self.realm_state.realm.context, {
                let local =
                    local_typed_object(scope, isolate_id, &array_buffer.0, &array_buffer.1).ok()?;
                local.was_detached()
            });
        if live_detached {
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

        let fulfilled_handler: V8Function =
            create_builtin_fn_with_captures::<V8Types, PromiseReactionCaptures>(
                self,
                PromiseReactionCaptures {
                    on_fulfilled,
                    on_rejected: None,
                    fulfilled_capability,
                    rejected_capability: None,
                },
                promise_reaction_fulfilled_behaviour,
                1,
                empty_name.clone(),
                false,
            );
        let rejected_handler: V8Function =
            create_builtin_fn_with_captures::<V8Types, PromiseReactionCaptures>(
                self,
                PromiseReactionCaptures {
                    on_fulfilled: None,
                    on_rejected,
                    fulfilled_capability: None,
                    rejected_capability,
                },
                promise_reaction_rejected_behaviour,
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
        // The platform object lives on the cppgc heap, wrapped in a
        // type-erased [`V8PlatformData`] that traces through the concrete
        // type. `create_interface_instance` wraps the platform object in
        // advance; other callers (prototypes, namespace objects) have no
        // edges and get a no-op trace.
        let platform = match V8PlatformData::try_recover(data) {
            Ok(platform) => platform,
            Err(raw) => V8PlatformData::noop(raw),
        };
        let platform_ptr = self.with_cpp_heap(|heap| {
            // SAFETY: The returned `UnsafePtr` is immediately stored into the
            // JS wrapper's cpp heap wrappable slot by `wrap` below — the
            // required destination for a stack-created pointer.
            unsafe { v8::cppgc::make_garbage_collected(heap, platform) }
        });
        // cppgc is non-moving; the platform data address is stable for the
        // lifetime of the wrapper, so it can be cached in the internal field.
        let platform_data_pointer = unsafe { platform_ptr.as_ref() } as *const V8PlatformData;
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

            let marker = v8::External::new(
                scope,
                std::ptr::addr_of!(HOST_OBJECT_MARKER).cast_mut().cast(),
            );
            assert!(object.set_internal_field(0, marker.into()));
            object.set_aligned_pointer_in_internal_field(
                1,
                platform_data_pointer.cast(),
                HOST_OBJECT_TAG,
            );

            // Register the cppgc platform object with the wrapper so the
            // unified heap traces it (and through it its cells and JS edges)
            // while the wrapper is alive.
            let isolate = &mut ****scope;
            // SAFETY: `wrap` stores the wrappable pointer in the wrapper's cpp
            // heap slot; the tag is unique within this embedder. The isolate
            // is reborrowed from the active scope, matching the scope-macro
            // reborrow pattern.
            unsafe {
                v8::Object::wrap::<HOST_OBJECT_TAG, V8PlatformData>(isolate, object, &platform_ptr)
            }

            object_from_wrapped_value(wrap_local_value(scope, isolate_id, object.into()))
        })
    }

    fn with_object_any(&self, object: &V8Object) -> Option<&dyn Any> {
        let platform_address: Option<usize> = if let Some(pointer) = object.0.host_data {
            Some(pointer.as_ptr() as usize)
        } else {
            self.realm_host_data()
                .associated_objects
                .iter()
                .find(|associated| &associated.object == object)
                .map(|associated| associated.platform_pointer as usize)
        };
        let address = platform_address?;
        assert!(
            !self.mutably_borrowed_platforms.borrow().contains(&address),
            "re-entrant immutable platform access during a mutable platform borrow"
        );
        // SAFETY: `host_data_pointer` validates the marker and tag before
        // placing this pointer in a V8Value; the associated records keep
        // their platform alive through a traced cppgc Member. cppgc is
        // non-moving, so the address is stable while the owner lives.
        let platform = unsafe { &*(address as *const V8PlatformData) };
        Some(platform.as_any())
    }

    fn with_object_any_mut(&mut self, object: &V8Object) -> Option<&mut dyn Any> {
        let platform_address: Option<usize> = if let Some(pointer) = object.0.host_data {
            Some(pointer.as_ptr() as usize)
        } else {
            self.realm_host_data_mut()
                .associated_objects
                .iter_mut()
                .find(|associated| &associated.object == object)
                .map(|associated| associated.platform_pointer as usize)
        };
        let address = platform_address?;
        assert!(
            !self.mutably_borrowed_platforms.borrow().contains(&address),
            "re-entrant mutable platform access during a mutable platform borrow"
        );
        // SAFETY: The marker and reachability invariants are the same as in
        // `with_object_any`. The `&mut self` receiver makes this the
        // exclusive host-data access path for the duration of the returned
        // borrow; marking is atomic, so no concurrent marker reads the
        // platform data while it is mutated.
        let platform = unsafe { &mut *(address as *mut V8PlatformData) };
        Some(platform.as_any_mut())
    }

    fn with_object_any_mut_with(
        &mut self,
        object: &V8Object,
        operation: Box<dyn FnOnce(&mut dyn Any, &mut dyn ExecutionContext<V8Types>) + '_>,
    ) {
        let platform_address: Option<usize> = if let Some(pointer) = object.0.host_data {
            Some(pointer.as_ptr() as usize)
        } else {
            self.realm_host_data_mut()
                .associated_objects
                .iter_mut()
                .find(|associated| &associated.object == object)
                .map(|associated| associated.platform_pointer as usize)
        };
        let Some(address) = platform_address else {
            return;
        };
        // Register the platform as mutably borrowed for the duration of the
        // operation: a re-entrant `with_object_any`/`with_object_any_mut`
        // access to the same platform through the passed-in execution
        // context would otherwise create a second live reference to the
        // same data (an aliasing violation) — this turns it into a panic.
        assert!(
            self.mutably_borrowed_platforms.borrow_mut().insert(address),
            "re-entrant mutable platform access through the execution context"
        );
        let set_pointer = &mut self.mutably_borrowed_platforms as *mut RefCell<HashSet<usize>>;
        let _guard = PlatformBorrowGuard {
            set: set_pointer,
            address,
        };
        // SAFETY: The address was obtained from the validated host pointer
        // or the associated platform record, both kept alive by their
        // tracing owners; the guard prevents any second access to this
        // platform through the execution context for the duration of the
        // operation, so the `&mut dyn Any` is the only live reference.
        let data_pointer =
            unsafe { (&mut *(address as *mut V8PlatformData)).as_any_mut() } as *mut dyn Any;
        // SAFETY: `data_pointer` was obtained from storage exclusively
        // borrowed above and is used only for this call.
        unsafe {
            operation(&mut *data_pointer, self);
        }
    }

    fn store_js_object(&mut self, slot: &mut Option<V8Object>, value: V8Object) {
        *slot = Some(value);
        // Convert the stored object's rooted handles into cppgc edges so the
        // slot participates in unified-heap cycle collection.
        crate::gc::Trace::store(slot, self);
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
        is_constructor: bool,
    ) -> V8Function {
        self.make_builtin_function(Box::new(behaviour), length, name, is_constructor)
    }
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;
    use std::cell::{Cell, RefCell};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::ptr::from_ref;
    use std::rc::Rc;

    use rusty_v8 as v8;

    use crate::gc::{Finalize, GcCell, Trace, gc_cell_new};
    use crate::v8_gc::Visitor;
    use crate::{
        EcmascriptHost, ExecutionContext, HostHooks, JsEngine, JsTypes, PropertyDescriptor,
    };

    use super::super::types::V8Handle;
    use super::super::{V8AsyncGenerator, V8PlatformData, V8WeakMap, V8WeakRef, V8WeakSet};
    use super::{
        CALLBACK_HANDLE_COMPACTION_THRESHOLD, CURRENT_CALLBACK_ISOLATE_ID, CURRENT_CALLBACK_SCOPE,
        StoredBehaviour, V8ArrayBuffer, V8Constructor, V8DataView, V8Engine, V8Function,
        V8Generator, V8Map, V8Object, V8Promise, V8Set, V8SharedArrayBuffer, V8TypedArray, V8Types,
        create_builtin_fn_with_captures, local_object, local_typed_object,
    };

    pub(crate) struct DropFlag(pub(crate) Rc<Cell<bool>>);

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
            .expect("ArrayBuffer must retain a typed handle");
        assert!(
            !matches!(array_buffer.1, V8Handle::Edge(_)),
            "fresh values are rooted"
        );

        let shared_array_buffer = evaluate_object("new SharedArrayBuffer(8)");
        let shared_array_buffer = V8Types::object_as_shared_array_buffer(&shared_array_buffer)
            .expect("SharedArrayBuffer must retain a typed handle");
        assert!(
            !matches!(shared_array_buffer.1, V8Handle::Edge(_)),
            "fresh values are rooted"
        );

        let typed_array = evaluate_object("new Uint8Array(8)");
        let typed_array = V8Types::object_as_typed_array(&typed_array)
            .expect("TypedArray must retain a typed handle");
        assert!(
            !matches!(typed_array.1, V8Handle::Edge(_)),
            "fresh values are rooted"
        );

        let data_view = evaluate_object("new DataView(new ArrayBuffer(8))");
        let data_view =
            V8Types::object_as_data_view(&data_view).expect("DataView must retain a typed handle");
        assert!(
            !matches!(data_view.1, V8Handle::Edge(_)),
            "fresh values are rooted"
        );

        let promise = evaluate_object("Promise.resolve(1)");
        let promise =
            V8Types::object_as_promise(&promise).expect("Promise must retain a typed handle");
        assert!(
            !matches!(promise.1, V8Handle::Edge(_)),
            "fresh values are rooted"
        );

        let map = evaluate_object("new Map()");
        let map = V8Types::object_as_map(&map).expect("Map must retain a typed handle");
        assert!(
            !matches!(map.1, V8Handle::Edge(_)),
            "fresh values are rooted"
        );

        let set = evaluate_object("new Set()");
        let set = V8Types::object_as_set(&set).expect("Set must retain a typed handle");
        assert!(
            !matches!(set.1, V8Handle::Edge(_)),
            "fresh values are rooted"
        );

        let function = evaluate_object("(function typedFunction() {})");
        let function =
            V8Types::object_as_function(&function).expect("Function must retain a typed handle");
        assert!(
            !matches!(function.1, V8Handle::Edge(_)),
            "fresh values are rooted"
        );
    }

    /// The store path converts rooted handles into cppgc edges and then
    /// makes later stores cheap: `needs_store` is the guard that keeps a
    /// `borrow_mut` drop over an already-stored container from opening a V8
    /// value scope per element.
    #[test]
    fn store_converts_roots_then_skips_the_value_scope() {
        let mut engine = V8Engine::new();
        let mut value = ExecutionContext::evaluate_script(&mut engine, "({ marker: 'payload' })")
            .expect("the payload object must evaluate");
        assert!(value.needs_store(), "a fresh value holds rooted handles");
        Trace::store(&mut value, &mut engine);
        assert!(!value.needs_store(), "a stored value's handles are edges");

        let fresh_object = ExecutionContext::evaluate_script(&mut engine, "new ArrayBuffer(8)")
            .expect("the ArrayBuffer must evaluate");
        let fresh_object =
            V8Types::value_as_object(&fresh_object).expect("the value must be an object");
        let mut object = fresh_object;
        assert!(
            object.0.needs_store() && object.1.is_root(),
            "a fresh object holds two rooted handles"
        );
        Trace::store(&mut object, &mut engine);
        assert!(
            !object.0.needs_store() && !object.1.is_root(),
            "both object handles must become edges"
        );

        let fresh_array_buffer =
            ExecutionContext::evaluate_script(&mut engine, "new ArrayBuffer(8)")
                .expect("the ArrayBuffer must evaluate");
        let fresh_array_buffer =
            V8Types::value_as_object(&fresh_array_buffer).expect("the value must be an object");
        let mut array_buffer = V8Types::object_as_array_buffer(&fresh_array_buffer)
            .expect("ArrayBuffer must retain a typed handle");
        assert!(
            array_buffer.0.0.needs_store() && array_buffer.1.is_root(),
            "a fresh typed wrapper holds rooted handles"
        );
        Trace::store(&mut array_buffer, &mut engine);
        assert!(
            !array_buffer.0.0.needs_store() && !array_buffer.1.is_root(),
            "a typed wrapper's handles must become edges"
        );
    }

    /// Microbenchmark for the store-on-drop path: push a batch of fresh
    /// rooted values into a cell, then clear it. Each push re-walks the whole
    /// cell, so the `needs_store` guard is what keeps the already-stored
    /// elements from opening a V8 value scope each. Not a correctness test.
    #[test]
    #[ignore = "microbenchmark; run with --ignored --nocapture"]
    fn store_on_borrow_mut_drop_throughput() {
        let mut engine = V8Engine::new();
        let cell = gc_cell_new(Vec::new(), &mut engine);
        let value = engine.value_from_number(1.0);
        let batch = 1000usize;
        let batches = 100usize;
        let start = std::time::Instant::now();
        for _ in 0..batches {
            for _ in 0..batch {
                cell.borrow_mut(&mut engine).push(value.clone());
            }
            cell.borrow_mut(&mut engine).clear();
        }
        let elapsed = start.elapsed();
        let total = (batch * batches) as u32;
        println!(
            "store-on-drop: {total} pushes in {elapsed:?} ({:?}/push)",
            elapsed / total
        );
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

    /// A child realm whose wrapper is captured by one of its own native
    /// callbacks must be collectable once the Rust realm handle is dropped:
    /// the callback record holds only a raw pointer into a cppgc capture
    /// payload attached to the function (never a `Root`), so the realm reaches
    /// the callback through the function without the callback rooting the
    /// realm back.
    #[test]
    fn realm_wrapper_captured_by_a_native_callback_is_collected() {
        let mut parent_engine = V8Engine::new();
        let mut child_engine = parent_engine.new_child_realm();

        let captured = child_engine.create_plain_object(None);
        let callback_name = child_engine.property_key_from_str("capturedCallback");
        let callback: V8Function = create_builtin_fn_with_captures::<V8Types, Option<V8Object>>(
            &mut child_engine,
            Some(captured.clone()),
            |_arguments, _this, _captures, execution_context| {
                Ok(execution_context.value_undefined())
            },
            0,
            callback_name.clone(),
            false,
        );
        let child_global = child_engine.realm_global_object();
        let callback_value = V8Types::value_from_object(callback.0.clone());
        child_engine
            .create_data_property(child_global.clone(), callback_name, callback_value)
            .expect("installing the captured callback must succeed");

        let realm_collected = Rc::new(Cell::new(false));
        let realm_weak = install_guaranteed_finalizer(
            &mut child_engine,
            &child_global,
            Rc::clone(&realm_collected),
        );

        // Drop every Rust root into the child realm: the only remaining
        // reference to the global is the realm's own object graph.
        drop(callback);
        drop(captured);
        drop(child_global);
        drop(child_engine);

        parent_engine.gc();
        assert!(
            realm_collected.get(),
            "a realm whose wrapper is captured by its own native callback must be collectable"
        );
        drop(realm_weak);
    }

    #[test]
    fn realm_function_captured_by_a_native_callback_is_collected() {
        let mut parent_engine = V8Engine::new();
        let mut child_engine = parent_engine.new_child_realm();

        let user_function = {
            let user_value = ExecutionContext::evaluate_script(&mut child_engine, "(() => 1)")
                .expect("the child realm must evaluate the function");
            V8Types::value_as_object(&user_value)
                .and_then(|object| V8Types::object_as_function(&object))
                .expect("the evaluated value must be a function")
        };
        let callback_name = child_engine.property_key_from_str("capturedFunction");
        let callback: V8Function = create_builtin_fn_with_captures::<V8Types, Option<V8Function>>(
            &mut child_engine,
            Some(user_function.clone()),
            |_arguments, _this, _captures, execution_context| {
                Ok(execution_context.value_undefined())
            },
            0,
            callback_name.clone(),
            false,
        );
        let child_global = child_engine.realm_global_object();
        let callback_value = V8Types::value_from_object(callback.0.clone());
        child_engine
            .create_data_property(child_global.clone(), callback_name, callback_value)
            .expect("installing the captured callback must succeed");

        let realm_collected = Rc::new(Cell::new(false));
        let realm_weak = install_guaranteed_finalizer(
            &mut child_engine,
            &child_global,
            Rc::clone(&realm_collected),
        );

        drop(callback);
        drop(user_function);
        drop(child_global);
        drop(child_engine);

        parent_engine.gc();
        assert!(
            realm_collected.get(),
            "a realm function captured by its own native callback must not pin the realm"
        );
        drop(realm_weak);
    }

    /// Soak: repeatedly build a child realm whose native callbacks capture
    /// realm wrappers (a direct callback and a `.then()` reaction), drop the
    /// realm, and collect. V8 can keep the most recent realm's functions for
    /// one more collection cycle, so the invariant asserted is boundedness,
    /// not zero: the live-callback count must stay flat across many
    /// iterations and the realm-state list must collapse back to the parent.
    #[test]
    fn repeated_realm_teardown_does_not_leak_callbacks_or_realms() {
        let mut parent_engine = V8Engine::new();
        let iterations = 128;
        let baseline = parent_engine
            .shared_isolate
            .callback_handles
            .borrow()
            .iter()
            .filter(|handle| !handle.weak.is_empty())
            .count();
        let mut max_live_callbacks = baseline;
        for iteration in 0..iterations {
            let mut child_engine = parent_engine.new_child_realm();

            // A captured callback installed on the child global.
            let captured = child_engine.create_plain_object(None);
            let callback_name = child_engine.property_key_from_str("capturedCallback");
            let callback: V8Function = create_builtin_fn_with_captures::<V8Types, Option<V8Object>>(
                &mut child_engine,
                Some(captured.clone()),
                |_arguments, _this, _captures, execution_context| {
                    Ok(execution_context.value_undefined())
                },
                0,
                callback_name.clone(),
                false,
            );
            let child_global = child_engine.realm_global_object();
            let callback_value = V8Types::value_from_object(callback.0.clone());
            child_engine
                .create_data_property(child_global, callback_name, callback_value)
                .expect("installing the captured callback must succeed");

            // A `.then()` chain whose handlers also capture a realm wrapper.
            let promise_value =
                ExecutionContext::evaluate_script(&mut child_engine, "new Promise(() => {})")
                    .expect("the child realm must create a promise");
            let promise_object =
                V8Types::value_as_object(&promise_value).expect("the value must be an object");
            let promise =
                V8Types::object_as_promise(&promise_object).expect("the value must be a promise");
            let empty_name = child_engine.property_key_from_str("");
            let reaction_handler: V8Function =
                create_builtin_fn_with_captures::<V8Types, Option<V8Object>>(
                    &mut child_engine,
                    Some(captured.clone()),
                    |_arguments, _this, _captures, execution_context| {
                        Ok(execution_context.value_undefined())
                    },
                    1,
                    empty_name,
                    false,
                );
            let _ = child_engine
                .perform_promise_then(promise, Some(reaction_handler), None, None)
                .expect("registering the reaction must succeed");
            drop(promise_value);

            drop(captured);
            drop(child_engine);
            parent_engine.gc();

            let live_callbacks = parent_engine
                .shared_isolate
                .callback_handles
                .borrow()
                .iter()
                .filter(|handle| !handle.weak.is_empty())
                .count();
            max_live_callbacks = max_live_callbacks.max(live_callbacks);
            assert!(
                live_callbacks <= baseline + 8,
                "iteration {iteration}: live callback records grew to {live_callbacks}"
            );
        }

        parent_engine
            .shared_isolate
            .realm_states
            .borrow_mut()
            .retain(|state| state.strong_count() != 0);
        let live_realms = parent_engine.shared_isolate.realm_states.borrow().len();
        assert_eq!(
            live_realms, 1,
            "only the parent realm must remain alive after {iterations} teardowns"
        );
        assert!(
            max_live_callbacks <= baseline + 8,
            "callback records grew unboundedly (max {max_live_callbacks})"
        );
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

    /// A traced platform payload with a finalization probe and the three
    /// reference kinds production platform objects carry: a reflector edge
    /// back to the JS wrapper, a peer edge to another wrapper, and a nested
    /// cell holding JS references.
    struct TestPlatform {
        dropped: DropFlag,
        reflector: Option<V8Object>,
        peer: Option<V8Object>,
        cell: Option<GcCell<Option<V8Object>>>,
    }

    // SAFETY: Every edge the platform holds is visited exactly once: the
    // reflector, the peer, and the nested cell (whose trace walks the Member
    // to the heap cell). The finalization probe is not traced.
    unsafe impl Trace for TestPlatform {
        unsafe fn trace(&self, visitor: &mut Visitor) {
            if let Some(reflector) = &self.reflector {
                // SAFETY: Delegated to the field's own trace implementation.
                unsafe { Trace::trace(reflector, visitor) }
            }
            if let Some(peer) = &self.peer {
                // SAFETY: Delegated to the field's own trace implementation.
                unsafe { Trace::trace(peer, visitor) }
            }
            if let Some(cell) = &self.cell {
                // SAFETY: Delegated to the field's own trace implementation.
                unsafe { Trace::trace(cell, visitor) }
            }
        }

        fn store(&mut self, ec: &mut dyn ExecutionContext<V8Types>) {
            if let Some(reflector) = &mut self.reflector {
                Trace::store(reflector, ec);
            }
            if let Some(peer) = &mut self.peer {
                Trace::store(peer, ec);
            }
            if let Some(cell) = &mut self.cell {
                Trace::store(cell, ec);
            }
        }
    }

    impl Finalize for TestPlatform {}

    /// Install a guaranteed finalizer on `object`; the returned weak handle
    /// must stay alive for the finalizer to run.
    fn install_guaranteed_finalizer(
        engine: &mut V8Engine,
        object: &V8Object,
        flag: Rc<Cell<bool>>,
    ) -> v8::Weak<v8::Object> {
        let isolate_id = engine.isolate_id;
        v8_engine_scope_with_context!(scope, engine, &engine.realm_state.realm.context, {
            let local = local_object(scope, isolate_id, object)
                .expect("finalizer installation received a reclaimed or cross-isolate object");
            v8::Weak::with_guaranteed_finalizer(scope, local, Box::new(move || flag.set(true)))
        })
    }

    /// De-risking prototype for function-owned callback captures. A
    /// `v8::Function` created by the API is **not** an API wrapper
    /// (`is_api_wrapper()` is false), so `v8::Object::wrap` silently stores
    /// the cppgc pointer without establishing a traced edge: the payload is
    /// swept while the function is still alive (observed as a SIGBUS). The
    /// workable form is a private-symbol property on the function whose
    /// value is an API object that wraps the payload — V8 marks the private
    /// property like any other, reaches the API object, and cppgc traces the
    /// payload through the wrapper link.
    #[test]
    fn function_object_private_capture_holder_is_traced() {
        let mut engine = V8Engine::new();
        let dropped = Rc::new(Cell::new(false));
        let prototype = engine.create_plain_object(None);
        let holder = engine.create_object_with_any(
            prototype,
            Box::new(V8PlatformData::new(TestPlatform {
                dropped: DropFlag(Rc::clone(&dropped)),
                reflector: None,
                peer: None,
                cell: None,
            })),
        );
        let name = engine.property_key_from_str("captured");
        let function = engine.create_builtin_fn_static(
            |_arguments, _this, execution_context| Ok(execution_context.value_undefined()),
            0,
            name,
            false,
        );

        let isolate_id = engine.isolate_id;
        v8_engine_scope_with_context!(scope, &mut engine, &engine.realm_state.realm.context, {
            let function_local = local_typed_object(scope, isolate_id, &function.0, &function.1)
                .expect("the native function handle must materialize");
            assert!(
                !function_local.is_api_wrapper(),
                "an API function is not an API wrapper; the direct wrap link is not traced"
            );
            let holder_local = local_object(scope, isolate_id, &holder)
                .expect("the capture holder handle must materialize");
            let key_string = v8::String::new(scope, "formal-web#callback-captures")
                .expect("private-key string allocation failed");
            let key = v8::Private::for_api(scope, Some(key_string));
            function_local
                .set_private(scope, key, holder_local.into())
                .expect("setting the private capture holder must not throw");
        });

        assert!(!dropped.get(), "the payload is alive before any collection");
        // Drop the Rust root: only the function's private property references
        // the holder now, so a surviving payload proves the function traces it.
        drop(holder);
        engine.gc();
        assert!(
            !dropped.get(),
            "the function's private capture holder must be traced across a collection"
        );
        drop(function);
        engine.gc();
        assert!(
            dropped.get(),
            "the payload must be swept with the unreachable function"
        );
    }

    #[test]
    fn reflector_cycle_is_collected_by_forced_gc() {
        let mut engine = V8Engine::new();
        let platform_dropped = Rc::new(Cell::new(false));
        let prototype = engine.create_plain_object(None);
        let platform = V8PlatformData::new(TestPlatform {
            dropped: DropFlag(Rc::clone(&platform_dropped)),
            reflector: None,
            peer: None,
            cell: None,
        });
        let wrapper = engine.create_object_with_any(prototype, Box::new(platform));

        // The platform stores an edge back to its own wrapper: the wrapper
        // traces the platform (v8::Object::wrap) and the platform traces the
        // wrapper (reflector edge) — a cross-heap cycle.
        engine.with_object_any_mut_with(
            &wrapper,
            Box::new(|data, ec| {
                let platform = data
                    .downcast_mut::<TestPlatform>()
                    .expect("wrapper carries test platform data");
                assert!(
                    !platform.dropped.0.get(),
                    "the probe must not fire while the platform is alive"
                );
                ec.store_js_object(&mut platform.reflector, wrapper.clone());
            }),
        );

        let wrapper_collected = Rc::new(Cell::new(false));
        let wrapper_weak =
            install_guaranteed_finalizer(&mut engine, &wrapper, Rc::clone(&wrapper_collected));

        drop(wrapper);
        // One full collection: the unified heap marks the wrapper and its
        // platform together, so the cycle is reclaimed in a single pass.
        engine.gc();

        assert!(
            platform_dropped.get(),
            "the platform must be collected with its unreachable wrapper"
        );
        assert!(
            wrapper_collected.get(),
            "the wrapper must be collected with its reflector-holding platform"
        );
        drop(wrapper_weak);
    }

    #[test]
    fn mutual_platform_object_cycle_is_collected() {
        let mut engine = V8Engine::new();
        let a_dropped = Rc::new(Cell::new(false));
        let b_dropped = Rc::new(Cell::new(false));
        let prototype = engine.create_plain_object(None);
        let a = engine.create_object_with_any(
            prototype.clone(),
            Box::new(V8PlatformData::new(TestPlatform {
                dropped: DropFlag(Rc::clone(&a_dropped)),
                reflector: None,
                peer: None,
                cell: None,
            })),
        );
        let b = engine.create_object_with_any(
            prototype,
            Box::new(V8PlatformData::new(TestPlatform {
                dropped: DropFlag(Rc::clone(&b_dropped)),
                reflector: None,
                peer: None,
                cell: None,
            })),
        );

        engine.with_object_any_mut_with(
            &a,
            Box::new(|data, ec| {
                let platform = data
                    .downcast_mut::<TestPlatform>()
                    .expect("wrapper A carries test platform data");
                assert!(
                    !platform.dropped.0.get(),
                    "the probe must not fire while platform A is alive"
                );
                ec.store_js_object(&mut platform.peer, b.clone());
            }),
        );
        engine.with_object_any_mut_with(
            &b,
            Box::new(|data, ec| {
                let platform = data
                    .downcast_mut::<TestPlatform>()
                    .expect("wrapper B carries test platform data");
                assert!(
                    !platform.dropped.0.get(),
                    "the probe must not fire while platform B is alive"
                );
                ec.store_js_object(&mut platform.peer, a.clone());
            }),
        );

        let a_wrapper_collected = Rc::new(Cell::new(false));
        let b_wrapper_collected = Rc::new(Cell::new(false));
        let a_weak = install_guaranteed_finalizer(&mut engine, &a, Rc::clone(&a_wrapper_collected));
        let b_weak = install_guaranteed_finalizer(&mut engine, &b, Rc::clone(&b_wrapper_collected));

        drop(a);
        drop(b);
        // One full collection: the two wrappers and their platforms form a
        // single cross-heap cycle, reclaimed in one pass.
        engine.gc();

        assert!(
            a_dropped.get(),
            "platform A must be collected with the cycle"
        );
        assert!(
            b_dropped.get(),
            "platform B must be collected with the cycle"
        );
        assert!(
            a_wrapper_collected.get(),
            "wrapper A must be collected with the cycle"
        );
        assert!(
            b_wrapper_collected.get(),
            "wrapper B must be collected with the cycle"
        );
        drop(a_weak);
        drop(b_weak);
    }

    #[test]
    fn stored_js_reference_dies_with_its_cell() {
        let mut engine = V8Engine::new();

        // X is a plain object reachable only through the cell edge below.
        let x = ExecutionContext::evaluate_script(&mut engine, "({ marker: 'payload' })")
            .expect("the payload object must evaluate");
        let x_object = V8Types::value_as_object(&x).expect("the payload must be an object");
        let x_collected = Rc::new(Cell::new(false));
        let x_weak = install_guaranteed_finalizer(&mut engine, &x_object, Rc::clone(&x_collected));

        // gc_cell_new converts the rooted handle into a cppgc edge.
        let cell = gc_cell_new(Some(x_object), &mut engine);
        drop(x);

        // A platform object owns the cell so it is traced while the platform
        // lives (the production pattern for stream controller state).
        let platform_dropped = Rc::new(Cell::new(false));
        let prototype = engine.create_plain_object(None);
        let owner = engine.create_object_with_any(
            prototype,
            Box::new(V8PlatformData::new(TestPlatform {
                dropped: DropFlag(Rc::clone(&platform_dropped)),
                reflector: None,
                peer: None,
                cell: Some(cell),
            })),
        );
        assert!(
            engine
                .with_object_any(&owner)
                .and_then(|data| data.downcast_ref::<TestPlatform>())
                .is_some_and(|platform| !platform.dropped.0.get()),
            "the probe must not fire while the owner is alive"
        );

        drop(owner);
        // One full collection: the payload edge dies with the cell, and the
        // cell dies with its owning platform, all in the same pass.
        engine.gc();

        assert!(
            platform_dropped.get(),
            "the owning platform must be collected"
        );
        assert!(
            x_collected.get(),
            "the payload referenced only through the cell edge must be collected with it"
        );
        drop(x_weak);
    }

    #[test]
    fn platform_data_finalized_at_isolate_destruction() {
        let platform_dropped = Rc::new(Cell::new(false));
        {
            let mut engine = V8Engine::new();
            let prototype = engine.create_plain_object(None);
            let owner = engine.create_object_with_any(
                prototype,
                Box::new(V8PlatformData::new(TestPlatform {
                    dropped: DropFlag(Rc::clone(&platform_dropped)),
                    reflector: None,
                    peer: None,
                    cell: None,
                })),
            );
            // Keep the wrapper alive: destruction must finalize the platform
            // data even though no collection ran.
            assert!(
                engine
                    .with_object_any(&owner)
                    .and_then(|data| data.downcast_ref::<TestPlatform>())
                    .is_some_and(|platform| !platform.dropped.0.get()),
                "the platform must be alive before destruction"
            );
        }
        assert!(
            platform_dropped.get(),
            "platform data must be finalized when the isolate is destroyed"
        );
    }

    #[test]
    fn arrow_functions_are_not_constructors() {
        let mut engine = V8Engine::new();
        let arrow = ExecutionContext::evaluate_script(&mut engine, "(() => 1)")
            .expect("the arrow function must evaluate");
        assert!(
            !engine.is_constructor(&arrow),
            "arrow functions are callable but have no [[Construct]]"
        );
        let arrow_object = V8Types::value_as_object(&arrow).expect("the arrow must be an object");
        assert!(
            V8Types::object_as_constructor(&arrow_object).is_none(),
            "object_as_constructor must reject non-constructible functions"
        );

        let normal = ExecutionContext::evaluate_script(&mut engine, "(function normal() {})")
            .expect("the normal function must evaluate");
        assert!(engine.is_constructor(&normal));
        let normal_object =
            V8Types::value_as_object(&normal).expect("the function must be an object");
        assert!(
            V8Types::object_as_constructor(&normal_object).is_some(),
            "ordinary functions are constructors"
        );

        let generator = ExecutionContext::evaluate_script(&mut engine, "(function* gen() {})")
            .expect("the generator function must evaluate");
        assert!(
            !engine.is_constructor(&generator),
            "generator functions are callable but have no [[Construct]]"
        );
    }

    #[test]
    fn define_property_or_throw_preserves_absent_descriptor_fields() {
        let mut engine = V8Engine::new();
        let object = ExecutionContext::evaluate_script(
            &mut engine,
            "globalThis.__descriptorTarget = { value: 7 }; globalThis.__descriptorTarget",
        )
        .expect("the target object must evaluate");
        let object = V8Types::value_as_object(&object).expect("the target must be an object");
        let value_key = engine.property_key_from_str("value");
        let original = engine
            .get_own_property(object.clone(), value_key.clone())
            .expect("the own property must be readable")
            .expect("the property must exist");
        assert_eq!(original.writable, Some(true));

        // Define only `enumerable: false`: value and writable must be untouched.
        engine
            .define_property_or_throw(
                object.clone(),
                value_key.clone(),
                PropertyDescriptor {
                    value: None,
                    writable: None,
                    get: None,
                    set: None,
                    enumerable: Some(false),
                    configurable: None,
                },
            )
            .expect("defining only enumerable must succeed");
        let after = engine
            .get_own_property(object.clone(), value_key.clone())
            .expect("the property must remain readable")
            .expect("the property must still exist");
        assert_eq!(after.writable, Some(true), "writability must be preserved");
        let value = ExecutionContext::get(&mut engine, object.clone(), value_key)
            .expect("the value must be readable");
        assert_eq!(
            engine.to_number(value).expect("the value must be numeric"),
            7.0,
            "the property value must be preserved"
        );
    }

    #[test]
    fn accessor_update_with_only_a_setter_preserves_the_getter() {
        let mut engine = V8Engine::new();
        let object = ExecutionContext::evaluate_script(
            &mut engine,
            "globalThis.__accessorTarget = { get field() { return 'getter'; }, set field(v) {} }; globalThis.__accessorTarget",
        )
        .expect("the accessor object must evaluate");
        let object = V8Types::value_as_object(&object).expect("the target must be an object");
        let key = engine.property_key_from_str("field");
        let setter = ExecutionContext::evaluate_script(&mut engine, "(value => {})")
            .expect("the setter function must evaluate");
        let setter = V8Types::value_as_object(&setter)
            .and_then(|object| V8Types::object_as_function(&object))
            .expect("the setter must be a function");

        // Define only the setter: the existing getter must be preserved.
        engine
            .define_property_or_throw(
                object.clone(),
                key.clone(),
                PropertyDescriptor {
                    value: None,
                    writable: None,
                    get: None,
                    set: Some(setter),
                    enumerable: None,
                    configurable: None,
                },
            )
            .expect("defining only the setter must succeed");
        let after = engine
            .get_own_property(object, key)
            .expect("the property must remain readable")
            .expect("the property must still exist");
        assert!(
            after.get.is_some(),
            "the existing getter must survive a setter-only update"
        );
    }

    #[test]
    fn number_wrapper_data_extracts_script_created_wrappers() {
        let mut engine = V8Engine::new();
        for source in ["new Number(42)", "new Number('42')"] {
            let wrapper = ExecutionContext::evaluate_script(&mut engine, source)
                .expect("the wrapper expression must evaluate");
            let object = V8Types::value_as_object(&wrapper).expect("the wrapper must be an object");
            assert!(V8Types::object_is_number_wrapper(&object));
            assert_eq!(
                V8Types::number_wrapper_data(&object),
                Some(42.0),
                "the wrapper's [[NumberData]] must be extracted from `{source}`"
            );
        }
    }

    #[test]
    fn wrapper_constructors_coerce_their_arguments() {
        let mut engine = V8Engine::new();
        let intrinsics = engine.realm_intrinsics(&engine.current_realm());

        let string_arg = engine.value_from_string(engine.js_string_from_str("42"));
        let number_wrapper = engine
            .construct(intrinsics.number.clone(), &[string_arg], None)
            .expect("constructing Number must succeed");
        assert_eq!(
            V8Types::number_wrapper_data(&number_wrapper),
            Some(42.0),
            "new Number('42') must record the coerced number"
        );

        let number_arg = engine.value_from_number(42.0);
        let string_wrapper = engine
            .construct(intrinsics.string.clone(), &[number_arg], None)
            .expect("constructing String must succeed");
        assert_eq!(
            V8Types::string_wrapper_data(&string_wrapper)
                .map(|string| { String::from_utf16_lossy(&string.utf16) }),
            Some("42".to_owned()),
            "new String(42) must record the coerced string"
        );

        let number_arg = engine.value_from_number(3.0);
        let boolean_wrapper = engine
            .construct(intrinsics.boolean.clone(), &[number_arg], None)
            .expect("constructing Boolean must succeed");
        assert_eq!(
            V8Types::boolean_wrapper_data(&boolean_wrapper),
            Some(true),
            "new Boolean(3) must record the coerced boolean"
        );
    }

    #[test]
    fn is_array_and_to_number_ignore_patched_page_globals() {
        let mut engine = V8Engine::new();
        ExecutionContext::evaluate_script(
            &mut engine,
            "Array.isArray = () => false; Number = () => 999;",
        )
        .expect("the global patch must evaluate");
        let array = ExecutionContext::evaluate_script(&mut engine, "[1, 2, 3]")
            .expect("the array must evaluate");
        assert!(
            engine.is_array(&array).expect("is_array must not fail"),
            "IsArray must not consult the patched Array.isArray"
        );
        let string = engine.value_from_string(engine.js_string_from_str("42"));
        assert_eq!(
            engine.to_number(string).expect("ToNumber must not fail"),
            42.0,
            "ToNumber must not consult the patched Number global"
        );
    }

    #[test]
    fn helper_operations_ignore_patched_page_globals() {
        let mut engine = V8Engine::new();
        ExecutionContext::evaluate_script(
            &mut engine,
            "Object.getPrototypeOf = () => null; Reflect.ownKeys = () => [];",
        )
        .expect("the global patch must evaluate");
        let object = ExecutionContext::evaluate_script(&mut engine, "({ a: 1 })")
            .expect("the object must evaluate");
        let object = V8Types::value_as_object(&object).expect("the object must be an object");
        assert!(
            engine
                .get_prototype_of(object.clone())
                .expect("GetPrototypeOf must not fail")
                .is_some(),
            "GetPrototypeOf must not consult the patched Object.getPrototypeOf"
        );
        let keys = engine
            .own_property_keys(object)
            .expect("OwnPropertyKeys must not fail");
        assert_eq!(
            keys.len(),
            1,
            "OwnPropertyKeys must not consult the patched Reflect.ownKeys"
        );
    }

    #[test]
    fn create_realm_registers_its_own_intrinsics() {
        let mut engine = V8Engine::new();
        let realm = engine.create_realm();
        let intrinsics = engine.realm_intrinsics(&realm);
        // The intrinsics of the new realm are its own Object.prototype: the
        // realm's `Object.prototype` object.
        let value = JsEngine::evaluate_script(&mut engine, "Object.prototype", &realm)
            .expect("the realm script must evaluate");
        let proto = V8Types::value_as_object(&value).expect("the prototype must be an object");
        assert!(
            engine.same_value(
                &V8Types::value_from_object(intrinsics.object_prototype.clone()),
                &V8Types::value_from_object(proto),
            ),
            "realm_intrinsics must source from the created realm"
        );
        let caller_proto = engine
            .realm_intrinsics(&engine.current_realm())
            .object_prototype;
        assert!(
            !engine.same_value(
                &V8Types::value_from_object(intrinsics.object_prototype.clone()),
                &V8Types::value_from_object(caller_proto),
            ),
            "realm_intrinsics must not fall back to the caller's realm"
        );
    }

    #[test]
    fn allocate_array_buffer_honors_the_supplied_constructor() {
        let mut engine = V8Engine::new();
        let subclass =
            ExecutionContext::evaluate_script(&mut engine, "class Sub extends ArrayBuffer {}; Sub")
                .expect("the subclass must evaluate");
        let subclass = V8Types::value_as_object(&subclass)
            .and_then(|object| V8Types::object_as_constructor(&object))
            .expect("Sub must be a constructor");
        let buffer = ExecutionContext::allocate_array_buffer(&mut engine, subclass, 8, None)
            .expect("the allocation must succeed");
        let prototype = engine
            .get_prototype_of(buffer.0)
            .expect("the buffer prototype must be readable")
            .expect("the buffer must have a prototype");
        let sub_prototype = ExecutionContext::evaluate_script(&mut engine, "Sub.prototype")
            .expect("Sub.prototype must evaluate");
        let sub_prototype =
            V8Types::value_as_object(&sub_prototype).expect("Sub.prototype must be an object");
        assert!(
            engine.same_value(
                &V8Types::value_from_object(prototype),
                &V8Types::value_from_object(sub_prototype),
            ),
            "AllocateArrayBuffer must construct through the supplied subclass"
        );
    }

    #[test]
    fn module_import_invokes_the_load_imported_module_hook() {
        let mut engine = V8Engine::new();
        let hook_called = Rc::new(Cell::new(false));
        let hook_flag = Rc::clone(&hook_called);
        let mut hooks = HostHooks::empty();
        hooks.load_imported_module = Some(Box::new(move |_request, _capability| {
            hook_flag.set(true);
        }));
        engine.set_host_hooks(hooks);
        let realm = engine.current_realm();
        let result = JsEngine::evaluate_module(
            &mut engine,
            "import 'virtual-module'; export const answer = 42;",
            &realm,
        );
        assert!(
            result.is_err(),
            "synchronous instantiation of an imported module must fail"
        );
        assert!(
            hook_called.get(),
            "the load_imported_module hook must be invoked for the import"
        );
    }

    #[test]
    fn panicking_load_imported_module_hook_does_not_unwind() {
        let mut engine = V8Engine::new();
        let mut hooks = HostHooks::empty();
        hooks.load_imported_module = Some(Box::new(|_request, _capability| {
            panic!("load_imported_module hook panicked");
        }));
        engine.set_host_hooks(hooks);
        let realm = engine.current_realm();
        let result = JsEngine::evaluate_module(
            &mut engine,
            "import 'virtual-module'; export const answer = 42;",
            &realm,
        );
        assert!(
            result.is_err(),
            "a panicking import hook must surface as an evaluation error, not unwind across V8"
        );
    }

    #[test]
    fn callback_records_are_reclaimed_after_collection() {
        let mut engine = V8Engine::new();
        let record_dropped = Rc::new(Cell::new(false));
        let captured = DropFlag(Rc::clone(&record_dropped));
        let behaviour: StoredBehaviour = Box::new(move |_args, _this, ec| {
            let _ = &captured;
            Ok(ec.value_undefined())
        });
        let function = engine.make_builtin_function(
            behaviour,
            0,
            engine.property_key_from_str("test_fn"),
            false,
        );
        drop(function);
        // A full collection runs the guaranteed weak finalizer, which must
        // release the callback record (and with it the captured behaviour
        // closure).
        engine.gc();
        assert!(
            record_dropped.get(),
            "the callback record must be freed once its function is collected"
        );
    }

    #[test]
    fn callback_handles_compact_stale_entries() {
        let mut engine = V8Engine::new();
        let make_function = |engine: &mut V8Engine| {
            let behaviour: StoredBehaviour = Box::new(|_args, _this, ec| Ok(ec.value_undefined()));
            engine.make_builtin_function(
                behaviour,
                0,
                engine.property_key_from_str("test_fn"),
                false,
            )
        };
        // Cross the compaction threshold, then drop every function so each
        // registry entry goes stale.
        let functions: Vec<_> = (0..CALLBACK_HANDLE_COMPACTION_THRESHOLD + 8)
            .map(|_| make_function(&mut engine))
            .collect();
        assert_eq!(
            engine.shared_isolate.callback_handles.borrow().len(),
            CALLBACK_HANDLE_COMPACTION_THRESHOLD + 8,
            "every native function must be registered"
        );
        drop(functions);
        engine.gc();
        // The next registration compacts the stale entries out of the
        // registry instead of letting it grow for the isolate's lifetime.
        let _keep = make_function(&mut engine);
        let live_count = engine.shared_isolate.callback_handles.borrow().len();
        assert!(
            live_count <= CALLBACK_HANDLE_COMPACTION_THRESHOLD,
            "the registry must compact stale entries (len={live_count})"
        );
    }

    #[test]
    fn reentrant_platform_access_panics() {
        let mut engine = V8Engine::new();
        let prototype = engine.create_plain_object(None);
        let wrapper =
            engine.create_object_with_any(prototype, Box::new(DropFlag(Rc::new(Cell::new(false)))));
        let object = wrapper.clone();
        let result = catch_unwind(AssertUnwindSafe(|| {
            engine.with_object_any_mut_with(
                &wrapper,
                Box::new(move |_data, ec| {
                    let _ = ec.with_object_any(&object);
                }),
            );
        }));
        assert!(
            result.is_err(),
            "re-entrant platform access must panic instead of aliasing"
        );
    }

    #[test]
    fn associated_platform_cells_survive_forced_gc() {
        let mut engine = V8Engine::new();
        let global = engine.realm_global_object();

        // A payload object reachable only through the associated platform's
        // cell (the Window pattern: event listeners, timers, ...).
        let payload = ExecutionContext::evaluate_script(&mut engine, "({ marker: 'payload' })")
            .expect("the payload object must evaluate");
        let payload_object =
            V8Types::value_as_object(&payload).expect("the payload must be an object");
        let payload_collected = Rc::new(Cell::new(false));
        let payload_weak = install_guaranteed_finalizer(
            &mut engine,
            &payload_object,
            Rc::clone(&payload_collected),
        );

        let cell = gc_cell_new(Some(payload_object), &mut engine);
        drop(payload);

        // The content path associates the raw platform (e.g. the Window) with
        // the realm global through the generic helper, which wraps it with
        // its real trace on the cppgc heap.
        crate::associate_existing_object(
            &mut engine,
            &global,
            TestPlatform {
                dropped: DropFlag(Rc::new(Cell::new(false))),
                reflector: None,
                peer: None,
                cell: Some(cell),
            },
        );

        assert!(
            engine
                .with_object_any(&global)
                .and_then(|data| data.downcast_ref::<TestPlatform>())
                .and_then(|platform| platform.cell.as_ref())
                .is_some(),
            "the associated platform must be reachable through the global object"
        );

        // A full collection must not sweep the associated platform's cells or
        // the JS payload they reference.
        engine.gc();

        assert!(
            !payload_collected.get(),
            "the payload referenced only through the associated platform cell must survive gc"
        );
        assert!(
            engine
                .with_object_any(&global)
                .and_then(|data| data.downcast_ref::<TestPlatform>())
                .and_then(|platform| platform.cell.as_ref())
                .is_some(),
            "the associated platform cell must survive gc"
        );
        drop(payload_weak);
    }

    #[test]
    fn traced_captures_keep_payload_alive_and_follow_function_lifetime() {
        struct Payload(Rc<Cell<bool>>);
        impl Finalize for Payload {}
        unsafe impl Trace for Payload {
            unsafe fn trace(&self, _visitor: &mut Visitor) {}
            fn store(&mut self, _ec: &mut dyn ExecutionContext<V8Types>) {}
        }
        impl Drop for Payload {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }

        let mut engine = V8Engine::new();
        let finalized = Rc::new(Cell::new(false));
        let function_name = engine.property_key_from_str("tracedCapturesFn");

        let function: V8Function = create_builtin_fn_with_captures::<V8Types, Payload>(
            &mut engine,
            Payload(Rc::clone(&finalized)),
            |_arguments,
             _this_value,
             _captures: &Payload,
             ec: &mut dyn ExecutionContext<V8Types>| Ok(ec.value_undefined()),
            0,
            function_name,
            false,
        );

        // The function handle is rooted: the traced captures must survive a
        // full collection (their platform is traced from the wrapper, which
        // is rooted by the record's behaviour closure).
        engine.gc();
        assert!(
            !finalized.get(),
            "captures of a rooted function must survive gc"
        );

        // Dropping the function handle frees the record, which drops the
        // closure's root on the captures wrapper; the wrapper and its
        // captures platform are then collectable.
        drop(function);
        engine.gc();
        assert!(
            finalized.get(),
            "captures must be released once the function is collected"
        );
    }
}

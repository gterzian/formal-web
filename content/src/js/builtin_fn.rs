use js_engine::{Completion, ExecutionContext, JsTypes, JsTypesWithRealm};

/// The behaviour of a builtin function with traced captures: the arguments,
/// the `this` value, the captures and the execution context.
pub(crate) type TracedCaptureBehaviour<T, C> = fn(
    &[<T as JsTypes>::JsValue],
    <T as JsTypes>::JsValue,
    &C,
    &mut dyn ExecutionContext<T>,
) -> Completion<<T as JsTypes>::JsValue, T>;

/// Create a builtin function with GC-traceable captures.
/// Generic over `T` so Web IDL infrastructure (operation.rs, attribute.rs)
/// can call it with their own type parameter.
///
/// The captures are stored in a cppgc-traced platform object (V8) or the
/// engine's traced-capture storage (Boa/JSC), so their cells and JS edges
/// stay alive exactly while the function is reachable. Do NOT close over JS
/// handles in a bare closure passed to the engine's `make_builtin_function`:
/// a Rust closure's captures cannot be traced, so such handles are untraced
/// roots (realm pinning) or leave cells collectable mid-flight (streams
/// liveness). See `content/src/js/bindings/README.md`.
#[cfg(boa_backend)]
pub(crate) fn create_builtin_fn_with_traced_captures<T, C>(
    ec: &mut dyn ExecutionContext<T>,
    captures: C,
    behaviour: TracedCaptureBehaviour<T, C>,
    length: u32,
    name: T::PropertyKey,
    is_constructor: bool,
) -> T::Function
where
    T: JsTypes + JsTypesWithRealm,
    C: js_engine::gc::Trace + 'static,
{
    js_engine::boa::create_builtin_fn_with_captures(
        ec,
        captures,
        behaviour,
        length,
        name,
        is_constructor,
    )
}

#[cfg(v8_backend)]
pub(crate) fn create_builtin_fn_with_traced_captures<T, C>(
    ec: &mut dyn ExecutionContext<T>,
    captures: C,
    behaviour: TracedCaptureBehaviour<T, C>,
    length: u32,
    name: T::PropertyKey,
    is_constructor: bool,
) -> T::Function
where
    T: JsTypes + JsTypesWithRealm,
    C: js_engine::gc::Trace + 'static,
{
    js_engine::v8::create_builtin_fn_with_captures(
        ec,
        captures,
        behaviour,
        length,
        name,
        is_constructor,
    )
}

#[cfg(jsc_backend)]
pub(crate) fn create_builtin_fn_with_traced_captures<T, C>(
    ec: &mut dyn ExecutionContext<T>,
    captures: C,
    behaviour: TracedCaptureBehaviour<T, C>,
    length: u32,
    name: T::PropertyKey,
    is_constructor: bool,
) -> T::Function
where
    T: JsTypes + JsTypesWithRealm,
    C: 'static,
{
    use js_engine::jsc::{JscFunction, JscPropertyKey, JscTypes};

    // SAFETY: On the JSC backend, T is always JscTypes.
    let jsc_ec: &mut dyn ExecutionContext<JscTypes> = unsafe { std::mem::transmute(ec) };

    // SAFETY: fn pointers are all usize-sized regardless of signature.
    let jsc_behaviour: TracedCaptureBehaviour<JscTypes, C> =
        unsafe { std::mem::transmute(behaviour) };

    // SAFETY: T::PropertyKey and JscPropertyKey have same size at runtime.
    let jsc_name: JscPropertyKey = unsafe {
        let mut dst = std::mem::MaybeUninit::uninit();
        std::ptr::copy_nonoverlapping(
            &name as *const T::PropertyKey as *const u8,
            dst.as_mut_ptr() as *mut u8,
            std::mem::size_of::<JscPropertyKey>(),
        );
        std::mem::forget(name);
        dst.assume_init()
    };

    let result = js_engine::jsc::create_builtin_fn_with_captures(
        jsc_ec,
        captures,
        jsc_behaviour,
        length,
        jsc_name,
        is_constructor,
    );

    // SAFETY: T::Function and JscFunction have same size at runtime.
    unsafe {
        let mut dst = std::mem::MaybeUninit::uninit();
        std::ptr::copy_nonoverlapping(
            &result as *const JscFunction as *const u8,
            dst.as_mut_ptr() as *mut u8,
            std::mem::size_of::<JscFunction>(),
        );
        let _ = result;
        dst.assume_init()
    }
}

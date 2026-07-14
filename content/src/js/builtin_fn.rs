use js_engine::{Completion, ExecutionContext, JsTypes, JsTypesWithRealm};

/// Create a builtin function with GC-traceable captures.
/// Generic over `T` so Web IDL infrastructure (operation.rs, attribute.rs)
/// can call it with their own type parameter.
#[cfg(not(jsc_backend))]
pub(crate) fn create_builtin_fn_with_traced_captures<T, C>(
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

#[cfg(jsc_backend)]
pub(crate) fn create_builtin_fn_with_traced_captures<T, C>(
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
    T: JsTypes + JsTypesWithRealm,
    C: 'static,
{
    use js_engine::jsc::{JscFunction, JscPropertyKey, JscTypes, JscValue};

    // SAFETY: On the JSC backend, T is always JscTypes.
    let jsc_ec: &mut dyn ExecutionContext<JscTypes> = unsafe { std::mem::transmute(ec) };

    // SAFETY: fn pointers are all usize-sized regardless of signature.
    let jsc_behaviour: fn(
        &[JscValue],
        JscValue,
        &C,
        &mut dyn ExecutionContext<JscTypes>,
    ) -> Completion<JscValue, JscTypes> = unsafe { std::mem::transmute(behaviour) };

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

use js_engine::{Completion, ExecutionContext, JsTypes, JsTypesWithRealm};

/// Create a builtin function with GC-traceable captures.
/// Generic over `T` so Web IDL infrastructure (operation.rs, attribute.rs)
/// can call it with their own type parameter.
///
/// The captures are stored in a cppgc-traced platform object (V8) or the
/// engine's traced-capture storage (Boa), so their cells and JS edges
/// stay alive exactly while the function is reachable. Do NOT close over JS
/// handles in a bare closure passed to the engine's `make_builtin_function`:
/// a Rust closure's captures cannot be traced, so such handles are untraced
/// roots (realm pinning). See `content/src/js/bindings/README.md`.
#[cfg(boa_backend)]
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

#[cfg(v8_backend)]
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
    js_engine::v8::create_builtin_fn_with_captures(
        ec,
        captures,
        behaviour,
        length,
        name,
        is_constructor,
    )
}

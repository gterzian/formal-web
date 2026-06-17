//! <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
//!
//! The WindowProxy is an exotic object that wraps a Window ordinary object
//! and is implemented as a JavaScript Proxy (see Web IDL observable array
//! pattern in §3.10).  The Proxy target is the inner Window and the handler
//! is an ordinary object whose trap functions implement the WindowProxy
//! semantics per HTML §7.2.3.
//!
//! For same-origin access (currently always true) most operations delegate
//! to the inner Window object via the default Proxy behavior.  Only traps
//! that differ from the default Proxy behavior are provided:
//! - [[SetPrototypeOf]]: SetImmutablePrototype
//! - [[PreventExtensions]]: always false
//!
//! [[IsExtensible]] is left to the default (delegates to target), which
//! satisfies the Proxy invariant (trap result must match target result)
//! while still returning the correct value for an extensible Window.

use boa_engine::{
    Context, JsObject, JsResult, JsValue,
    builtins::proxy::Proxy,
    js_string,
    native_function::NativeFunction,
    object::FunctionObjectBuilder,
};

/// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
///
/// Create a WindowProxy exotic object wrapping the given Window, using a
/// JavaScript Proxy with custom handler traps.
pub(crate) fn create_window_proxy(
    window: &JsObject,
    context: &mut Context,
) -> JsResult<JsValue> {
    // Create handler with null prototype, matching the Web IDL observable
    // array pattern.
    let handler = JsObject::with_null_proto();

    // ── setPrototypeOf trap ──
    // <https://html.spec.whatwg.org/#windowproxy-setprototypeof>
    // Implements SetImmutablePrototype: only succeeds if V matches current.
    {
        let trap = NativeFunction::from_copy_closure_with_captures(
            |_this, args, window: &JsObject, _context| {
                let current = window.prototype();
                let undefined_val = JsValue::undefined();
                let val = args.get(1).unwrap_or(&undefined_val);

                let same = match (&current, val) {
                    (Some(current_proto), _) => val
                        .as_object()
                        .map_or(false, |v| JsObject::equals(current_proto, &v)),
                    (None, _) => val.is_null(),
                };
                Ok(JsValue::new(same))
            },
            window.clone(),
        );
        let set_prototype_of_fn = FunctionObjectBuilder::new(context.realm(), trap)
            .name(js_string!("setPrototypeOf"))
            .length(2)
            .build();
        handler.create_data_property_or_throw(
            js_string!("setPrototypeOf"),
            set_prototype_of_fn,
            context,
        )?;
    }

    // ── preventExtensions trap ──
    // <https://html.spec.whatwg.org/#windowproxy-preventextensions>
    {
        let trap = NativeFunction::from_copy_closure_with_captures(
            |_this, _args, _window: &JsObject, _context| Ok(JsValue::new(false)),
            window.clone(),
        );
        let prevent_extensions_fn = FunctionObjectBuilder::new(context.realm(), trap)
            .name(js_string!("preventExtensions"))
            .length(1)
            .build();
        handler.create_data_property_or_throw(
            js_string!("preventExtensions"),
            prevent_extensions_fn,
            context,
        )?;
    }

    // All other traps (get, set, has, deleteProperty, defineProperty,
    // getOwnPropertyDescriptor, ownKeys, getPrototypeOf) are NOT provided.
    // The default Proxy behavior delegates them to the target (Window),
    // which is correct for the same-origin case.

    Proxy::create(&JsValue::from(window.clone()), &handler.into(), context)
        .map(JsValue::from)
}

/// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
///
/// Resolve the inner Window from a value that may be a WindowProxy
/// (Proxy exotic object wrapping a Window).  When accessors on
/// Window.prototype are invoked via a WindowProxy (e.g.
/// `proxy.onload = ...`), the receiver (`this`) is the Proxy object.
/// This function extracts the Proxy's target (the inner Window) so
/// that downcasts succeed.
pub(crate) fn resolve_window(value: &JsValue, context: &Context) -> JsObject {
    if let Some(object) = value.as_object() {
        if let Some(proxy) = object.downcast_ref::<Proxy>() {
            if let Ok((target, _)) = proxy.try_data() {
                return target;
            }
        }
        return object.clone();
    }
    context.global_object()
}

// ── Cross-origin helper operations ──
// These implement the abstract operations from HTML spec § 7.2.1.3.
// They are ready for cross-origin support.

/// <https://html.spec.whatwg.org/#crossoriginproperties-(-o-)>
#[allow(dead_code)]
struct CrossOriginPropertyEntry {
    property: &'static str,
    needs_get: bool,
    needs_set: bool,
}

/// <https://html.spec.whatwg.org/#crossoriginproperties-(-o-)>
#[allow(dead_code)]
fn cross_origin_window_properties() -> Vec<CrossOriginPropertyEntry> {
    vec![
        CrossOriginPropertyEntry { property: "window", needs_get: true, needs_set: false },
        CrossOriginPropertyEntry { property: "self", needs_get: true, needs_set: false },
        CrossOriginPropertyEntry { property: "location", needs_get: true, needs_set: true },
        CrossOriginPropertyEntry { property: "close", needs_get: false, needs_set: false },
        CrossOriginPropertyEntry { property: "closed", needs_get: true, needs_set: false },
        CrossOriginPropertyEntry { property: "focus", needs_get: false, needs_set: false },
        CrossOriginPropertyEntry { property: "blur", needs_get: false, needs_set: false },
        CrossOriginPropertyEntry { property: "frames", needs_get: true, needs_set: false },
        CrossOriginPropertyEntry { property: "length", needs_get: true, needs_set: false },
        CrossOriginPropertyEntry { property: "top", needs_get: true, needs_set: false },
        CrossOriginPropertyEntry { property: "opener", needs_get: true, needs_set: false },
        CrossOriginPropertyEntry { property: "parent", needs_get: true, needs_set: false },
        CrossOriginPropertyEntry { property: "postMessage", needs_get: false, needs_set: false },
    ]
}

/// <https://html.spec.whatwg.org/#crossoriginownpropertykeys-(-o-)>
#[allow(dead_code)]
pub(crate) fn cross_origin_own_property_keys() -> Vec<boa_engine::property::PropertyKey> {
    let mut keys = cross_origin_window_properties()
        .into_iter()
        .map(|p| boa_engine::property::PropertyKey::String(js_string!(p.property)))
        .collect::<Vec<_>>();

    keys.push(boa_engine::property::PropertyKey::String(js_string!("then")));
    keys.push(boa_engine::property::PropertyKey::Symbol(
        boa_engine::JsSymbol::to_string_tag(),
    ));
    keys.push(boa_engine::property::PropertyKey::Symbol(
        boa_engine::JsSymbol::has_instance(),
    ));
    keys.push(boa_engine::property::PropertyKey::Symbol(
        boa_engine::JsSymbol::is_concat_spreadable(),
    ));

    keys
}

/// <https://html.spec.whatwg.org/#crossoriginownpropertykeys-(-o-)>
#[allow(dead_code)]
pub(crate) fn is_cross_origin_property(name: &str) -> bool {
    cross_origin_window_properties()
        .iter()
        .any(|p| p.property == name)
}

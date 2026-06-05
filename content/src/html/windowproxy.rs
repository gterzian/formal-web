//! <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
//!
//! The WindowProxy is a handle that wraps a Window ordinary object.
//! Each browsing context has an associated WindowProxy; when the browsing
//! context is navigated, the Window object wrapped by the WindowProxy
//! is changed.
//!
//! For same-origin access the WindowProxy is transparent: the JsObject
//! returned to JavaScript IS the wrapped Window.  Cross-origin property
//! filtering will be added when the content process supports multiple
//! origins (the helper operations at the bottom of this module are
//! ready for that).
//!
//! Long term the WindowProxy should be a proper exotic JsObject (matching
//! <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>), but boa's
//! `InternalObjectMethods` is pub(crate).  Until boa exposes the vtable
//! construction publicly, the WindowProxy is a Rust-side handle that
//! resolves to the target Window's JsObject.

use boa_engine::{Context, JsData, js_string, object::JsObject, property::PropertyKey};
use boa_gc::{Finalize, Trace};

use super::Window;

/// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
///
/// Each browsing context has an associated WindowProxy object.
/// The [[Window]] internal slot points to the current Window ordinary
/// object for that browsing context's active document.
#[derive(Trace, Finalize, JsData)]
pub struct WindowProxy {
    /// <https://html.spec.whatwg.org/#windowproxy-[[window]]>
    window: JsObject,
}

impl WindowProxy {
    /// Create a new WindowProxy wrapping the given Window JsObject.
    pub(crate) fn new(window: JsObject) -> Self {
        Self { window }
    }

    /// Returns the wrapped Window JsObject handle.
    /// TODO: Called during navigation to swap the active Window
    /// without changing the proxy object identity.
    #[allow(dead_code)]
    pub(crate) fn window_handle(&self) -> &JsObject {
        &self.window
    }
}

// ── Cross-origin helper operations ──
// These implement the abstract operations from HTML spec § 7.2.1.3.
// They are ready for cross-origin support; the same-origin fast path
// is handled by returning the Window directly (no proxy needed).

#[allow(dead_code)]
struct CrossOriginProperty {
    property_name: &'static str,
    needs_get: bool,
    needs_set: bool,
}

/// <https://html.spec.whatwg.org/#crossoriginproperties-(-o-)>
#[allow(dead_code)]
fn cross_origin_properties() -> Vec<CrossOriginProperty> {
    vec![
        CrossOriginProperty {
            property_name: "window",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginProperty {
            property_name: "self",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginProperty {
            property_name: "location",
            needs_get: true,
            needs_set: true,
        },
        CrossOriginProperty {
            property_name: "close",
            needs_get: false,
            needs_set: false,
        },
        CrossOriginProperty {
            property_name: "closed",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginProperty {
            property_name: "focus",
            needs_get: false,
            needs_set: false,
        },
        CrossOriginProperty {
            property_name: "blur",
            needs_get: false,
            needs_set: false,
        },
        CrossOriginProperty {
            property_name: "frames",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginProperty {
            property_name: "length",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginProperty {
            property_name: "top",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginProperty {
            property_name: "opener",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginProperty {
            property_name: "parent",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginProperty {
            property_name: "postMessage",
            needs_get: false,
            needs_set: false,
        },
    ]
}

/// <https://html.spec.whatwg.org/#crossoriginownpropertykeys-(-o-)>
#[allow(dead_code)]
pub(crate) fn cross_origin_own_property_keys() -> Vec<PropertyKey> {
    let mut keys: Vec<PropertyKey> = cross_origin_properties()
        .into_iter()
        .map(|p| PropertyKey::String(js_string!(p.property_name)))
        .collect();

    keys.push(PropertyKey::String(js_string!("then")));
    keys.push(PropertyKey::Symbol(boa_engine::JsSymbol::to_string_tag()));
    keys.push(PropertyKey::Symbol(boa_engine::JsSymbol::has_instance()));
    keys.push(PropertyKey::Symbol(
        boa_engine::JsSymbol::is_concat_spreadable(),
    ));

    keys
}

/// <https://html.spec.whatwg.org/#crossoriginownpropertykeys-(-o-)>
#[allow(dead_code)]
pub(crate) fn is_cross_origin_property(name: &str) -> bool {
    cross_origin_properties()
        .iter()
        .any(|p| p.property_name == name)
}

/// <https://html.spec.whatwg.org/#isplatformobjectsameorigin-(-o-)>
#[allow(dead_code)]
pub(crate) fn is_platform_object_same_origin(_window: &Window, _context: &Context) -> bool {
    true
}

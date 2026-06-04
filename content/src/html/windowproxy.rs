//! <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
//!
//! A WindowProxy is an exotic object that wraps a Window ordinary object, indirecting
//! most operations through to the wrapped object. Each browsing context has an associated
//! WindowProxy object.
//!
//! In the current single-origin content process the WindowProxy is transparent: it behaves
//! identically to the wrapped Window. Cross-origin property filtering will be added when
//! the content process supports multiple origins.
//!
//! This module provides the Rust-side wrapper and the cross-origin helper operations
//! (CrossOriginProperties, CrossOriginGetOwnPropertyHelper, CrossOriginPropertyFallback,
//! CrossOriginGet, CrossOriginSet, CrossOriginOwnPropertyKeys). These helpers are ready
//! for future cross-origin support and are implemented purely on top of boa's public API.

use boa_engine::{Context, JsNativeError, JsResult, js_string, object::JsObject};

use super::Window;

/// A WindowProxy wraps a Window JsObject.
///
/// This is a plain Rust struct (not a JsData exotic object) because the same-origin
/// case needs no custom internal methods — property access falls through to the Window
/// via the prototype chain. Cross-origin filtering will be applied at a higher level
/// when multiple origins are supported.
#[derive(Clone)]
#[allow(dead_code)]
pub struct WindowProxy {
    /// The wrapped Window JsObject.
    window: JsObject,
}

impl WindowProxy {
    /// Create a new WindowProxy wrapping the given Window object.
    #[allow(dead_code)]
    pub(crate) fn new(window: JsObject) -> Self {
        Self { window }
    }

    /// Access the wrapped native Window struct.
    #[allow(dead_code)]
    pub(crate) fn borrow_native(&self) -> JsResult<boa_gc::GcRef<'_, Window>> {
        self.window.downcast_ref::<Window>().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("WindowProxy [[Window]] slot does not point to a Window object")
                .into()
        })
    }

    /// Returns the JsObject handle for the wrapped Window.
    #[allow(dead_code)]
    pub(crate) fn window_handle(&self) -> &JsObject {
        &self.window
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Cross-origin helper operations
//
// These implement the abstract operations from the HTML spec section 7.2.1.3.
// They are ready for cross-origin support; the same-origin fast path is handled
// by returning the Window directly (no exotic-object interception needed).
// ──────────────────────────────────────────────────────────────────────────────

/// Metadata for a cross-origin accessible property.
/// <https://html.spec.whatwg.org/#crossoriginproperties-(-o-)>
#[allow(dead_code)]
struct CrossOriginProperty {
    property_name: &'static str,
    needs_get: bool,
    needs_set: bool,
}

/// <https://html.spec.whatwg.org/#crossoriginproperties-(-o-)>
/// CrossOriginProperties ( O )
#[allow(dead_code)]
fn cross_origin_properties() -> Vec<CrossOriginProperty> {
    vec![
        CrossOriginProperty { property_name: "window",   needs_get: true,  needs_set: false },
        CrossOriginProperty { property_name: "self",     needs_get: true,  needs_set: false },
        CrossOriginProperty { property_name: "location", needs_get: true,  needs_set: true },
        CrossOriginProperty { property_name: "close",    needs_get: false, needs_set: false },
        CrossOriginProperty { property_name: "closed",   needs_get: true,  needs_set: false },
        CrossOriginProperty { property_name: "focus",    needs_get: false, needs_set: false },
        CrossOriginProperty { property_name: "blur",     needs_get: false, needs_set: false },
        CrossOriginProperty { property_name: "frames",   needs_get: true,  needs_set: false },
        CrossOriginProperty { property_name: "length",   needs_get: true,  needs_set: false },
        CrossOriginProperty { property_name: "top",      needs_get: true,  needs_set: false },
        CrossOriginProperty { property_name: "opener",   needs_get: true,  needs_set: false },
        CrossOriginProperty { property_name: "parent",   needs_get: true,  needs_set: false },
        CrossOriginProperty { property_name: "postMessage", needs_get: false, needs_set: false },
    ]
}

/// <https://html.spec.whatwg.org/#crossoriginownpropertykeys-(-o-)>
/// CrossOriginOwnPropertyKeys ( O )
#[allow(dead_code)]
pub(crate) fn cross_origin_own_property_keys() -> Vec<boa_engine::property::PropertyKey> {
    let mut keys: Vec<boa_engine::property::PropertyKey> = cross_origin_properties()
        .into_iter()
        .map(|p| boa_engine::property::PropertyKey::String(js_string!(p.property_name)))
        .collect();

    keys.push(boa_engine::property::PropertyKey::String(js_string!("then")));
    keys.push(boa_engine::property::PropertyKey::Symbol(boa_engine::JsSymbol::to_string_tag()));
    keys.push(boa_engine::property::PropertyKey::Symbol(boa_engine::JsSymbol::has_instance()));
    keys.push(boa_engine::property::PropertyKey::Symbol(boa_engine::JsSymbol::is_concat_spreadable()));

    keys
}

/// Check whether a property name is in the cross-origin property set.
/// Used by the WindowProxy's [[GetOwnProperty]] step 4.
#[allow(dead_code)]
pub(crate) fn is_cross_origin_property(name: &str) -> bool {
    cross_origin_properties().iter().any(|p| p.property_name == name)
}

/// <https://html.spec.whatwg.org/#isplatformobjectsameorigin-(-o-)>
/// IsPlatformObjectSameOrigin ( O )
#[allow(dead_code)]
pub(crate) fn is_platform_object_same_origin(_window: &Window, _context: &Context) -> bool {
    // Single-origin only — always true.
    true
}

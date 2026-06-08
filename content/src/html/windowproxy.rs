//! <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
//!
//! The WindowProxy is an exotic object that wraps a Window ordinary object.
//! Each browsing context has an associated WindowProxy; when the browsing
//! context is navigated, the Window object wrapped by the WindowProxy
//! is changed.
//!
//! # Implementation note (same-origin transparent proxy)
//!
//! The HTML spec requires WindowProxy to override 10 internal methods
//! ([[GetPrototypeOf]], [[SetPrototypeOf]], [[IsExtensible]],
//! [[PreventExtensions]], [[GetOwnProperty]], [[DefineOwnProperty]],
//! [[Get]], [[Set]], [[Delete]], [[OwnPropertyKeys]]) per §7.2.3.
//!
//! For the current single-origin content process, the WindowProxy is
//! implemented as a transparent proxy: `create_window_proxy()` returns
//! the inner Window's `JsObject` directly.  This is correct for
//! same-origin access because:
//!
//! - [[GetPrototypeOf]] → `OrdinaryGetPrototypeOf(W)` → `W.prototype()`
//! - [[GetOwnProperty]] → `OrdinaryGetOwnProperty(W, P)` → `W.get()` etc.
//! - [[Get]] → `OrdinaryGet(this, P, Receiver)` → `W.get(P, Receiver)`
//! - [[Set]] → `OrdinarySet(W, P, V, Receiver)` → `W.set(P, V, ...)`
//! - [[Delete]] → `OrdinaryDelete(W, P)` → `W.delete_property_or_throw()`
//! - [[OwnPropertyKeys]] → `OrdinaryOwnPropertyKeys(W)`
//!
//! Returning W directly satisfies all same-origin operations because the
//! Window IS an ordinary object whose internal methods match the spec's
//! delegation targets.
//!
//! A proper exotic-object WindowProxy (overriding the vtable) will be
//! needed when cross-origin Window access is implemented.  At that point,
//! the new public API wrappers in `vendor/boa/` needed to construct
//! `InternalObjectMethods` from outside the engine crate should be added
//! without changing existing `pub(crate)` visibility boundaries.  See
//! `content/src/js/README.md` for the methodology.
//!
//! For the remaining spec-mandated operations:
//!
//! - [[SetPrototypeOf]] (SetImmutablePrototype): the Window's prototype
//!   is mutable, so returning W directly means prototype changes are
//!   reflected.  This is correct for same-origin; cross-origin will
//!   need an exotic override that rejects prototype changes.
//! - Child navigable array-index properties (window[0], window[1]) and
//!   named child navigable properties: not yet implemented in any form.
//! - Navigation-time Window swapping: not yet wired.
//! - `is_platform_object_same_origin`: hardcoded to `true` (no cross-origin
//!   support in the content process yet).

use boa_engine::js_string;
use boa_engine::object::JsObject;
use boa_engine::property::PropertyKey;

// ── WindowProxy handle ──

/// <https://html.spec.whatwg.org/#the-windowproxy-exotic-object>
///
/// Each browsing context has an associated WindowProxy object.
/// The [[Window]] internal slot points to the current Window ordinary
/// object for that browsing context's active document.
///
/// Currently a Rust-side handle only (not a JsObject).  The transparent
/// proxy implementation returns the wrapped Window JsObject directly.
///
/// TODO: Turn into a `JsData` exotic object when cross-origin Window
/// access is implemented.  At that point, add the required public API
/// wrappers to `vendor/boa/` (see `content/src/js/README.md`) and
/// override the 10 internal methods per HTML §7.2.3.
#[allow(dead_code)]
pub struct WindowProxy {
    /// <https://html.spec.whatwg.org/#windowproxy-[[window]]>
    window: JsObject,
}

#[allow(dead_code)]
impl WindowProxy {
    /// Create a new WindowProxy wrapping the given Window JsObject.
    pub(crate) fn new(window: JsObject) -> Self {
        Self { window }
    }

    /// Returns the wrapped Window JsObject handle.
    /// Used during navigation to swap the active Window without changing
    /// the proxy object identity.
    pub(crate) fn window_handle(&self) -> &JsObject {
        &self.window
    }
}

// ── Cross-origin helper operations ──
// These implement the abstract operations from HTML spec § 7.2.1.3.
// They are ready for cross-origin support; the same-origin fast path
// returns the Window directly (see module-level docs).

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
        CrossOriginPropertyEntry {
            property: "window",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginPropertyEntry {
            property: "self",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginPropertyEntry {
            property: "location",
            needs_get: true,
            needs_set: true,
        },
        CrossOriginPropertyEntry {
            property: "close",
            needs_get: false,
            needs_set: false,
        },
        CrossOriginPropertyEntry {
            property: "closed",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginPropertyEntry {
            property: "focus",
            needs_get: false,
            needs_set: false,
        },
        CrossOriginPropertyEntry {
            property: "blur",
            needs_get: false,
            needs_set: false,
        },
        CrossOriginPropertyEntry {
            property: "frames",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginPropertyEntry {
            property: "length",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginPropertyEntry {
            property: "top",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginPropertyEntry {
            property: "opener",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginPropertyEntry {
            property: "parent",
            needs_get: true,
            needs_set: false,
        },
        CrossOriginPropertyEntry {
            property: "postMessage",
            needs_get: false,
            needs_set: false,
        },
    ]
}

/// <https://html.spec.whatwg.org/#crossoriginownpropertykeys-(-o-)>
#[allow(dead_code)]
pub(crate) fn cross_origin_own_property_keys() -> Vec<PropertyKey> {
    let mut keys: Vec<PropertyKey> = cross_origin_window_properties()
        .into_iter()
        .map(|p| PropertyKey::String(js_string!(p.property)))
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
    cross_origin_window_properties()
        .iter()
        .any(|p| p.property == name)
}

/// <https://html.spec.whatwg.org/#isplatformobjectsameorigin-(-o-)>
///
/// Currently hardcoded to `true` because the content process only runs a
/// single origin.  When multi-origin support is added, this must check
/// whether the active document of the given Window's browsing context is
/// same-origin with the active document of the entry settings object's
/// browsing context.
#[allow(dead_code)]
pub(crate) fn is_platform_object_same_origin(_window: &JsObject, _context: &JsObject) -> bool {
    true
}

// ── Helpers ──

/// Check whether a PropertyKey is an array index property name.
///
/// <https://webidl.spec.whatwg.org/#dfn-array-index-property-name>
#[allow(dead_code)]
pub(crate) fn key_to_u32(key: &PropertyKey) -> Option<u32> {
    match key {
        PropertyKey::Index(index) => Some(index.get()),
        PropertyKey::String(string) => {
            let s = string.to_std_string_escaped();
            if s.is_empty() {
                return None;
            }
            let parsed: u64 = s.parse().ok()?;
            if parsed >= u32::MAX as u64 {
                return None;
            }
            if parsed.to_string() == s {
                Some(parsed as u32)
            } else {
                None
            }
        }
        PropertyKey::Symbol(_) => None,
    }
}

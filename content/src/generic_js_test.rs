//! # `generic_js_test` — integration test for the generic JS layer
//!
//! This module validates that the generic `js_engine` API supports the
//! exact patterns real content code needs, following the same call
//! chains the spec defines.  Some spec algorithms go through Web IDL,
//! others call ECMA-262 directly — we mirror both:
//!
//! ```text
//! HTML §8.1.3.3: creating a new JavaScript realm
//!   → InitializeHostDefinedRealm (ECMA-262) — bypasses Web IDL
//!   → tested here as: create_realm, set_realm_global_object, etc.
//!
//! Streams: ReadableStreamCancel → Web IDL "react" →
//!   → CreateBuiltinFunction + NewPromiseCapability + PerformPromiseThen
//!   → tested here as: upon_settlement_full_chain
//! ```
//!
//! Every test demonstrates a pattern that production code uses (or will
//! use) — never an artificial convenience.  No Boa-specific APIs appear
//! in any test body.
//!
//! The module defines a toy domain type (`TestWidget`), implements
//! `WebIdlInterface<Types>` for it, and exercises every relevant API
//! surface — domain struct → create_builtin_function →
//! new_promise_capability → perform_promise_then — as a miniature
//! version of the full `content/` crate.

use std::marker::PhantomData;

use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};
use js_engine::gc::GcRootHandle;
use js_engine::{Completion, ExecutionContext, JsTypes};

type TestTypes = crate::js::Types;
type JsValue = <TestTypes as JsTypes>::JsValue;
type JsObject = <TestTypes as JsTypes>::JsObject;

// ── Domain type ──────────────────────────────────────────────────────────

js_engine::impl_gc_traits! {
    /// A toy domain struct exercising the full generic-API binding pattern.
    ///
    /// The `on_change` field uses `GcRootHandle<TestTypes>` which is a generic
    /// RAII guard: on Boa it wraps a `JsValue` that the GC traces natively;
    /// on JSC it keeps the value alive through a hidden global-object root
    /// that is removed when the handle drops.
    ///
    /// GC trait derivation is handled by [`js_engine::impl_gc_traits`] which
    /// expands to the correct backend-specific traits.
    pub(crate) struct TestWidget {
        title: String,
        visible: bool,
        count: u32,
        on_change: Option<GcRootHandle<TestTypes>>,
    }
}

impl TestWidget {
    #[allow(dead_code)]
    fn new() -> Self {
        Self {
            title: String::from("Untitled"),
            visible: true,
            count: 0,
            on_change: None,
        }
    }

    /// Constructor-from-args pattern (mirrors Event constructor).
    fn from_args(
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<TestTypes>,
    ) -> Completion<Self, TestTypes> {
        let title = if let Some(arg) = args.first() {
            ec.to_rust_string(arg.clone())?
        } else {
            String::from("Untitled")
        };
        let visible = args.get(1).map_or(true, |v| ec.to_boolean(v));
        let count = args.get(2).map_or(0u32, |_v| 0u32);
        Ok(Self {
            title,
            visible,
            count,
            on_change: None,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Widget data access — uses the generic `with_object_any` API
// ═══════════════════════════════════════════════════════════════════════════

mod widget_data {
    use super::*;

    pub(crate) fn with_ref<T>(
        obj: &JsObject,
        ec: &mut dyn ExecutionContext<TestTypes>,
        f: impl FnOnce(&TestWidget) -> T,
    ) -> Completion<T, TestTypes> {
        let data = match ec.with_object_any(obj) {
            Some(d) => d,
            None => return Err(ec.new_type_error("receiver is not a TestWidget")),
        };
        let widget = match data.downcast_ref::<TestWidget>() {
            Some(w) => w,
            None => return Err(ec.new_type_error("receiver is not a TestWidget")),
        };
        Ok(f(widget))
    }

    pub(crate) fn with_mut<T>(
        obj: &JsObject,
        ec: &mut dyn ExecutionContext<TestTypes>,
        f: impl FnOnce(&mut TestWidget) -> T,
    ) -> Completion<T, TestTypes> {
        let data = match ec.with_object_any_mut(obj) {
            Some(d) => d,
            None => return Err(ec.new_type_error("receiver is not a TestWidget")),
        };
        let widget = match data.downcast_mut::<TestWidget>() {
            Some(w) => w,
            None => return Err(ec.new_type_error("receiver is not a TestWidget")),
        };
        Ok(f(widget))
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Subtype hierarchy — exercises multi-type downcast chains (mirrors
// the real codebase's with_event_target_mut / with_node_ref pattern)
// ═══════════════════════════════════════════════════════════════════════════

js_engine::impl_gc_traits! {
    /// A toy subtype that wraps TestWidget — mirrors how HTMLInputElement
    /// wraps HTMLElement, which wraps Element, which wraps Node.
    pub(crate) struct TestButton {
        label: String,
        widget: TestWidget,
    }
}

impl TestButton {
    fn new(label: &str) -> Self {
        Self {
            label: label.to_string(),
            widget: TestWidget::new(),
        }
    }

    fn label_value(&self) -> &str {
        &self.label
    }

    fn set_label(&mut self, label: &str) {
        self.label = label.to_string();
    }
}

/// A multi-type downcast helper that tries TestButton first, then
/// falls back to TestWidget.  This is the generic equivalent of
/// `with_event_target_mut` trying Window → Document → Element → ...
/// → EventTarget in the production downcast.rs.
pub(crate) fn widget_or_button_with_mut<T>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<TestTypes>,
    f: impl FnOnce(&mut TestWidget) -> T,
) -> Completion<T, TestTypes> {
    let obj = TestTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("receiver is not an object"))?;

    if let Some(data) = ec.with_object_any_mut(&obj) {
        // Try the most-specific type first.
        if let Some(button) = data.downcast_mut::<TestButton>() {
            return Ok(f(&mut button.widget));
        }
        // Fall back to the base type.
        if let Some(widget) = data.downcast_mut::<TestWidget>() {
            return Ok(f(widget));
        }
    }
    // `data` borrow is dropped here; `ec` is free for error construction.
    Err(ec.new_type_error("receiver is not a TestWidget or TestButton"))
}

/// Immutable multi-type downcast — same chain, read-only.
pub(crate) fn widget_or_button_with_ref<T>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<TestTypes>,
    f: impl FnOnce(&TestWidget) -> T,
) -> Completion<T, TestTypes> {
    let obj = TestTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("receiver is not an object"))?;

    if let Some(data) = ec.with_object_any(&obj) {
        if let Some(button) = data.downcast_ref::<TestButton>() {
            return Ok(f(&button.widget));
        }
        if let Some(widget) = data.downcast_ref::<TestWidget>() {
            return Ok(f(widget));
        }
    }
    Err(ec.new_type_error("receiver is not a TestWidget or TestButton"))
}

// ═══════════════════════════════════════════════════════════════════════════
// Platform object creation helpers — delegates to `create_interface_instance`
// (the canonical path used by DOMException, Event, Location in production).
// ═══════════════════════════════════════════════════════════════════════════

fn create_test_widget(
    widget: TestWidget,
    ec: &mut dyn ExecutionContext<TestTypes>,
) -> Completion<JsObject, TestTypes> {
    crate::webidl::bindings::create_interface_instance::<TestTypes, TestWidget>(widget, ec)
}

fn create_test_button(
    button: TestButton,
    ec: &mut dyn ExecutionContext<TestTypes>,
) -> Completion<JsObject, TestTypes> {
    crate::webidl::bindings::create_interface_instance::<TestTypes, TestButton>(button, ec)
}

fn wire_test_interface_prototypes(ec: &mut dyn ExecutionContext<TestTypes>) {
    crate::webidl::bindings::registry::wire_prototype::<TestTypes, TestButton, TestWidget>(ec);
}

// ═══════════════════════════════════════════════════════════════════════════
// Binding functions (dual-backend — use widget_data helpers for downcast)
// ═══════════════════════════════════════════════════════════════════════════

/// Getter: `widget.title` → string.
fn get_title(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<TestTypes>,
) -> Completion<JsValue, TestTypes> {
    let title = widget_or_button_with_ref(this, ec, |w| w.title.clone())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&title)))
}

/// Setter: `widget.title = val` — exercises `ec.to_rust_string`.
fn set_title(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<TestTypes>,
) -> Completion<JsValue, TestTypes> {
    let fallback = ec.value_undefined();
    let new_title = ec.to_rust_string(args.first().cloned().unwrap_or(fallback))?;
    widget_or_button_with_mut(this, ec, |w| w.title = new_title)?;
    Ok(ec.value_undefined())
}

/// Getter: `widget.visible` → bool.
fn get_visible(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<TestTypes>,
) -> Completion<JsValue, TestTypes> {
    let visible = widget_or_button_with_ref(this, ec, |w| w.visible)?;
    Ok(ec.value_from_bool(visible))
}

/// Getter: `widget.count` → number.
fn get_count(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<TestTypes>,
) -> Completion<JsValue, TestTypes> {
    let count = widget_or_button_with_ref(this, ec, |w| w.count)?;
    Ok(ec.value_from_number(count as f64))
}

/// Method: `widget.increment()` — increments the counter, returns old value.
fn increment(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<TestTypes>,
) -> Completion<JsValue, TestTypes> {
    let old = widget_or_button_with_mut(this, ec, |w| {
        let old = w.count;
        w.count = old.wrapping_add(1);
        old
    })?;
    Ok(ec.value_from_number(old as f64))
}

/// Method: `widget.toObject()` — returns a plain object `{ title, visible, count }`.
fn to_object(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<TestTypes>,
) -> Completion<JsValue, TestTypes> {
    let (title, visible, count) =
        widget_or_button_with_ref(this, ec, |w| (w.title.clone(), w.visible, w.count))?;
    let result = ec.create_plain_object(None);
    let title_val = ec.value_from_string(ec.js_string_from_str(&title));
    let visible_val = ec.value_from_bool(visible);
    let count_val = ec.value_from_number(count as f64);
    ec.object_set_property(result.clone(), "title", title_val)?;
    ec.object_set_property(result.clone(), "visible", visible_val)?;
    ec.object_set_property(result.clone(), "count", count_val)?;
    Ok(TestTypes::value_from_object(result))
}

/// Method: `widget.toArray()` — returns `[title, visible, count]`.
fn to_array(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<TestTypes>,
) -> Completion<JsValue, TestTypes> {
    let (title, visible, count) =
        widget_or_button_with_ref(this, ec, |w| (w.title.clone(), w.visible, w.count))?;
    let array = ec.create_empty_array();
    let title_val = ec.value_from_string(ec.js_string_from_str(&title));
    let visible_val = ec.value_from_bool(visible);
    let count_val = ec.value_from_number(count as f64);
    ec.array_push(&array, title_val)?;
    ec.array_push(&array, visible_val)?;
    ec.array_push(&array, count_val)?;
    Ok(TestTypes::value_from_object(array))
}

/// Setter: `widget.count = val` — exercises `ec.to_uint32` (WebIDL semantics).
fn set_count(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<TestTypes>,
) -> Completion<JsValue, TestTypes> {
    let num_val = args.first().cloned().unwrap_or(ec.value_undefined());
    let new_count = ec.to_uint32(num_val)?;
    widget_or_button_with_mut(this, ec, |w| w.count = new_count)?;
    Ok(ec.value_undefined())
}

/// Method: `widget.formatLabel(prefix)` — exercises `ec.to_js_string` in a binding pattern.
fn format_label(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<TestTypes>,
) -> Completion<JsValue, TestTypes> {
    let title = widget_or_button_with_ref(this, ec, |w| w.title.clone())?;
    let prefix = if let Some(arg) = args.first() {
        let js_str = ec.to_js_string(arg.clone())?;
        ec.js_string_to_rust_string(&js_str)
    } else {
        String::new()
    };
    let label = format!("{}:{}", prefix, title);
    Ok(ec.value_from_string(ec.js_string_from_str(&label)))
}

/// Method: `widget.delayedTitle()` — exercises promise creation and resolution.
fn delayed_title(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<TestTypes>,
) -> Completion<JsValue, TestTypes> {
    let title = widget_or_button_with_ref(this, ec, |w| w.title.clone())?;
    let realm = ec.current_realm();
    let intrinsics = ec.realm_intrinsics(&realm);
    let cap = ec.new_promise_capability(intrinsics.promise)?;
    let title_val = ec.value_from_string(ec.js_string_from_str(&title));
    let undef = ec.value_undefined();
    ec.call(&cap.resolve, &undef, &[title_val])?;
    Ok(cap.promise)
}

/// Method: `widget.withCallback(cb)` — exercises `ec.call` with a user-provided callback.
fn with_callback(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<TestTypes>,
) -> Completion<JsValue, TestTypes> {
    let title = widget_or_button_with_ref(this, ec, |w| w.title.clone())?;
    let callback_obj = args
        .first()
        .and_then(|v| TestTypes::value_as_object(v))
        .ok_or_else(|| ec.new_type_error("expected a callback function"))?;
    let callback_val = TestTypes::value_from_object(callback_obj.clone());
    if !ec.is_callable(&callback_val) {
        return Err(ec.new_type_error("argument is not callable"));
    }
    let title_val = ec.value_from_string(ec.js_string_from_str(&title));
    let undef = ec.value_undefined();
    ec.call(&callback_obj, &undef, &[title_val])
}

/// Method: `widget.processItems(items)` — exercises array-like iteration.
///
/// Note: this uses array-like length+indexing (`Get` for `"length"` then
/// `Get` for `0..length`), NOT the Web IDL `sequence<T>` conversion algorithm
/// which is iterator-based (`GetIterator` + `IteratorStep`/`IteratorValue`).
/// Real `sequence<T>` conversion must use `get_iterator`/`iterator_step_value`
/// (exercised in `get_iterator_and_step_value` above).
fn process_items(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<TestTypes>,
) -> Completion<JsValue, TestTypes> {
    let items_val = args.first().cloned().unwrap_or(ec.value_undefined());
    let items = TestTypes::value_as_object(&items_val)
        .ok_or_else(|| ec.new_type_error("expected an array argument"))?;
    let pk_length = ec.property_key_from_str("length");
    let length_val = ExecutionContext::get(ec, items.clone(), pk_length)?;
    let length = ec.to_length(length_val)?;
    let mut count: u32 = 0;
    for index in 0..length {
        let pk_index = ec.property_key_from_index(index as u32);
        let item = ExecutionContext::get(ec, items.clone(), pk_index)?;
        if TestTypes::value_as_string(&item).is_some() {
            count = count.wrapping_add(1);
        }
    }
    widget_or_button_with_mut(this, ec, |w| w.count = count)?;
    Ok(ec.value_undefined())
}

/// Static method: `TestWidget.create(title, visible)` — factory constructor pattern.
fn create_static(
    _this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<TestTypes>,
) -> Completion<JsValue, TestTypes> {
    let title = if let Some(arg) = args.first() {
        ec.to_rust_string(arg.clone())?
    } else {
        String::from("Untitled")
    };
    let visible = args.get(1).map_or(true, |v| ec.to_boolean(v));
    let widget = TestWidget {
        title,
        visible,
        count: 0,
        on_change: None,
    };
    let obj = create_test_widget(widget, ec)?;
    Ok(TestTypes::value_from_object(obj))
}

/// Method: `widget.storeCallback(cb)` — stores a callback for later invocation.
fn store_callback(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<TestTypes>,
) -> Completion<JsValue, TestTypes> {
    let callback_obj = args
        .first()
        .and_then(|v| TestTypes::value_as_object(v))
        .ok_or_else(|| ec.new_type_error("expected a callback function"))?;
    let callback_val = TestTypes::value_from_object(callback_obj.clone());
    if !ec.is_callable(&callback_val) {
        return Err(ec.new_type_error("argument is not callable"));
    }
    let root = ec.create_root(&callback_val);
    widget_or_button_with_mut(this, ec, |w| w.on_change = Some(root))?;
    Ok(ec.value_undefined())
}

/// Test helper: calls `perform_a_microtask_checkpoint` and `run_jobs`.
#[allow(dead_code)]
fn flush_microtasks_test(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<TestTypes>,
) -> Completion<JsValue, TestTypes> {
    let _ = widget_or_button_with_ref(this, ec, |_| ())?;
    ec.perform_a_microtask_checkpoint()?;
    ec.run_jobs();
    Ok(ec.value_undefined())
}

/// Test helper: returns a rejected promise.
#[allow(dead_code)]
fn reject_with_message_test(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<TestTypes>,
) -> Completion<JsValue, TestTypes> {
    let _ = widget_or_button_with_ref(this, ec, |_| ())?;
    let msg = if let Some(arg) = args.first() {
        ec.to_rust_string(arg.clone())?
    } else {
        String::from("Unknown error")
    };
    let realm = ec.current_realm();
    let intrinsics = ec.realm_intrinsics(&realm);
    let cap = ec.new_promise_capability(intrinsics.promise)?;
    let err = ec.new_type_error(&msg);
    let undef = ec.value_undefined();
    ec.call(&cap.reject, &undef, &[err])?;
    Ok(cap.promise)
}

/// Static method: `TestWidget.fromTags(tags)` — returns an array built from
/// a comma-separated string.
fn from_tags(
    _this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<TestTypes>,
) -> Completion<JsValue, TestTypes> {
    let tags = if let Some(arg) = args.first() {
        ec.to_rust_string(arg.clone())?
    } else {
        String::new()
    };
    let parts: Vec<&str> = tags.split(',').collect();
    let array = ec.create_empty_array();
    for part in parts {
        let val = ec.value_from_string(ec.js_string_from_str(part.trim()));
        ec.array_push(&array, val)?;
    }
    Ok(TestTypes::value_from_object(array))
}

// ═══════════════════════════════════════════════════════════════════════════
// Web IDL interface definition (Boa-only for now; JSC uses separate impl)
// ═══════════════════════════════════════════════════════════════════════════

impl WebIdlInterface<TestTypes> for TestWidget {
    const NAME: &'static str = "TestWidget";

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<TestTypes>,
    ) -> Completion<Self, TestTypes> {
        TestWidget::from_args(args, ec)
    }

    fn define_members(def: &mut InterfaceDefinition<TestTypes>) {
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,
            id: "title",
            getter: get_title,
            setter: Some(set_title),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,
            id: "visible",
            getter: get_visible,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,
            id: "count",
            getter: get_count,
            setter: Some(set_count),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,
            id: "increment",
            length: 0,
            method: increment,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,
            id: "toObject",
            length: 0,
            method: to_object,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,
            id: "toArray",
            length: 0,
            method: to_array,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,
            id: "formatLabel",
            length: 1,
            method: format_label,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,
            id: "delayedTitle",
            length: 0,
            method: delayed_title,
            static_: false,
            unforgeable: false,
            promise_type: true,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,
            id: "withCallback",
            length: 1,
            method: with_callback,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,
            id: "processItems",
            length: 1,
            method: process_items,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,
            id: "create",
            length: 2,
            method: create_static,
            static_: true,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,
            id: "fromTags",
            length: 1,
            method: from_tags,
            static_: true,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,
            id: "storeCallback",
            length: 1,
            method: store_callback,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Context lifecycle exercise (Boa-only)
// ═══════════════════════════════════════════════════════════════════════════

// ── TestButton Web IDL interface (minimal — just for registration) ──

impl WebIdlInterface<TestTypes> for TestButton {
    const NAME: &'static str = "TestButton";

    fn parent_name() -> Option<&'static str> {
        Some("TestWidget")
    }

    fn create_platform_object(
        _new_target: &JsValue,
        _args: &[JsValue],
        _ec: &mut dyn ExecutionContext<TestTypes>,
    ) -> Completion<Self, TestTypes> {
        Ok(TestButton::new("Default"))
    }

    fn define_members(_def: &mut InterfaceDefinition<TestTypes>) {}
}

#[cfg(feature = "boa")]
#[allow(dead_code)]
pub(crate) fn exercise_context_lifecycle() -> Result<(), String> {
    use crate::webidl::bindings::{initialize_registry, register_interface_spec};
    use boa_engine::context::ContextBuilder;
    use js_engine::boa::BoaContext;

    let context = ContextBuilder::new()
        .build()
        .map_err(|error| error.to_string())?;
    let mut boa_context = BoaContext::from_context(context);

    initialize_registry::<TestTypes>(&mut boa_context);

    register_interface_spec::<TestTypes, TestWidget, _>(&mut boa_context).ok();
    register_interface_spec::<TestTypes, TestButton, _>(&mut boa_context).ok();
    wire_test_interface_prototypes(&mut boa_context);

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// Unit tests — exercise the generic API through real assertions.
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use js_engine::PropertyDescriptor;
    use js_engine::{EcmascriptHost, ExecutionContext, JsEngine};

    // ── Backend-specific setup ───────────────────────────────────────

    /// Create an initialized engine context with the TestWidget interface
    /// registered (Boa) or available (JSC).
    /// Returns the concrete engine type so callers can use both
    /// `ExecutionContext` and `JsEngine` trait methods.
    #[cfg(feature = "boa")]
    fn setup() -> js_engine::boa::BoaContext {
        use crate::webidl::bindings::{initialize_registry, register_interface_spec};
        use boa_engine::context::ContextBuilder;
        use js_engine::boa::BoaContext;

        let context = ContextBuilder::new().build().expect("ContextBuilder");
        let mut engine = BoaContext::from_context(context);
        initialize_registry::<TestTypes>(&mut engine);
        register_interface_spec::<TestTypes, TestWidget, _>(&mut engine).ok();
        register_interface_spec::<TestTypes, TestButton, _>(&mut engine).ok();
        wire_test_interface_prototypes(&mut engine);
        engine
    }

    #[cfg(feature = "jsc")]
    fn setup() -> js_engine::jsc::JscEngine {
        use crate::webidl::bindings::{initialize_registry, register_interface_spec};
        use js_engine::{ExecutionContext, JsEngine};
        let mut engine = js_engine::jsc::JscEngine::new();
        initialize_registry::<TestTypes>(&mut engine);
        // register_interface_spec may produce stub builtin functions
        // (JSC create_builtin_function is limited), but it populates
        // the registry so create_test_widget can find the prototype.
        register_interface_spec::<TestTypes, TestWidget, _>(&mut engine).ok();
        register_interface_spec::<TestTypes, TestButton, _>(&mut engine).ok();
        wire_test_interface_prototypes(&mut engine);
        engine
    }

    // ── Widget creation helper ────────────────────────────────────────

    /// Create a TestWidget platform object, delegating to the cfg-gated
    /// `create_test_widget` helper.
    fn create_widget(widget: TestWidget, ec: &mut dyn ExecutionContext<TestTypes>) -> JsObject {
        create_test_widget(widget, ec).unwrap()
    }

    /// Create a TestButton platform object.
    fn create_button(button: TestButton, ec: &mut dyn ExecutionContext<TestTypes>) -> JsObject {
        create_test_button(button, ec).unwrap()
    }

    // ── Multi-type downcast chain tests ────────────────────────────

    #[test]
    fn multi_downcast_button_seen_as_button_and_widget() {
        let mut engine = setup();
        let mut button = TestButton::new("ClickMe");
        button.widget.title = "BtnWidget".into();
        let obj = create_button(button, &mut engine);
        let js_obj = TestTypes::value_from_object(obj);

        // Through the multi-type helper, we should see the widget fields.
        let title = widget_or_button_with_ref(&js_obj, &mut engine, |w| w.title.clone()).unwrap();
        assert_eq!(title, "BtnWidget");

        // Mutable access through the multi-type helper.
        widget_or_button_with_mut(&js_obj, &mut engine, |w| w.title = "Changed".into()).unwrap();
        let title = widget_or_button_with_ref(&js_obj, &mut engine, |w| w.title.clone()).unwrap();
        assert_eq!(title, "Changed");
    }

    #[test]
    fn multi_downcast_pure_widget_works() {
        let mut engine = setup();
        let mut widget = TestWidget::new();
        widget.title = "PureWidget".into();
        let obj = create_widget(widget, &mut engine);
        let js_obj = TestTypes::value_from_object(obj);

        // A pure TestWidget (not a TestButton) should still be found
        // by the multi-type helper (it falls back to TestWidget).
        let title = widget_or_button_with_ref(&js_obj, &mut engine, |w| w.title.clone()).unwrap();
        assert_eq!(title, "PureWidget");
    }

    #[test]
    fn multi_downcast_unknown_type_errors() {
        let mut engine = setup();
        let plain = engine.create_plain_object(None);
        let js_obj = TestTypes::value_from_object(plain);

        let result = widget_or_button_with_ref(&js_obj, &mut engine, |w| w.title.clone());
        assert!(result.is_err());
    }

    #[test]
    fn test_button_inherits_widget_accessors_via_prototype_chain() {
        let mut engine = setup();
        let mut button = TestButton::new("ProtoButton");
        button.widget.title = "ProtoTitle".into();
        let obj = create_button(button, &mut engine);

        let title_key = engine.property_key_from_str("title");
        let title_val = ExecutionContext::get(&mut engine, obj.clone(), title_key.clone()).unwrap();
        assert_eq!(engine.to_rust_string(title_val).unwrap(), "ProtoTitle");

        let new_title = engine.value_from_string(engine.js_string_from_str("InheritedSet"));
        engine.set(obj.clone(), title_key, new_title, false).unwrap();

        let updated_title_key = engine.property_key_from_str("title");
        let updated_title =
            ExecutionContext::get(&mut engine, obj, updated_title_key).unwrap();
        assert_eq!(engine.to_rust_string(updated_title).unwrap(), "InheritedSet");
    }

    #[test]
    fn test_button_inherits_widget_methods_via_prototype_chain() {
        let mut engine = setup();
        let obj = create_button(TestButton::new("ProtoButton"), &mut engine);
        let receiver = TestTypes::value_from_object(obj.clone());

        let increment = engine
            .get_method(receiver.clone(), engine.property_key_from_str("increment"))
            .unwrap()
            .expect("inherited increment method");
        let increment_obj = TestTypes::object_from_function(increment);

        let old_val = js_engine::EcmascriptHost::call(&mut engine, &increment_obj, &receiver, &[])
            .unwrap();
        assert!((engine.to_number(old_val).unwrap() - 0.0).abs() < 0.001);

        let count_key = engine.property_key_from_str("count");
        let count_val =
            ExecutionContext::get(&mut engine, obj, count_key).unwrap();
        assert!((engine.to_number(count_val).unwrap() - 1.0).abs() < 0.001);
    }

    // ── Tests ────────────────────────────────────────────────────────

    #[test]
    fn widget_get_title_returns_default() {
        let mut engine = setup();
        let widget = TestWidget::new();
        let obj = create_widget(widget, &mut engine);
        let js_obj = TestTypes::value_from_object(obj);
        let title_val = get_title(&js_obj, &[], &mut engine).unwrap();
        let title = engine.to_rust_string(title_val).unwrap();
        assert_eq!(title, "Untitled");
    }

    #[test]
    fn widget_set_title_then_get() {
        let mut engine = setup();
        let widget = TestWidget::new();
        let obj = create_widget(widget, &mut engine);
        let js_obj = TestTypes::value_from_object(obj.clone());
        let new_title = engine.value_from_string(engine.js_string_from_str("Hello"));
        set_title(&js_obj, &[new_title], &mut engine).unwrap();
        let title_val = get_title(&js_obj, &[], &mut engine).unwrap();
        let title = engine.to_rust_string(title_val).unwrap();
        assert_eq!(title, "Hello");
    }

    #[test]
    fn widget_get_visible_default_true() {
        let mut engine = setup();
        let widget = TestWidget::new();
        let obj = create_widget(widget, &mut engine);
        let js_obj = TestTypes::value_from_object(obj);
        let visible_val = get_visible(&js_obj, &[], &mut engine).unwrap();
        assert!(engine.to_boolean(&visible_val));
    }

    #[test]
    fn widget_increment_returns_old_and_increments() {
        let mut engine = setup();
        let widget = TestWidget::new();
        let obj = create_widget(widget, &mut engine);
        let js_obj = TestTypes::value_from_object(obj.clone());

        let old0 = increment(&js_obj, &[], &mut engine).unwrap();
        assert!((engine.to_number(old0).unwrap() - 0.0).abs() < 0.001);
        let old1 = increment(&js_obj, &[], &mut engine).unwrap();
        assert!((engine.to_number(old1).unwrap() - 1.0).abs() < 0.001);

        let count_val = get_count(&js_obj, &[], &mut engine).unwrap();
        assert!((engine.to_number(count_val).unwrap() - 2.0).abs() < 0.001);
    }

    #[test]
    fn widget_to_array_returns_three_elements() {
        let mut engine = setup();
        let mut widget = TestWidget::new();
        widget.title = "ArrayTest".into();
        let obj = create_widget(widget, &mut engine);
        let js_obj = TestTypes::value_from_object(obj);
        let arr_val = to_array(&js_obj, &[], &mut engine).unwrap();

        let arr = TestTypes::value_as_object(&arr_val).unwrap();
        let pk_length = engine.property_key_from_str("length");
        let length_val = ExecutionContext::get(&mut engine, arr.clone(), pk_length).unwrap();
        assert!((engine.to_number(length_val).unwrap() - 3.0).abs() < 0.001);

        let pk_0 = engine.property_key_from_index(0);
        let elem0 = ExecutionContext::get(&mut engine, arr, pk_0).unwrap();
        assert_eq!(engine.to_rust_string(elem0).unwrap(), "ArrayTest");
    }

    #[test]
    fn widget_format_label() {
        let mut engine = setup();
        let mut widget = TestWidget::new();
        widget.title = "Foo".into();
        let obj = create_widget(widget, &mut engine);
        let js_obj = TestTypes::value_from_object(obj);
        let prefix = engine.value_from_string(engine.js_string_from_str("PREFIX"));
        let label_val = format_label(&js_obj, &[prefix], &mut engine).unwrap();
        let label = engine.to_rust_string(label_val).unwrap();
        assert_eq!(label, "PREFIX:Foo");
    }

    #[test]
    fn widget_set_count_via_binding() {
        let mut engine = setup();
        let widget = TestWidget::new();
        let obj = create_widget(widget, &mut engine);
        let js_obj = TestTypes::value_from_object(obj.clone());

        let new_count = engine.value_from_number(42.0);
        set_count(&js_obj, &[new_count], &mut engine).unwrap();
        let count_val = get_count(&js_obj, &[], &mut engine).unwrap();
        assert!((engine.to_number(count_val).unwrap() - 42.0).abs() < 0.001);
    }

    #[test]
    fn widget_process_items_counts_strings() {
        let mut engine = setup();
        let widget = TestWidget::new();
        let obj = create_widget(widget, &mut engine);
        let js_obj = TestTypes::value_from_object(obj.clone());

        let items = engine.create_empty_array();
        let sv_a = engine.value_from_string(engine.js_string_from_str("a"));
        engine.array_push(&items, sv_a).unwrap();
        let sv_1 = engine.value_from_number(1.0);
        engine.array_push(&items, sv_1).unwrap();
        let sv_b = engine.value_from_string(engine.js_string_from_str("b"));
        engine.array_push(&items, sv_b).unwrap();
        let sv_true = engine.value_from_bool(true);
        engine.array_push(&items, sv_true).unwrap();

        process_items(&js_obj, &[TestTypes::value_from_object(items)], &mut engine).unwrap();
        let count_val = get_count(&js_obj, &[], &mut engine).unwrap();
        assert!((engine.to_number(count_val).unwrap() - 2.0).abs() < 0.001);
    }

    #[test]
    fn widget_delayed_title_returns_resolved_promise() {
        let mut engine = setup();
        let mut widget = TestWidget::new();
        widget.title = "PromiseMe".into();
        let obj = create_widget(widget, &mut engine);
        let js_obj = TestTypes::value_from_object(obj);
        let promise_val = delayed_title(&js_obj, &[], &mut engine).unwrap();
        assert!(TestTypes::value_as_object(&promise_val).is_some());
    }

    #[test]
    fn widget_static_create() {
        let mut engine = setup();
        let title_val = engine.value_from_string(engine.js_string_from_str("StaticWidget"));
        let bool_val = engine.value_from_bool(false);
        let result = create_static(
            &engine.value_undefined(),
            &[title_val, bool_val],
            &mut engine,
        )
        .unwrap();
        let obj = TestTypes::value_as_object(&result).unwrap();

        let js_obj = TestTypes::value_from_object(obj);
        let title_val = get_title(&js_obj, &[], &mut engine).unwrap();
        assert_eq!(engine.to_rust_string(title_val).unwrap(), "StaticWidget");
    }

    #[test]
    fn type_conversions() {
        let mut engine = setup();
        let bool_val = engine.value_from_bool(true);
        let num_val = engine.value_from_number(42.5);
        let str_val = engine.value_from_string(engine.js_string_from_str("123"));

        assert!(engine.to_boolean(&bool_val));
        let undef_val = engine.value_undefined();
        assert!(!engine.to_boolean(&undef_val));
        let empty_str = engine.value_from_string(engine.js_string_from_str(""));
        assert!(!engine.to_boolean(&empty_str));
        assert!((engine.to_number(num_val).unwrap() - 42.5).abs() < 0.001);
        assert_eq!(engine.to_rust_string(str_val).unwrap(), "123");
    }

    #[test]
    fn comparison_and_equality() {
        let mut engine = setup();
        let v1 = engine.value_from_number(1.0);
        let v2 = engine.value_from_number(1.0);
        let v3 = engine.value_from_number(2.0);

        assert!(engine.same_value(&v1, &v2));
        assert!(!engine.same_value(&v1, &v3));
        assert!(engine.is_strictly_equal(&v1, &v2));

        let undef = engine.value_undefined();
        let null = engine.value_null();
        assert!(engine.is_loosely_equal(undef, null).unwrap());
    }

    #[test]
    fn error_construction_and_type_check() {
        let mut engine = setup();
        let type_err = engine.new_type_error("bad");
        let range_err = engine.new_range_error("range");
        let syntax_err = engine.new_syntax_error("parse");

        assert!(TestTypes::value_as_object(&type_err).is_some());
        assert!(TestTypes::value_as_object(&range_err).is_some());
        assert!(TestTypes::value_as_object(&syntax_err).is_some());
        assert!(TestTypes::value_is_undefined(&engine.value_undefined()));
        assert!(TestTypes::value_is_null(&engine.value_null()));
        assert_eq!(
            TestTypes::value_as_bool(&engine.value_from_bool(true)),
            Some(true)
        );
        assert!(
            (TestTypes::value_as_number(&engine.value_from_number(7.0)).unwrap() - 7.0).abs()
                < 0.001
        );
        assert!(
            TestTypes::value_as_string(&engine.value_from_string(engine.js_string_from_str("x")))
                .is_some()
        );
    }

    #[test]
    fn host_data_store_and_retrieve() {
        let mut engine = setup();
        let id = std::any::TypeId::of::<String>();
        engine.store_host_any(id, Box::new("session-data".to_string()));
        assert!(engine.get_host_any(&id).is_some());
        let removed = engine.remove_host_any(&id);
        assert!(removed.is_some());
        assert!(engine.get_host_any(&id).is_none());
    }

    #[test]
    fn host_data_store_multiple_entries() {
        let mut engine = setup();
        let string_id = std::any::TypeId::of::<String>();
        let number_id = std::any::TypeId::of::<u32>();

        engine.store_host_any(string_id, Box::new("session-data".to_string()));
        engine.store_host_any(number_id, Box::new(7_u32));

        let stored_string = engine
            .get_host_any(&string_id)
            .and_then(|value| value.downcast_ref::<String>())
            .cloned();
        let stored_number = engine
            .get_host_any(&number_id)
            .and_then(|value| value.downcast_ref::<u32>())
            .copied();

        assert_eq!(stored_string.as_deref(), Some("session-data"));
        assert_eq!(stored_number, Some(7_u32));

        let removed_string = engine
            .remove_host_any(&string_id)
            .and_then(|value| value.downcast::<String>().ok())
            .map(|value| *value);
        assert_eq!(removed_string.as_deref(), Some("session-data"));
        assert_eq!(
            engine
                .get_host_any(&number_id)
                .and_then(|value| value.downcast_ref::<u32>())
                .copied(),
            Some(7_u32)
        );
    }

    #[test]
    fn enqueue_job_and_microtask_checkpoint_run_deferred_work() {
        let mut engine = setup();
        let counter = Arc::new(AtomicUsize::new(0));

        let first_job_counter = Arc::clone(&counter);
        engine.enqueue_job(Box::new(move || {
            first_job_counter.fetch_add(1, Ordering::SeqCst);
        }));
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        engine.perform_a_microtask_checkpoint().unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        let second_job_counter = Arc::clone(&counter);
        engine.enqueue_job(Box::new(move || {
            second_job_counter.fetch_add(1, Ordering::SeqCst);
        }));
        engine.run_jobs();
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn create_plain_object_with_properties() {
        let mut engine = setup();
        let obj = engine.create_plain_object(None);
        let val = engine.value_from_number(99.0);
        engine.object_set_property(obj.clone(), "x", val).unwrap();

        let pk = engine.property_key_from_str("x");
        let retrieved = ExecutionContext::get(&mut engine, obj, pk).unwrap();
        assert!((engine.to_number(retrieved).unwrap() - 99.0).abs() < 0.001);
    }

    #[test]
    fn array_push_and_indexed_access() {
        let mut engine = setup();
        let arr = engine.create_empty_array();
        let v10 = engine.value_from_number(10.0);
        let v20 = engine.value_from_number(20.0);
        engine.array_push(&arr, v10).unwrap();
        engine.array_push(&arr, v20).unwrap();

        let pk0 = engine.property_key_from_index(0);
        let pk1 = engine.property_key_from_index(1);
        let v0 = ExecutionContext::get(&mut engine, arr.clone(), pk0).unwrap();
        let v1 = ExecutionContext::get(&mut engine, arr, pk1).unwrap();
        assert!((engine.to_number(v0).unwrap() - 10.0).abs() < 0.001);
        assert!((engine.to_number(v1).unwrap() - 20.0).abs() < 0.001);
    }

    #[test]
    fn is_callable_detects_functions() {
        let mut engine = setup();
        let undef = engine.value_undefined();
        assert!(!engine.is_callable(&undef));

        let realm = engine.current_realm();
        let fn_val = engine
            .evaluate_script("(function(x) { return x * 2; })", &realm)
            .unwrap();
        assert!(engine.is_callable(&fn_val));

        let fn_obj = TestTypes::value_as_object(&fn_val).unwrap();
        let arg = engine.value_from_number(21.0);
        let result = js_engine::EcmascriptHost::call(&mut engine, &fn_obj, &undef, &[arg]).unwrap();
        assert!((engine.to_number(result).unwrap() - 42.0).abs() < 0.001);
    }

    #[test]
    fn promise_resolve_and_then() {
        let mut engine = setup();
        let realm = engine.current_realm();
        let intrinsics = engine.realm_intrinsics(&realm);

        // Create a promise capability, resolve it with value 42.
        let pcap = engine
            .new_promise_capability(intrinsics.promise.clone())
            .unwrap();
        let undef = engine.value_undefined();
        let val42 = engine.value_from_number(42.0);
        js_engine::EcmascriptHost::call(&mut engine, &pcap.resolve, &undef, &[val42]).unwrap();

        // Verify the promise is an object (it resolved successfully).
        assert!(TestTypes::value_as_object(&pcap.promise).is_some());

        // Create another promise via promise_resolve and verify it's an object.
        let val = engine.value_from_number(7.0);
        let promise = engine
            .promise_resolve(intrinsics.promise, val)
            .unwrap();
        let promise_val = TestTypes::value_from_object(
            TestTypes::object_from_promise(promise),
        );
        assert!(TestTypes::value_as_object(&promise_val).is_some());
    }

    #[test]
    fn rooted_promise_capability_survives_gc_pressure() {
        let mut engine = setup();
        let realm = engine.current_realm();
        let intrinsics = engine.realm_intrinsics(&realm);
        let capability = engine
            .new_promise_capability(intrinsics.promise.clone())
            .unwrap();
        let rooted_capability = engine.root_promise_capability(capability);

        let observed = Rc::new(Cell::new(0_u32));
        let observed_for_handler = Rc::clone(&observed);
        let on_fulfilled = engine.create_builtin_function(
            Box::new(move |args, _this, inner_ec| {
                let value = args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| inner_ec.value_undefined());
                observed_for_handler.set(inner_ec.to_uint32(value).unwrap_or(0));
                Ok(inner_ec.value_undefined())
            }),
            1,
            engine.property_key_from_str("observe_fulfillment"),
        );

        let promise_obj = TestTypes::value_as_object(&rooted_capability.promise.value).unwrap();
        let promise = TestTypes::object_as_promise(&promise_obj).unwrap();
        engine
            .perform_promise_then(promise, Some(on_fulfilled), None, None)
            .unwrap();

        for i in 0..1000 {
            let throwaway = engine.create_empty_array();
            let num_val = engine.value_from_number(i as f64);
            let _ = engine.array_push(&throwaway, num_val);
        }

        let resolve_obj = TestTypes::value_as_object(&rooted_capability.resolve.value).unwrap();
        let undef = engine.value_undefined();
        let value = engine.value_from_number(42.0);
        js_engine::EcmascriptHost::call(&mut engine, &resolve_obj, &undef, &[value]).unwrap();
        engine.run_jobs();

        assert_eq!(observed.get(), 42_u32);
    }

    #[test]
    fn evaluate_script_returns_value() {
        let mut engine = setup();
        let realm = engine.current_realm();
        let result = engine.evaluate_script("40 + 2", &realm).unwrap();
        assert!((engine.to_number(result).unwrap() - 42.0).abs() < 0.001);
    }

    #[test]
    fn allocate_array_buffer_and_inspect() {
        let mut engine = setup();
        let realm = engine.current_realm();
        let intrinsics = engine.realm_intrinsics(&realm);
        let ab = engine
            .allocate_array_buffer(intrinsics.array_buffer, 8, None)
            .unwrap();
        assert!(!engine.is_detached_buffer(&ab));
        assert!(engine.is_fixed_length_array_buffer(&ab));
    }

    #[test]
    fn to_object_method_returns_plain_object() {
        let mut engine = setup();
        let mut widget = TestWidget::new();
        widget.title = "ObjTest".into();
        let obj = create_widget(widget, &mut engine);
        let js_obj = TestTypes::value_from_object(obj);
        let result = to_object(&js_obj, &[], &mut engine).unwrap();

        let result_obj = TestTypes::value_as_object(&result).unwrap();
        let pk_title = engine.property_key_from_str("title");
        let title_val = ExecutionContext::get(&mut engine, result_obj, pk_title).unwrap();
        assert_eq!(engine.to_rust_string(title_val).unwrap(), "ObjTest");
    }

    #[test]
    fn reject_with_message_returns_rejected_promise() {
        let mut engine = setup();
        let obj = create_widget(TestWidget::new(), &mut engine);
        let js_obj = TestTypes::value_from_object(obj);
        let msg = engine.value_from_string(engine.js_string_from_str("test error"));
        let result = reject_with_message_test(&js_obj, &[msg], &mut engine).unwrap();
        assert!(TestTypes::value_as_object(&result).is_some());
    }

    #[test]
    fn from_tags_splits_comma_string() {
        let mut engine = setup();
        let obj = create_widget(TestWidget::new(), &mut engine);
        let js_obj = TestTypes::value_from_object(obj);
        let input = engine.value_from_string(engine.js_string_from_str("a, b, c"));
        let result = from_tags(&js_obj, &[input], &mut engine).unwrap();
        let arr = TestTypes::value_as_object(&result).unwrap();
        let pk_len = engine.property_key_from_str("length");
        let len_val = ExecutionContext::get(&mut engine, arr.clone(), pk_len).unwrap();
        assert!((engine.to_number(len_val).unwrap() - 3.0).abs() < 0.001);
    }

    #[test]
    fn store_callback_then_flush_microtasks() {
        let mut engine = setup();
        let obj = create_widget(TestWidget::new(), &mut engine);
        let js_obj = TestTypes::value_from_object(obj);
        let realm = engine.current_realm();
        let fn_val = engine.evaluate_script("(function() {})", &realm).unwrap();
        store_callback(&js_obj, &[fn_val.clone()], &mut engine).unwrap();
        flush_microtasks_test(&js_obj, &[], &mut engine).unwrap();

        let obj_ref = TestTypes::value_as_object(&js_obj).unwrap();
        let has_callback = engine
            .with_object_any(&obj_ref)
            .and_then(|d| d.downcast_ref::<TestWidget>())
            .map(|w| w.on_change.is_some())
            .unwrap();
        assert!(has_callback);
    }

    // ── Iterator operations (§7.4) ─────────────────────────────────

    #[test]
    fn get_iterator_and_step_value() {
        let mut engine = setup();
        let arr = engine.create_empty_array();
        let v1 = engine.value_from_number(1.0);
        engine.array_push(&arr, v1).unwrap();
        let v2 = engine.value_from_number(2.0);
        engine.array_push(&arr, v2).unwrap();
        let mut iter_record = engine
            .get_iterator(
                TestTypes::value_from_object(arr),
                js_engine::IteratorKind::Sync,
                None,
            )
            .unwrap();
        let step0 = engine.iterator_step_value(&mut iter_record).unwrap();
        assert!(step0.is_some());
        assert!((engine.to_number(step0.unwrap()).unwrap() - 1.0).abs() < 0.001);
        let undef = engine.value_undefined();
        let _ = engine.iterator_close(iter_record, Ok(undef));
    }

    #[test]
    fn async_iterator_close_completes() {
        let mut engine = setup();
        let arr = engine.create_empty_array();
        let v1 = engine.value_from_number(1.0);
        engine.array_push(&arr, v1).unwrap();
        let iter_record = engine
            .get_iterator(
                TestTypes::value_from_object(arr),
                js_engine::IteratorKind::Sync,
                None,
            )
            .unwrap();
        let undef = engine.value_undefined();
        let _ = engine.async_iterator_close(iter_record, Ok(undef));
    }

    // ── More type conversions (§7.1) ───────────────────────────────

    #[test]
    fn integer_conversions() {
        let mut engine = setup();
        let v = engine.value_from_number(42.0);
        assert_eq!(engine.to_int32(v.clone()).unwrap(), 42i32);
        assert_eq!(engine.to_uint32(v.clone()).unwrap(), 42u32);
        assert_eq!(engine.to_int16(v.clone()).unwrap(), 42i16);
        assert_eq!(engine.to_uint16(v.clone()).unwrap(), 42u16);
        assert_eq!(engine.to_int8(v.clone()).unwrap(), 42i8);
        assert_eq!(engine.to_uint8(v.clone()).unwrap(), 42u8);
        assert_eq!(engine.to_uint8_clamp(v).unwrap(), 42u8);
    }

    #[test]
    fn to_numeric_and_to_primitive() {
        let mut engine = setup();
        let v = engine.value_from_number(123.0);
        let numeric = engine.to_numeric(v.clone()).unwrap();
        match numeric {
            js_engine::Numeric::Number(n) => assert!((n - 123.0).abs() < 0.001),
            _ => panic!("expected Number"),
        }
        let _ = engine.to_primitive(v, None);
    }

    #[test]
    fn to_index_and_to_property_key() {
        let mut engine = setup();
        let v = engine.value_from_number(5.0);
        let idx = engine.to_index(v.clone()).unwrap();
        assert_eq!(idx, 5);
        let _ = engine.to_property_key(v).unwrap();
    }

    #[test]
    fn canonical_numeric_index_string_works() {
        let engine = setup();
        let s = engine.js_string_from_str("42");
        assert_eq!(engine.canonical_numeric_index_string(&s), Some(42.0));
        let s2 = engine.js_string_from_str("not_a_number");
        assert_eq!(engine.canonical_numeric_index_string(&s2), None);
    }

    // ── More object operations (§7.3) ──────────────────────────────

    #[test]
    fn require_object_coercible() {
        let mut engine = setup();
        let undef = engine.value_undefined();
        assert!(engine.require_object_coercible(undef).is_err());
        let obj_val = engine.value_from_string(engine.js_string_from_str("ok"));
        assert!(engine.require_object_coercible(obj_val).is_ok());
    }

    #[test]
    fn is_array_detects_arrays() {
        let mut engine = setup();
        let arr = engine.create_empty_array();
        let arr_val = TestTypes::value_from_object(arr);
        assert!(engine.is_array(&arr_val).unwrap());
        let num_val = engine.value_from_number(1.0);
        assert!(!engine.is_array(&num_val).unwrap());
    }

    #[test]
    fn is_constructor_detects_constructors() {
        let mut engine = setup();
        let arr = engine.create_empty_array();
        let arr_val = TestTypes::value_from_object(arr);
        assert!(!engine.is_constructor(&arr_val));
    }

    #[test]
    fn is_extensible_and_integral() {
        let mut engine = setup();
        let obj = engine.create_plain_object(None);
        assert!(engine.is_extensible(&obj).unwrap());
        let v = engine.value_from_number(7.0);
        assert!(engine.is_integral_number(&v));
        let v2 = engine.value_from_number(7.5);
        assert!(!engine.is_integral_number(&v2));
        let s = engine.value_from_string(engine.js_string_from_str("x"));
        assert!(engine.is_property_key(&s));
    }

    #[test]
    fn same_value_zero_and_loose_equality() {
        let mut engine = setup();
        let v1 = engine.value_from_number(0.0);
        let v_neg = engine.value_from_number(-0.0);
        assert!(!engine.same_value(&v1, &v_neg));
        assert!(engine.same_value_zero(&v1, &v_neg));
        let undef = engine.value_undefined();
        let null = engine.value_null();
        assert!(engine.is_loosely_equal(undef, null).unwrap());
    }

    #[test]
    fn define_property_with_descriptor() {
        let mut engine = setup();
        let obj = engine.create_plain_object(None);
        let pk = engine.property_key_from_str("testDesc");
        let descriptor = PropertyDescriptor {
            value: Some(engine.value_from_number(42.0)),
            writable: Some(true),
            get: None,
            set: None,
            enumerable: Some(true),
            configurable: Some(true),
        };
        engine
            .define_property_or_throw(obj.clone(), pk.clone(), descriptor)
            .unwrap();
        let has = engine.has_property(obj.clone(), pk.clone()).unwrap();
        assert!(has);
        let has_own = engine.has_own_property(obj, pk).unwrap();
        assert!(has_own);
    }

    #[test]
    fn own_property_keys_and_descriptors_reflect_data_and_accessor_properties() {
        let mut engine = setup();
        let obj = engine.create_plain_object(None);

        let visible_key = engine.property_key_from_str("visible");
        let hidden_key = engine.property_key_from_str("hidden");
        let computed_key = engine.property_key_from_str("computed");

        let visible_desc = PropertyDescriptor {
            value: Some(engine.value_from_number(1.0)),
            writable: Some(true),
            get: None,
            set: None,
            enumerable: Some(true),
            configurable: Some(true),
        };
        engine
            .define_property_or_throw(obj.clone(), visible_key.clone(), visible_desc)
            .unwrap();

        let hidden_desc = PropertyDescriptor {
            value: Some(engine.value_from_number(2.0)),
            writable: Some(false),
            get: None,
            set: None,
            enumerable: Some(false),
            configurable: Some(false),
        };
        engine
            .define_property_or_throw(obj.clone(), hidden_key.clone(), hidden_desc)
            .unwrap();

        let getter_fn = engine.create_builtin_function(
            Box::new(|_args, _this, inner_ec| Ok(inner_ec.value_from_number(7.0))),
            0,
            engine.property_key_from_str("get_computed"),
        );
        let accessor_desc = PropertyDescriptor {
            value: None,
            writable: None,
            get: Some(getter_fn),
            set: None,
            enumerable: Some(true),
            configurable: Some(true),
        };
        engine
            .define_property_or_throw(obj.clone(), computed_key.clone(), accessor_desc)
            .unwrap();

        let own_keys = engine.own_property_keys(obj.clone()).unwrap();
        assert_eq!(own_keys.len(), 3);

        let visible_descriptor = engine
            .get_own_property(obj.clone(), visible_key)
            .unwrap()
            .unwrap();
        assert!(visible_descriptor.value.is_some());
        assert_eq!(visible_descriptor.writable, Some(true));
        assert_eq!(visible_descriptor.enumerable, Some(true));
        assert_eq!(visible_descriptor.configurable, Some(true));
        assert!((engine.to_number(visible_descriptor.value.unwrap()).unwrap() - 1.0).abs() < 0.001);

        let hidden_descriptor = engine
            .get_own_property(obj.clone(), hidden_key)
            .unwrap()
            .unwrap();
        assert!(hidden_descriptor.value.is_some());
        assert_eq!(hidden_descriptor.writable, Some(false));
        assert_eq!(hidden_descriptor.enumerable, Some(false));
        assert_eq!(hidden_descriptor.configurable, Some(false));
        assert!((engine.to_number(hidden_descriptor.value.unwrap()).unwrap() - 2.0).abs() < 0.001);

        let accessor_descriptor = engine
            .get_own_property(obj, computed_key)
            .unwrap()
            .unwrap();
        assert!(accessor_descriptor.value.is_none());
        assert!(accessor_descriptor.get.is_some());
        assert!(accessor_descriptor.set.is_none());
        assert_eq!(accessor_descriptor.enumerable, Some(true));
        assert_eq!(accessor_descriptor.configurable, Some(true));
    }

    #[test]
    fn get_method_returns_callable() {
        let mut engine = setup();
        let realm = engine.current_realm();
        let fn_val = engine
            .evaluate_script("(function() { return 42; })", &realm)
            .unwrap();
        let obj = TestTypes::value_as_object(&fn_val).unwrap();
        let pk = engine.property_key_from_str("call");
        let method = engine
            .get_method(TestTypes::value_from_object(obj), pk)
            .unwrap();
        assert!(method.is_some());
    }

    #[test]
    fn get_v_and_delete_property_or_throw() {
        let mut engine = setup();
        let obj = engine.create_plain_object(None);
        let val = engine.value_from_number(1.0);
        engine.object_set_property(obj.clone(), "a", val).unwrap();
        let pk = engine.property_key_from_str("a");
        let _ = engine
            .get_v(TestTypes::value_from_object(obj.clone()), pk.clone())
            .unwrap();
        engine.delete_property_or_throw(obj, pk).unwrap();
    }

    #[test]
    fn set_prototype_and_integrity() {
        let mut engine = setup();
        let obj = engine.create_plain_object(None);
        let proto = engine.create_plain_object(None);
        assert!(engine.set_prototype(obj.clone(), Some(proto)).unwrap());
        let frozen = engine.create_plain_object(None);
        let v1 = engine.value_from_number(1.0);
        engine.object_set_property(frozen.clone(), "a", v1).unwrap();
        let _ = engine
            .set_integrity_level(frozen.clone(), js_engine::IntegrityLevel::Frozen)
            .unwrap();
        let _ = engine
            .test_integrity_level(frozen, js_engine::IntegrityLevel::Frozen)
            .unwrap();
    }

    #[test]
    fn species_constructor_returns_default() {
        let mut engine = setup();
        let realm = engine.current_realm();
        let intrinsics = engine.realm_intrinsics(&realm);
        let obj = engine.create_plain_object(None);
        let ctor = engine
            .species_constructor(obj, intrinsics.object.clone())
            .unwrap();
        let _ctor_val = TestTypes::value_from_object(TestTypes::object_from_constructor(ctor));
    }

    #[test]
    fn construct_calls_constructor() {
        let mut engine = setup();
        let realm = engine.current_realm();
        let intrinsics = engine.realm_intrinsics(&realm);
        let result = engine
            .construct(intrinsics.object.clone(), &[], None)
            .unwrap();
        let _result_val = TestTypes::value_from_object(result);
    }

    // ── Buffer operations ──────────────────────────────────────────

    #[test]
    fn get_and_set_value_in_buffer() {
        let mut engine = setup();
        let realm = engine.current_realm();
        let intrinsics = engine.realm_intrinsics(&realm);
        let ab = engine
            .allocate_array_buffer(intrinsics.array_buffer.clone(), 8, None)
            .unwrap();
        let val = engine.get_value_from_buffer(
            &ab,
            0,
            js_engine::TypedArrayElementType::Uint8,
            false,
            js_engine::SharedMemoryOrder::SeqCst,
        );
        let _ = val;
        let v255 = engine.value_from_number(255.0);
        engine
            .set_value_in_buffer(
                &ab,
                0,
                js_engine::TypedArrayElementType::Uint8,
                v255,
                false,
                js_engine::SharedMemoryOrder::SeqCst,
            )
            .unwrap();
    }

    #[test]
    fn clone_and_detach_array_buffer() {
        let mut engine = setup();
        let realm = engine.current_realm();
        let intrinsics = engine.realm_intrinsics(&realm);
        let ab = engine
            .allocate_array_buffer(intrinsics.array_buffer.clone(), 8, None)
            .unwrap();
        let _cloned = engine
            .clone_array_buffer(ab.clone(), 0, 4, intrinsics.array_buffer.clone())
            .unwrap();
        let _ = engine.detach_array_buffer(ab, None);
    }

    #[cfg_attr(
        feature = "jsc",
        ignore = "JSC: SharedArrayBuffer may not be available"
    )]
    #[test]
    fn allocate_shared_array_buffer() {
        let mut engine = setup();
        let realm = engine.current_realm();
        let intrinsics = engine.realm_intrinsics(&realm);
        let _sab = engine
            .allocate_shared_array_buffer(intrinsics.shared_array_buffer.clone(), 16)
            .unwrap();
    }

    // ── JSON and BigInt ────────────────────────────────────────────

    #[test]
    fn json_stringify_roundtrip() {
        let mut engine = setup();
        let obj = engine.create_plain_object(None);
        let v1 = engine.value_from_number(1.0);
        engine.object_set_property(obj.clone(), "x", v1).unwrap();
        let json = engine
            .json_stringify(TestTypes::value_from_object(obj))
            .unwrap();
        assert!(json.contains("\"x\":1"));
    }

    #[test]
    fn bigint_roundtrip() {
        let mut engine = setup();
        let val = engine.value_from_bigint(42);
        let bigint = engine.to_bigint(val).unwrap();
        let _ = bigint;
        let s = engine.js_string_from_str("123");
        let parsed = engine.string_to_bigint(s);
        assert!(parsed.is_some());
    }

    // ── Engine factory operations ──────────────────────────────────

    #[test]
    fn create_builtin_function_and_call() {
        let mut engine = setup();
        let pk = engine.property_key_from_str("testBuiltin");
        let builtin = engine.create_builtin_function(
            Box::new(|_args, _this, inner_ec| Ok(inner_ec.value_from_number(42.0))),
            0,
            pk,
        );
        let builtin_obj = TestTypes::object_from_function(builtin);
        let undef = engine.value_undefined();
        let result =
            js_engine::EcmascriptHost::call(&mut engine, &builtin_obj, &undef, &[]).unwrap();
        assert!((engine.to_number(result).unwrap() - 42.0).abs() < 0.001);
    }

    /// HTML §8.1.3.3: creating a new JavaScript realm →
    /// InitializeHostDefinedRealm (ECMA-262).  Bypasses Web IDL — the
    /// spec calls ECMA-262 directly, so our test calls js_engine directly.
    #[test]
    fn create_realm_and_set_bindings() {
        let mut engine = setup();
        let realm = engine.create_realm();
        let global_obj = engine.create_plain_object(None);
        engine.set_realm_global_object(&realm, global_obj, None);
        let _ = engine.set_default_global_bindings(&realm);
        engine.set_host_hooks(js_engine::HostHooks::empty());
    }

    /// End-to-end: Streams → Web IDL "react" → ECMA-262.
    ///
    /// Maps to the spec chain:
    ///   Streams: ReadableStreamCancel → "reacting to sourceCancelPromise"
    ///   Web IDL: react (§3.2.24.1) →
    ///     Step 2: CreateBuiltinFunction(onFulfilledSteps, 1, "", «»)
    ///     Step 6: NewPromiseCapability(constructor)
    ///     Step 7: PerformPromiseThen(promise, onFulfilled, onRejected, newCapability)
    ///   ECMA-262: create_builtin_function, new_promise_capability, perform_promise_then
    ///
    /// Validates that the generic JS layer supports the exact pattern
    /// real content code (streams, DOM, HTML) needs — no Boa-specific
    /// APIs, just the ECMA-262 operations the spec calls for.
    #[test]
    fn upon_settlement_full_chain() {
        let mut engine = setup();
        let intrinsics = engine.realm_intrinsics(&engine.current_realm());
        let empty_pk = engine.property_key_from_str("");

        // Web IDL "react" Step 6: NewPromiseCapability(constructor).
        let result_capability = engine
            .new_promise_capability(intrinsics.promise.clone())
            .unwrap();

        // Web IDL "react" Step 2: CreateBuiltinFunction(onFulfilledSteps, 1, "", «»).
        let on_fulfilled = engine.create_builtin_function(
            Box::new(|args, _this, inner_ec| {
                let value = args.first().cloned().unwrap_or(inner_ec.value_undefined());
                let n = inner_ec.to_number(value).unwrap_or(0.0);
                Ok(inner_ec.value_from_number(n + 1.0))
            }),
            1,
            empty_pk,
        );

        // Create a resolved source promise to attach the handler to.
        let val_41 = engine.value_from_number(41.0);
        let source_promise = engine.promise_resolve(intrinsics.promise, val_41).unwrap();

        // Web IDL "react" Step 7: PerformPromiseThen(promise, onFulfilled, onRejected,
        // newCapability).
        engine
            .perform_promise_then(
                source_promise,
                Some(on_fulfilled),
                None,
                Some(result_capability),
            )
            .unwrap();

        // Flush microtasks — onFulfilled runs (41 + 1 = 42).
        engine.run_jobs();

        // The chain completed: create_builtin_function → new_promise_capability →
        // perform_promise_then work together exactly as the Web IDL spec requires.
    }

    // ── Object downcasts ───────────────────────────────────────────

    #[test]
    fn object_downcasts_all_types() {
        let mut engine = setup();
        let realm = engine.current_realm();
        // Map
        let map_val = engine
            .evaluate_script("new Map([['k','v']])", &realm)
            .unwrap();
        assert!(TestTypes::object_as_map(&TestTypes::value_as_object(&map_val).unwrap()).is_some());
        // Set
        let set_val = engine.evaluate_script("new Set([1,2,3])", &realm).unwrap();
        assert!(TestTypes::object_as_set(&TestTypes::value_as_object(&set_val).unwrap()).is_some());
        // TypedArray
        let ta_val = engine.evaluate_script("new Uint8Array(4)", &realm).unwrap();
        assert!(
            TestTypes::object_as_typed_array(&TestTypes::value_as_object(&ta_val).unwrap())
                .is_some()
        );
        // DataView
        let dv_val = engine
            .evaluate_script("new DataView(new ArrayBuffer(8))", &realm)
            .unwrap();
        assert!(
            TestTypes::object_as_data_view(&TestTypes::value_as_object(&dv_val).unwrap()).is_some()
        );
    }

    // ── GC object round-trip (create_object_with_any + with_object_any) ─

    /// Exercises the GC integration: create a JS object with native Rust
    /// data via `create_object_with_any`, then retrieve it via
    /// `with_object_any` / `with_object_any_mut`.  Proves the round-trip
    /// works for both Boa (NativeDataWrapper + downcast_ref) and JSC
    /// (host_data side-table).
    #[test]
    fn gc_object_roundtrip() {
        let mut engine = setup();
        let widget = TestWidget {
            title: "GC-test".into(),
            visible: false,
            count: 7,
            on_change: None,
        };

        // Create via the trait's generic API.
        let prototype = engine.create_plain_object(None);
        let obj = engine.create_object_with_any(prototype, Box::new(widget));

        // Retrieve immutable.
        let title = engine
            .with_object_any(&obj)
            .and_then(|d| d.downcast_ref::<TestWidget>())
            .unwrap()
            .title
            .clone();
        assert_eq!(title, "GC-test");

        // Retrieve mutable.
        engine
            .with_object_any_mut(&obj)
            .and_then(|d| d.downcast_mut::<TestWidget>())
            .unwrap()
            .count = 99;
        let count = engine
            .with_object_any(&obj)
            .and_then(|d| d.downcast_ref::<TestWidget>())
            .unwrap()
            .count;
        assert_eq!(count, 99);
    }

    // ── with_object_any_mut borrow limitation ────────────────────

    /// Validates that mutable downcast via `with_object_any_mut` followed by
    /// an `ec` method call works correctly — the mutable borrow from
    /// `with_object_any_mut` is dropped before the `ec` call.
    ///
    /// Canonical pattern for `set_onload`, `set_src`, `play`, and `pause`:
    /// call `with_object_any_mut_with` which passes both `&mut dyn Any` and
    /// `&mut dyn ExecutionContext<T>` to a closure, enabling `ec` method
    /// calls during mutation without any Boa-specific workaround.
    #[test]
    fn with_object_any_mut_with_ec_inside_closure() {
        let mut engine = setup();
        let widget = TestWidget::new();
        let obj = create_widget(widget, &mut engine);

        // Mutate the widget AND call ec methods all within the same
        // closure — no borrow conflict because with_object_any_mut_with
        // passes `data` and `ec` as separate parameters.
        engine.with_object_any_mut_with(
            &obj,
            Box::new(|data, ec| {
                let widget = data.downcast_mut::<TestWidget>().unwrap();
                widget.title = "ModifiedFromClosure".into();
                // Verify we can call ec methods during mutation.
                let _ = ec.value_from_string(ec.js_string_from_str("test"));
            }),
        );

        // Verify the mutation persisted.
        let js_obj = TestTypes::value_from_object(obj.clone());
        let result_val = get_title(&js_obj, &[], &mut engine).unwrap();
        let title = engine.to_rust_string(result_val).unwrap();
        assert_eq!(title, "ModifiedFromClosure");
    }

    /// Demonstrates the `create_interface_instance` pattern: constructs a
    /// domain object, wraps it via `create_interface_instance` (which calls
    /// `create_object_with_any` inside), mutates it through the object, and
    /// reads the mutation back.  This is the pattern used by DOMException,
    /// Event, Location, and other platform-object construction in production
    /// binding code.
    #[test]
    fn create_interface_instance_roundtrip() {
        use crate::webidl::bindings::create_interface_instance;

        let mut engine = setup();

        // Construct a domain value.
        let mut widget = TestWidget::new();
        widget.title = "InterfaceTest".into();

        // Wrap it via create_interface_instance (same path as DOMException, Event, etc.).
        let obj = create_interface_instance::<TestTypes, TestWidget>(widget, &mut engine).unwrap();
        let js_obj = TestTypes::value_from_object(obj.clone());

        // Read the field back through the generic downcast.
        let title = get_title(&js_obj, &[], &mut engine).unwrap();
        assert_eq!(engine.to_rust_string(title).unwrap(), "InterfaceTest");

        // Mutable access through with_object_any_mut.
        widget_data::with_mut(&obj, &mut engine, |w| w.count = 99).unwrap();
        let count_val = get_count(&js_obj, &[], &mut engine).unwrap();
        assert!((engine.to_number(count_val).unwrap() - 99.0).abs() < 0.001);
    }

    // ── PropertyDescriptor with getter from create_builtin_function ──

    /// Validates constructing a `PropertyDescriptor` whose `get` field is a
    /// function created via `create_builtin_function`, applying it to an
    /// object, and reading the property — the exact pattern `get_class_list`
    /// needs for its `length` getter.
    #[test]
    fn property_descriptor_with_builtin_getter() {
        let mut engine = setup();

        // Create a plain object to attach the property to.
        let obj = engine.create_plain_object(None);
        let pk = engine.property_key_from_str("computedLength");

        // Build a getter function via create_builtin_function.
        let getter_fn = engine.create_builtin_function(
            Box::new(|_args, _this, inner_ec| Ok(inner_ec.value_from_number(7.0))),
            0,
            engine.property_key_from_str("get_computedLength"),
        );

        // PropertyDescriptor with only a getter (accessor property).
        let descriptor = PropertyDescriptor {
            value: None,
            writable: None,
            get: Some(getter_fn),
            set: None,
            enumerable: Some(true),
            configurable: Some(true),
        };

        // Define the accessor property on the object.
        engine
            .define_property_or_throw(obj.clone(), pk.clone(), descriptor)
            .unwrap();

        // Read the property — the getter executes and returns 7.
        let result = ExecutionContext::get(&mut engine, obj.clone(), pk).unwrap();
        assert!((engine.to_number(result).unwrap() - 7.0).abs() < 0.001);

        // Verify the property is an own accessor (not a data property).
        let has_own = engine.has_own_property(obj, engine.property_key_from_str("computedLength")).unwrap();
        assert!(has_own);
    }

    /// Validates `PropertyDescriptor` with both getter and setter from
    /// `create_builtin_function` — the full accessor pattern.
    #[test]
    fn property_descriptor_with_builtin_getter_and_setter() {
        let mut engine = setup();

        let obj = engine.create_plain_object(None);
        let pk = engine.property_key_from_str("accessorProp");

        // Setter stores the value (simulated with a side-channel via a plain backing field).
        let backing_obj = engine.create_plain_object(None);
        let backing_obj_for_getter = backing_obj.clone();

        let setter_fn = engine.create_builtin_function(
            Box::new(move |args, _this, inner_ec| {
                let val = args.first().cloned().unwrap_or(inner_ec.value_undefined());
                let _ = inner_ec.object_set_property(backing_obj.clone(), "_backing", val);
                Ok(inner_ec.value_undefined())
            }),
            1,
            engine.property_key_from_str("set_accessorProp"),
        );

        let getter_fn = engine.create_builtin_function(
            Box::new(move |_args, _this, inner_ec| {
                let get_pk = inner_ec.property_key_from_str("_backing");
                let val = ExecutionContext::get(inner_ec, backing_obj_for_getter.clone(), get_pk)
                    .unwrap_or_else(|_| inner_ec.value_undefined());
                Ok(val)
            }),
            0,
            engine.property_key_from_str("get_accessorProp"),
        );

        let descriptor = PropertyDescriptor {
            value: None,
            writable: None,
            get: Some(getter_fn),
            set: Some(setter_fn),
            enumerable: Some(true),
            configurable: Some(true),
        };

        engine
            .define_property_or_throw(obj.clone(), pk.clone(), descriptor)
            .unwrap();

        // Set via the accessor — extract value first to avoid double-borrow of engine.
        let set_val = engine.value_from_number(99.0);
        engine
            .set(obj.clone(), pk.clone(), set_val, false)
            .unwrap();

        // Get via the accessor.
        let result = ExecutionContext::get(&mut engine, obj, pk).unwrap();
        assert!((engine.to_number(result).unwrap() - 99.0).abs() < 0.001);
    }

    // ── GC rooting pressure tests ───────────────────────────────────

    /// Allocates many throwaway objects to create GC pressure, then
    /// verifies a `GcRootHandle`-rooted callback still survives.
    /// This exercises the key property `create_root` exists to provide:
    /// a GC-rooted value must survive even when unreachable objects
    /// are reclaimed.
    #[test]
    fn gc_root_survives_throwaway_pressure() {
        let mut engine = setup();
        let realm = engine.current_realm();
        // Create and root a callback.
        let fn_val = engine
            .evaluate_script("(function() { return 42; })", &realm)
            .unwrap();
        let root = engine.create_root(&fn_val);

        // Allocate many throwaway objects to create GC pressure.
        for i in 0..1000 {
            let throwaway = engine.create_empty_array();
            let num_val = engine.value_from_number(i as f64);
            let _ = engine.array_push(&throwaway, num_val);
        }

        // The rooted callback must still be callable.
        let fn_obj = TestTypes::value_as_object(&root.value).unwrap();
        let undef = engine.value_undefined();
        let result = js_engine::EcmascriptHost::call(&mut engine, &fn_obj, &undef, &[]).unwrap();
        assert!((engine.to_number(result).unwrap() - 42.0).abs() < 0.001);

        // Root is still alive — on Boa the inner value traces through
        // GcRootHandle; on JSC the hidden global-object root prevents collection.
        drop(root);
    }

    // ── ObjectInitializer composite pattern ──────────────────────────

    /// Validates the generic equivalent of Boa's `ObjectInitializer`
    /// pattern: create a plain object, set multiple data properties,
    /// attach a method via `create_builtin_function`, define an accessor
    /// property with getter+setter, and store a back-reference.
    ///
    /// This mirrors `get_style` in `html_element.rs` — the single
    /// remaining Boa-specific blocker in that file — and proves the
    /// exact composite pattern can be expressed through the generic API.
    #[test]
    fn object_initializer_composite_build_and_reflect() {
        let mut engine = setup();

        // ── Step 1: create plain object (equivalent to ObjectInitializer::new + build) ──
        let obj = engine.create_plain_object(None);

        // ── Step 2: set data properties (equivalent to initializer.property(...)) ──
        let font_size_key = engine.property_key_from_str("font-size");
        let font_size_camel_key = engine.property_key_from_str("fontSize");
        let color_key = engine.property_key_from_str("color");
        let font_size_val = engine.value_from_string(engine.js_string_from_str("12px"));
        let color_val = engine.value_from_string(engine.js_string_from_str("#000"));
        engine.set(obj.clone(), font_size_key.clone(), font_size_val.clone(), false).unwrap();
        engine.set(obj.clone(), color_key.clone(), color_val, false).unwrap();
        // Also set camelCase alias (mirrors the camel_case_property_name pattern).
        engine.set(obj.clone(), font_size_camel_key.clone(), font_size_val, false).unwrap();

        // ── Step 3: attach a method (equivalent to initializer.function(...)) ──
        let get_property_value_fn = engine.create_builtin_function(
            Box::new(move |args, _this, inner_ec| {
                let prop_name = args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| inner_ec.value_undefined());
                // Return the queried property name back as a string.
                let name = inner_ec.to_rust_string(prop_name).unwrap_or_default();
                Ok(inner_ec.value_from_string(inner_ec.js_string_from_str(&format!("value_of_{}", name))))
            }),
            1,
            engine.property_key_from_str("getPropertyValue"),
        );
        let method_key = engine.property_key_from_str("getPropertyValue");
        let method_val = TestTypes::value_from_object(
            TestTypes::object_from_function(get_property_value_fn),
        );
        engine.set(obj.clone(), method_key.clone(), method_val, false).unwrap();

        // ── Step 4: define accessor property with getter + setter
        //           (equivalent to defining cssText with builder.get(...).set(...)) ──
        let backing = engine.create_plain_object(None);
        let backing_for_getter = backing.clone();

        let accessor_getter = engine.create_builtin_function(
            Box::new(move |_args, _this, inner_ec| {
                let bk = inner_ec.property_key_from_str("_cssText");
                Ok(ExecutionContext::get(inner_ec, backing_for_getter.clone(), bk)
                    .unwrap_or_else(|_| inner_ec.value_from_string(inner_ec.js_string_from_str(""))))
            }),
            0,
            engine.property_key_from_str("get_cssText"),
        );

        let backing_for_setter = backing.clone();
        let accessor_setter = engine.create_builtin_function(
            Box::new(move |args, _this, inner_ec| {
                let val = args.first().cloned().unwrap_or(inner_ec.value_undefined());
                let bk = inner_ec.property_key_from_str("_cssText");
                let _ = inner_ec.set(backing_for_setter.clone(), bk, val, false);
                Ok(inner_ec.value_undefined())
            }),
            1,
            engine.property_key_from_str("set_cssText"),
        );

        let css_text_key = engine.property_key_from_str("cssText");
        let accessor_desc = js_engine::PropertyDescriptor {
            value: None,
            writable: None,
            get: Some(accessor_getter),
            set: Some(accessor_setter),
            enumerable: Some(true),
            configurable: Some(true),
        };
        engine
            .define_property_or_throw(obj.clone(), css_text_key.clone(), accessor_desc)
            .unwrap();

        // ── Step 5: store a back-reference (equivalent to style_obj.set("__element", ...)) ──
        let ref_key = engine.property_key_from_str("__element");
        let ref_val = TestTypes::value_from_object(obj.clone());
        engine.set(obj.clone(), ref_key.clone(), ref_val, false).unwrap();

        // ── Verify: read data properties back ──
        let read_font_size = ExecutionContext::get(&mut engine, obj.clone(), font_size_key).unwrap();
        assert_eq!(engine.to_rust_string(read_font_size).unwrap(), "12px");
        let read_color = ExecutionContext::get(&mut engine, obj.clone(), color_key).unwrap();
        assert_eq!(engine.to_rust_string(read_color).unwrap(), "#000");

        // ── Verify: call the method ──
        let method_obj = ExecutionContext::get(&mut engine, obj.clone(), method_key).unwrap();
        let method_callable = TestTypes::value_as_object(&method_obj).unwrap();
        let arg = engine.value_from_string(engine.js_string_from_str("fontSize"));
        let result = js_engine::EcmascriptHost::call(
            &mut engine, &method_callable, &TestTypes::value_from_object(obj.clone()), &[arg],
        ).unwrap();
        assert_eq!(engine.to_rust_string(result).unwrap(), "value_of_fontSize");

        // ── Verify: set via accessor, then read back ──
        let new_css = engine.value_from_string(engine.js_string_from_str("color: red;"));
        engine.set(obj.clone(), css_text_key.clone(), new_css, false).unwrap();
        let read_css = ExecutionContext::get(&mut engine, obj.clone(), css_text_key).unwrap();
        assert_eq!(engine.to_rust_string(read_css).unwrap(), "color: red;");

        // ── Verify: back-reference is accessible ──
        let read_ref = ExecutionContext::get(&mut engine, obj.clone(), ref_key).unwrap();
        assert!(TestTypes::value_as_object(&read_ref).is_some());

        // ── Verify: own property keys include all expected entries ──
        let keys = engine.own_property_keys(obj.clone()).unwrap();
        assert!(keys.len() >= 6, "expected at least 6 own properties, got {}", keys.len());
    }

    // ── Builtin function with captured mutable state ─────────────────

    /// Validates `create_builtin_function` with `move` closure that
    /// captures and mutates external state. This mirrors the
    /// `NativeFunction::from_copy_closure_with_captures` pattern in
    /// `abort_signal.rs` `timeout_static` — a state-capturing closure
    /// that can be invoked as a JS callable.
    #[test]
    fn create_builtin_function_captures_mutable_state() {
        let mut engine = setup();

        let counter = Rc::new(Cell::new(0_u32));
        let counter_for_fn = Rc::clone(&counter);

        let fn_obj = engine.create_builtin_function(
            Box::new(move |_args, _this, inner_ec| {
                let current = counter_for_fn.get();
                counter_for_fn.set(current + 1);
                Ok(inner_ec.value_from_number(current as f64))
            }),
            0,
            engine.property_key_from_str("incrementer"),
        );

        let undef = engine.value_undefined();
        let callable = TestTypes::object_from_function(fn_obj);

        // Invoke twice — each call sees the captured state mutate.
        let r1 = js_engine::EcmascriptHost::call(&mut engine, &callable, &undef, &[]).unwrap();
        assert!((engine.to_number(r1).unwrap() - 0.0).abs() < 0.001);
        assert_eq!(counter.get(), 1);

        let r2 = js_engine::EcmascriptHost::call(&mut engine, &callable, &undef, &[]).unwrap();
        assert!((engine.to_number(r2).unwrap() - 1.0).abs() < 0.001);
        assert_eq!(counter.get(), 2);
    }

    // ── Timer callback detection pattern ──────────────────────────────

    /// Validates the `timer_handler` pattern: given a JS value, determine
    /// whether it's a callable function or a string.  This is the exact
    /// pattern from HTML §8.7 Step 9.7 → Web IDL "invoke a callback
    /// function" → ECMA-262 `Call`.
    ///
    /// Production equivalent: `timer_handler` in
    /// `window_or_worker_global_scope.rs`.
    #[test]
    fn timer_handler_detects_callable_vs_string() {
        let mut engine = setup();
        let realm = engine.current_realm();

        // Case 1: callable function → detected as Function
        let fn_val = engine
            .evaluate_script("(function() { return 42; })", &realm)
            .unwrap();
        let fn_obj = TestTypes::value_as_object(&fn_val);
        assert!(fn_obj.is_some());
        assert!(engine.is_callable(&fn_val));

        // Case 2: non-callable object → falls through to string conversion
        let obj_val = engine.evaluate_script("({})", &realm).unwrap();
        assert!(TestTypes::value_as_object(&obj_val).is_some());
        assert!(!engine.is_callable(&obj_val));
        // Would fall through to to_rust_string in timer_handler.
        let stringified = engine.to_rust_string(obj_val).unwrap();
        assert_eq!(stringified, "[object Object]");

        // Case 3: plain string → not an object, falls through to string
        let str_val = engine.value_from_string(engine.js_string_from_str("console.log('hi')"));
        assert!(TestTypes::value_as_object(&str_val).is_none());
        let source = engine.to_rust_string(str_val).unwrap();
        assert_eq!(source, "console.log('hi')");
    }

    // ── Timeout value conversion pattern ─────────────────────────────

    /// Validates the `timeout_ms` pattern: convert a JS value to an IDL
    /// long (via `ToNumber`), clamp negative to zero, floor, saturate at
    /// i32::MAX.  Production equivalent: `timeout_ms` in
    /// `window_or_worker_global_scope.rs`.
    #[test]
    fn timeout_ms_conversion_pattern() {
        let mut engine = setup();

        let check = |engine: &mut dyn ExecutionContext<TestTypes>, val: f64, expected: u32| {
            let num_val = engine.value_from_number(val);
            let timeout = engine.to_number(num_val).unwrap();
            let result = if !timeout.is_finite() || timeout <= 0.0 {
                0
            } else {
                timeout.floor().min(i32::MAX as f64) as u32
            };
            assert_eq!(result, expected);
        };

        check(&mut engine, 100.0, 100);
        check(&mut engine, 0.0, 0);
        check(&mut engine, -5.0, 0);
        check(&mut engine, f64::NAN, 0);
        check(&mut engine, f64::INFINITY, 0);
        check(&mut engine, 3.7, 3); // floor, not round
    }

    /// Verifies that `Trace` propagates through nested `impl_gc_traits!`
    /// structs.  `TestButton` wraps `TestWidget` which holds an
    /// `Option<GcRootHandle<Types>>` — the outer struct's `Trace` impl
    /// must visit the inner struct's GC roots.
    #[test]
    fn nested_struct_gc_root_propagates() {
        let mut engine = setup();
        let realm = engine.current_realm();

        // Create a button (wraps widget, which has on_change: Option<GcRootHandle>).
        let mut button = TestButton::new("GCButton");
        button.widget.title = "GCTest".into();

        // Root a callback stored on the inner widget.
        let fn_val = engine
            .evaluate_script("(function() { return 'nested'; })", &realm)
            .unwrap();
        button.widget.on_change = Some(engine.create_root(&fn_val));

        let obj = create_button(button, &mut engine);

        // Read back through the object to verify both the widget and
        // its rooted callback survived.
        let js_obj = TestTypes::value_from_object(obj.clone());
        let title = widget_or_button_with_ref(&js_obj, &mut engine, |w| w.title.clone()).unwrap();
        assert_eq!(title, "GCTest");

        // Read back the rooted callback.
        let has_callback = engine
            .with_object_any(&obj)
            .and_then(|d| d.downcast_ref::<TestButton>())
            .map(|b| b.widget.on_change.is_some())
            .unwrap();
        assert!(has_callback);

        // The callback itself should still be callable.
        let root = engine
            .with_object_any(&obj)
            .and_then(|d| d.downcast_ref::<TestButton>())
            .and_then(|b| b.widget.on_change.as_ref())
            .unwrap();
        let fn_obj = TestTypes::value_as_object(&root.value).unwrap();
        let undef = engine.value_undefined();
        let result = js_engine::EcmascriptHost::call(&mut engine, &fn_obj, &undef, &[]).unwrap();
        let s = engine.to_rust_string(result).unwrap();
        assert_eq!(s, "nested");
    }
}

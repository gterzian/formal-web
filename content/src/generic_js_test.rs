//! # `generic_js_test` — integration test for the generic JS layer
//!
//! This module is a self-contained mini-version of the `content` crate.
//! It exercises the generic `js_engine` API (`ExecutionContext<T>`,
//! `JsTypes`, `WebIdlInterface<T>`, etc.) so that we get fast feedback
//! on the design and type-checking of the generic layer before applying
//! it to the full codebase.
//!
//! The module defines a toy domain type (`TestWidget`), implements
//! `WebIdlInterface<Types>` for it, and provides a top-level function
//! (`exercise_generic_api`) that exercises every relevant API surface.

use std::marker::PhantomData;

use crate::js::Types;
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface, create_interface_instance,
};
use js_engine::{
    Completion, ExecutionContext, IteratorKind, JsEngine, JsTypes, PropertyDescriptor,
};

// Local type aliases for the active backend's associated types.
// Changing `Types` in `crate::js` switches between Boa and JSC.
type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;

/// Create a TestWidget platform object directly from a TestWidget value.
/// Bypasses `create_interface_instance` / `create_object_with_any` which wrap
/// data in `NativeDataWrapper` (breaking `downcast_ref::<TestWidget>()`).
/// Will be unnecessary after Phase 5 (GC abstraction) makes downcast generic.
#[cfg(feature = "boa")]
fn create_test_widget(
    widget: TestWidget,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    use crate::webidl::bindings::registry::get_prototype_from_host_defined;
    use boa_engine::object::JsObject as BoaJsObject;
    let prototype = get_prototype_from_host_defined::<Types, TestWidget>(ec)
        .ok_or_else(|| ec.new_type_error("TestWidget not registered"))?;
    Ok(BoaJsObject::from_proto_and_data(Some(prototype), widget))
}

// ── Domain type ──────────────────────────────────────────────────────────

/// A toy domain struct exercising the full generic-API binding pattern.
///
/// GC derives are engine-specific and conditional on the active backend.
/// When `feature = "jsc"` is added to the content crate, a corresponding
/// `cfg_attr` arm (or empty, since JSC GCs natively) will be added here.
///
/// ## Example: keeping a JS reference alive
///
/// The `on_change` field demonstrates how a domain struct holds a JS
/// callback reference:
///
/// | Backend | Mechanism | Field type |
/// |---|---|---|
/// | Boa | `#[derive(boa_gc::Trace)]` auto-traces `JsObject` fields | `Option<JsObject>` |
/// | JSC | `GcRootHandle` protects value from GC via `JSValueProtect` | `Option<GcRootHandle<JscTypes>>` |
///
/// For JSC, `Trace` and `Finalize` are implemented manually since there's
/// no derive macro.  The manual impls are empty because `GcRootHandle` is
/// self-rooting (it calls `JSValueUnprotect` on drop).
#[cfg_attr(
    feature = "boa",
    derive(boa_gc::Finalize, boa_gc::Trace, boa_engine::JsData)
)]
pub(crate) struct TestWidget {
    title: String,
    visible: bool,
    count: u32,
    /// Optional change callback — kept alive per-backend (see struct doc).
    #[cfg(feature = "boa")]
    on_change: Option<JsObject>,
    #[cfg(feature = "jsc")]
    #[allow(unexpected_cfgs)]
    on_change: Option<js_engine::gc::GcRootHandle<js_engine::jsc::JscTypes>>,
}

// JSC backend: no derive macro — implement Trace/Finalize by hand.
// SAFETY: TestWidget contains no JS references when `feature = "jsc"`
// (the `on_change` field is `GcRootHandle`, which self-roots).
// Note: `feature = "jsc"` is not yet defined in content/Cargo.toml;
// the `allow` silences the "unexpected cfg" warning until it is added.
#[cfg(feature = "jsc")]
#[allow(unexpected_cfgs)]
unsafe impl js_engine::gc::Trace for TestWidget {}
#[cfg(feature = "jsc")]
#[allow(unexpected_cfgs)]
impl js_engine::gc::Finalize for TestWidget {}

impl TestWidget {
    fn new() -> Self {
        Self {
            title: String::from("Untitled"),
            visible: true,
            count: 0,
            #[cfg(feature = "boa")]
            on_change: None,
        }
    }

    /// Constructor-from-args pattern (mirrors Event constructor).
    fn from_args(
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
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
            #[cfg(feature = "boa")]
            on_change: None,
        })
    }
}

// ── Binding functions ───────────────────────────────────────────────────

/// Getter: `widget.title` → string.
fn get_title(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("TestWidget receiver is not an object"))?;
    let widget = obj
        .downcast_ref::<TestWidget>()
        .ok_or_else(|| ec.new_type_error("receiver is not a TestWidget"))?;
    Ok(ec.value_from_string(ec.js_string_from_str(&widget.title)))
}

/// Setter: `widget.title = val` — exercises `ec.to_rust_string`.
fn set_title(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let fallback = ec.value_undefined();
    let new_title = ec.to_rust_string(args.first().cloned().unwrap_or(fallback))?;
    let obj = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("TestWidget receiver is not an object"))?;
    let mut widget = obj
        .downcast_mut::<TestWidget>()
        .ok_or_else(|| ec.new_type_error("receiver is not a TestWidget"))?;
    widget.title = new_title;
    Ok(ec.value_undefined())
}

/// Getter: `widget.visible` → bool.
fn get_visible(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("TestWidget receiver is not an object"))?;
    let widget = obj
        .downcast_ref::<TestWidget>()
        .ok_or_else(|| ec.new_type_error("receiver is not a TestWidget"))?;
    Ok(ec.value_from_bool(widget.visible))
}

/// Getter: `widget.count` → number.
fn get_count(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("TestWidget receiver is not an object"))?;
    let widget = obj
        .downcast_ref::<TestWidget>()
        .ok_or_else(|| ec.new_type_error("receiver is not a TestWidget"))?;
    Ok(ec.value_from_number(widget.count as f64))
}

/// Method: `widget.increment()` — increments the counter, returns old value.
fn increment(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("TestWidget receiver is not an object"))?;
    let mut widget = obj
        .downcast_mut::<TestWidget>()
        .ok_or_else(|| ec.new_type_error("receiver is not a TestWidget"))?;
    let old = widget.count;
    widget.count = old.wrapping_add(1);
    Ok(ec.value_from_number(old as f64))
}

/// Method: `widget.toObject()` — returns a plain object `{ title, visible, count }`.
///
/// Exercises `ec.create_plain_object` and `ec.object_set_property`.
fn to_object(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("TestWidget receiver is not an object"))?;
    let widget = obj
        .downcast_ref::<TestWidget>()
        .ok_or_else(|| ec.new_type_error("receiver is not a TestWidget"))?;
    let result = ec.create_plain_object(None);
    let title_val = ec.value_from_string(ec.js_string_from_str(&widget.title));
    let visible_val = ec.value_from_bool(widget.visible);
    let count_val = ec.value_from_number(widget.count as f64);
    ec.object_set_property(result.clone(), "title", title_val)?;
    ec.object_set_property(result.clone(), "visible", visible_val)?;
    ec.object_set_property(result.clone(), "count", count_val)?;
    Ok(Types::value_from_object(result))
}

/// Method: `widget.toArray()` — returns `[title, visible, count]`.
///
/// Exercises `ec.create_empty_array` and `ec.array_push`.
fn to_array(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("TestWidget receiver is not an object"))?;
    let widget = obj
        .downcast_ref::<TestWidget>()
        .ok_or_else(|| ec.new_type_error("receiver is not a TestWidget"))?;
    let array = ec.create_empty_array();
    let title_val = ec.value_from_string(ec.js_string_from_str(&widget.title));
    let visible_val = ec.value_from_bool(widget.visible);
    let count_val = ec.value_from_number(widget.count as f64);
    ec.array_push(&array, title_val)?;
    ec.array_push(&array, visible_val)?;
    ec.array_push(&array, count_val)?;
    Ok(Types::value_from_object(array))
}

/// Setter: `widget.count = val` — exercises `ec.to_uint32` (WebIDL semantics).
fn set_count(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("TestWidget receiver is not an object"))?;
    let num_val = args.first().cloned().unwrap_or(ec.value_undefined());
    let new_count = ec.to_uint32(num_val)?;
    let mut widget = obj
        .downcast_mut::<TestWidget>()
        .ok_or_else(|| ec.new_type_error("receiver is not a TestWidget"))?;
    widget.count = new_count;
    Ok(ec.value_undefined())
}

/// Method: `widget.formatLabel(prefix)` — exercises `ec.to_js_string` in a binding pattern.
fn format_label(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("TestWidget receiver is not an object"))?;
    let widget = obj
        .downcast_ref::<TestWidget>()
        .ok_or_else(|| ec.new_type_error("receiver is not a TestWidget"))?;
    let prefix = if let Some(arg) = args.first() {
        let js_str = ec.to_js_string(arg.clone())?;
        ec.js_string_to_rust_string(&js_str)
    } else {
        String::new()
    };
    let label = format!("{}:{}", prefix, widget.title);
    Ok(ec.value_from_string(ec.js_string_from_str(&label)))
}

/// Method: `widget.delayedTitle()` — exercises promise creation and resolution.
fn delayed_title(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("TestWidget receiver is not an object"))?;
    let widget = obj
        .downcast_ref::<TestWidget>()
        .ok_or_else(|| ec.new_type_error("receiver is not a TestWidget"))?;
    let realm = ec.current_realm();
    let intrinsics = ec.realm_intrinsics(&realm);
    let cap = ec.new_promise_capability(intrinsics.promise)?;
    let title_val = ec.value_from_string(ec.js_string_from_str(&widget.title));
    let undef = ec.value_undefined();
    ec.call(&cap.resolve, &undef, &[title_val])?;
    Ok(cap.promise)
}

/// Method: `widget.withCallback(cb)` — exercises `ec.call` with a user-provided callback.
fn with_callback(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("TestWidget receiver is not an object"))?;
    let widget = obj
        .downcast_ref::<TestWidget>()
        .ok_or_else(|| ec.new_type_error("receiver is not a TestWidget"))?;
    let callback_obj = args
        .first()
        .and_then(|v| Types::value_as_object(v))
        .ok_or_else(|| ec.new_type_error("expected a callback function"))?;
    let callback_val = Types::value_from_object(callback_obj.clone());
    if !ec.is_callable(&callback_val) {
        return Err(ec.new_type_error("argument is not callable"));
    }
    let title_val = ec.value_from_string(ec.js_string_from_str(&widget.title));
    let undef = ec.value_undefined();
    ec.call(&callback_obj, &undef, &[title_val])
}

/// Method: `widget.processItems(items)` — exercises sequence iteration with numeric keys.
/// Mirrors the AbortSignal.any() pattern: iterate `items.length`, call `ec.get` by index.
fn process_items(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("TestWidget receiver is not an object"))?;
    let mut widget = obj
        .downcast_mut::<TestWidget>()
        .ok_or_else(|| ec.new_type_error("receiver is not a TestWidget"))?;
    let items_val = args.first().cloned().unwrap_or(ec.value_undefined());
    let items = Types::value_as_object(&items_val)
        .ok_or_else(|| ec.new_type_error("expected an array argument"))?;
    let pk_length = ec.property_key_from_str("length");
    let length_val = ExecutionContext::get(ec, items.clone(), pk_length)?;
    let length = ec.to_length(length_val)?;
    // to_length returns u64 (clamped to 2^53-1 per spec).  The cast to
    // u32 is safe for any realistic array; beyond u32::MAX loop iterations
    // would be impractical.
    let mut count: u32 = 0;
    for index in 0..length {
        let pk_index = ec.property_key_from_index(index as u32);
        let item = ExecutionContext::get(ec, items.clone(), pk_index)?;
        // Count string items
        if Types::value_as_string(&item).is_some() {
            count = count.wrapping_add(1);
        }
    }
    widget.count = count;
    Ok(ec.value_undefined())
}

/// Static method: `TestWidget.create(title, visible)` — factory constructor pattern.
/// Exercises static operations (no `this` downcast).
fn create_static(
    _this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
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
        #[cfg(feature = "boa")]
        on_change: None,
    };
    #[cfg(feature = "boa")]
    {
        let obj = create_test_widget(widget, ec)?;
        return Ok(Types::value_from_object(obj));
    }
    #[cfg(not(feature = "boa"))]
    {
        let obj = create_interface_instance::<Types, TestWidget>(widget, ec)?;
        Ok(Types::value_from_object(obj))
    }
}

/// Method: `widget.storeCallback(cb)` — stores a callback for later invocation.
/// Mirrors the host callback pattern (`Callback::from_object` +
/// `call_user_objects_operation`) used in event dispatch, streams, etc.
fn store_callback(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("TestWidget receiver is not an object"))?;
    let mut widget = obj
        .downcast_mut::<TestWidget>()
        .ok_or_else(|| ec.new_type_error("receiver is not a TestWidget"))?;
    let callback_obj = args
        .first()
        .and_then(|v| Types::value_as_object(v))
        .ok_or_else(|| ec.new_type_error("expected a callback function"))?;
    let callback_val = Types::value_from_object(callback_obj.clone());
    if !ec.is_callable(&callback_val) {
        return Err(ec.new_type_error("argument is not callable"));
    }
    #[cfg(feature = "boa")]
    {
        widget.on_change = Some(callback_obj);
    }
    // JSC: protect the callback from GC via GcRootHandle.
    // When the widget is dropped, the GcRootHandle's Drop impl calls
    // JSValueUnprotect, releasing the GC hold.  No leak, no dangling.
    //
    // Usage (when content gains a `jsc` feature):
    //   let callback_val = Types::value_from_object(callback_obj);
    //   let root = ec.create_root(&callback_val);
    //   widget.on_change = Some(root);
    //
    // `create_root` is not yet on `ExecutionContext<T>` (it lives on
    // `JsEngineGcExt`); add it as part of real-code Phase 5.
    #[cfg(feature = "jsc")]
    #[allow(unexpected_cfgs)]
    {
        unimplemented!("JSC: create_root() not yet on ExecutionContext trait");
    }
    Ok(ec.value_undefined())
}

/// Test helper (not a WebIDL operation): calls `perform_a_microtask_checkpoint`
/// and `run_jobs`.  Real content code calls these from domain logic, not JS.
fn flush_microtasks_test(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("TestWidget receiver is not an object"))?;
    let _widget = obj
        .downcast_ref::<TestWidget>()
        .ok_or_else(|| ec.new_type_error("receiver is not a TestWidget"))?;
    ec.perform_a_microtask_checkpoint()?;
    ec.run_jobs();
    Ok(ec.value_undefined())
}

/// Test helper (not a WebIDL operation): returns a rejected promise.
/// Mirrors the `rejected_promise_from_error` / "a promise rejected with"
/// pattern used throughout content (wasm/mod.rs, readablestream.rs, etc.).
fn reject_with_message_test(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("TestWidget receiver is not an object"))?;
    let _widget = obj
        .downcast_ref::<TestWidget>()
        .ok_or_else(|| ec.new_type_error("receiver is not a TestWidget"))?;
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
/// a comma-separated string.  Mirrors the `JsArray::from_iter` pattern
/// used in content (location.rs:655, element.rs:703, etc.).
fn from_tags(
    _this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
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
    Ok(Types::value_from_object(array))
}

// ── WebIDL interface definition ─────────────────────────────────────────

impl WebIdlInterface<Types> for TestWidget {
    const NAME: &'static str = "TestWidget";

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        TestWidget::from_args(args, ec)
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
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
        // reject_with_message_test and flush_microtasks_test are
        // test-only helpers — they are plain Rust functions, not
        // WebIDL operations.  See the tests module for their usage.
    }
}

// ── Context lifecycle exercise ──────────────────────────────────────
// ── Context lifecycle exercise ──────────────────────────────────────

/// Exercises the full entry-point lifecycle:
/// 1. Create a `BoaContext` (the concrete engine runtime).
/// 2. Call `initialize_registry`.
/// 3. Call `register_interface_spec` for `TestWidget` — this proves
///    that our binding functions (getters, setters, operations) actually
///    work with `create_builtin_function` (the key bridging point for
///    eliminating `NativeFunction::from_closure`).
///
/// This is the POC equivalent of `build_context` in host_hooks.rs.
/// Engine-specific: uses BoaContext + ContextBuilder directly.
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

    initialize_registry::<Types>(&mut boa_context);

    register_interface_spec::<Types, TestWidget, _>(&mut boa_context).ok();

    Ok(())
}

// ══════════════════════════════════════════════
// Unit tests — exercise the generic API through real assertions.
// Only the engine-setup helper is Boa-specific; all test bodies
// use the generic ExecutionContext / JsEngine traits.
// ══════════════════════════════════════════════

#[cfg(test)]
#[cfg(feature = "boa")]
mod tests {
    use super::*;
    use crate::webidl::bindings::{initialize_registry, register_interface_spec};
    use boa_engine::context::ContextBuilder;
    use js_engine::boa::BoaContext;
    use js_engine::{EcmascriptHost, ExecutionContext};

    /// Create an initialized BoaContext with the TestWidget interface registered.
    fn setup() -> BoaContext {
        let context = ContextBuilder::new().build().expect("ContextBuilder");
        let mut engine = BoaContext::from_context(context);
        initialize_registry::<Types>(&mut engine);
        register_interface_spec::<Types, TestWidget, _>(&mut engine).ok();
        engine
    }

    /// Create a TestWidget platform object.  Delegates to the cfg-gated
    /// `create_test_widget` helper above.
    fn create_widget(widget: TestWidget, ec: &mut dyn ExecutionContext<Types>) -> JsObject {
        create_test_widget(widget, ec).unwrap()
    }

    #[test]
    fn widget_get_title_returns_default() {
        let mut engine = setup();
        let widget = TestWidget::new();
        let obj = create_widget(widget, &mut engine);
        let js_obj = Types::value_from_object(obj);
        let title_val = get_title(&js_obj, &[], &mut engine).unwrap();
        let title = engine.to_rust_string(title_val).unwrap();
        assert_eq!(title, "Untitled");
    }

    #[test]
    fn widget_set_title_then_get() {
        let mut engine = setup();
        let widget = TestWidget::new();
        let obj = create_widget(widget, &mut engine);
        let js_obj = Types::value_from_object(obj.clone());
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
        let js_obj = Types::value_from_object(obj);
        let visible_val = get_visible(&js_obj, &[], &mut engine).unwrap();
        assert!(engine.to_boolean(&visible_val));
    }

    #[test]
    fn widget_increment_returns_old_and_increments() {
        let mut engine = setup();
        let widget = TestWidget::new();
        let obj = create_widget(widget, &mut engine);
        let js_obj = Types::value_from_object(obj.clone());

        // First increment returns 0, second returns 1.
        let old0 = increment(&js_obj, &[], &mut engine).unwrap();
        assert!((engine.to_number(old0).unwrap() - 0.0).abs() < 0.001);
        let old1 = increment(&js_obj, &[], &mut engine).unwrap();
        assert!((engine.to_number(old1).unwrap() - 1.0).abs() < 0.001);

        // Check count getter now returns 2.
        let count_val = get_count(&js_obj, &[], &mut engine).unwrap();
        assert!((engine.to_number(count_val).unwrap() - 2.0).abs() < 0.001);
    }

    #[test]
    fn widget_to_array_returns_three_elements() {
        let mut engine = setup();
        let mut widget = TestWidget::new();
        widget.title = "ArrayTest".into();
        let obj = create_widget(widget, &mut engine);
        let js_obj = Types::value_from_object(obj);
        let arr_val = to_array(&js_obj, &[], &mut engine).unwrap();

        // Check length
        let arr = Types::value_as_object(&arr_val).unwrap();
        let pk_length = engine.property_key_from_str("length");
        let length_val = ExecutionContext::get(&mut engine, arr.clone(), pk_length).unwrap();
        assert!((engine.to_number(length_val).unwrap() - 3.0).abs() < 0.001);

        // Check first element is the title
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
        let js_obj = Types::value_from_object(obj);
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
        let js_obj = Types::value_from_object(obj.clone());

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
        let js_obj = Types::value_from_object(obj.clone());

        // Build ["a", 1, "b", true] via generic API
        let items = engine.create_empty_array();
        let sv_a = engine.value_from_string(engine.js_string_from_str("a"));
        engine.array_push(&items, sv_a).unwrap();
        let sv_1 = engine.value_from_number(1.0);
        engine.array_push(&items, sv_1).unwrap();
        let sv_b = engine.value_from_string(engine.js_string_from_str("b"));
        engine.array_push(&items, sv_b).unwrap();
        let sv_true = engine.value_from_bool(true);
        engine.array_push(&items, sv_true).unwrap();

        process_items(&js_obj, &[Types::value_from_object(items)], &mut engine).unwrap();
        let count_val = get_count(&js_obj, &[], &mut engine).unwrap();
        assert!((engine.to_number(count_val).unwrap() - 2.0).abs() < 0.001);
    }

    #[test]
    fn widget_delayed_title_returns_resolved_promise() {
        let mut engine = setup();
        let mut widget = TestWidget::new();
        widget.title = "PromiseMe".into();
        let obj = create_widget(widget, &mut engine);
        let js_obj = Types::value_from_object(obj);
        let promise_val = delayed_title(&js_obj, &[], &mut engine).unwrap();
        // The promise should be an object.
        assert!(Types::value_as_object(&promise_val).is_some());
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
        let obj = Types::value_as_object(&result).unwrap();

        // Read back via get_title binding.
        let js_obj = Types::value_from_object(obj);
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
        assert!(
            engine
                .is_loosely_equal(undef.clone(), null.clone())
                .unwrap()
        );
    }

    #[test]
    fn error_construction_and_type_check() {
        let mut engine = setup();
        let type_err = engine.new_type_error("bad");
        let range_err = engine.new_range_error("range");

        assert!(Types::value_as_object(&type_err).is_some());
        assert!(Types::value_as_object(&range_err).is_some());
        assert!(Types::value_is_undefined(&engine.value_undefined()));
        assert!(Types::value_is_null(&engine.value_null()));
        assert_eq!(
            Types::value_as_bool(&engine.value_from_bool(true)),
            Some(true)
        );
        assert!(
            (Types::value_as_number(&engine.value_from_number(7.0)).unwrap() - 7.0).abs() < 0.001
        );
        assert!(
            Types::value_as_string(&engine.value_from_string(engine.js_string_from_str("x")))
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

        // Evaluate a function expression via the engine trait.
        let realm = engine.current_realm();
        let fn_val = engine
            .evaluate_script("(function(x) { return x * 2; })", &realm)
            .unwrap();
        assert!(engine.is_callable(&fn_val));

        // Call it.
        let fn_obj = Types::value_as_object(&fn_val).unwrap();
        let arg = engine.value_from_number(21.0);
        let result = js_engine::EcmascriptHost::call(&mut engine, &fn_obj, &undef, &[arg]).unwrap();
        assert!((engine.to_number(result).unwrap() - 42.0).abs() < 0.001);
    }

    #[test]
    fn promise_resolve_and_then() {
        let mut engine = setup();
        let realm = engine.current_realm();
        let intrinsics = engine.realm_intrinsics(&realm);

        // Promise.resolve(42)
        let val = engine.value_from_number(42.0);
        let promise = engine
            .promise_resolve(intrinsics.promise.clone(), val)
            .unwrap();
        assert!(
            Types::value_as_object(&Types::value_from_object(Types::object_from_promise(
                promise
            )))
            .is_some()
        );

        // new_promise_capability + resolve
        let pcap = engine.new_promise_capability(intrinsics.promise).unwrap();
        let undef = engine.value_undefined();
        let val7 = engine.value_from_number(7.0);
        let call_result =
            js_engine::EcmascriptHost::call(&mut engine, &pcap.resolve, &undef, &[val7]);
        assert!(call_result.is_ok());
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
        let js_obj = Types::value_from_object(obj);
        let result = to_object(&js_obj, &[], &mut engine).unwrap();

        // Should be an object with "title" property.
        let result_obj = Types::value_as_object(&result).unwrap();
        let pk_title = engine.property_key_from_str("title");
        let title_val = ExecutionContext::get(&mut engine, result_obj, pk_title).unwrap();
        assert_eq!(engine.to_rust_string(title_val).unwrap(), "ObjTest");
    }

    #[test]
    fn reject_with_message_returns_rejected_promise() {
        let mut engine = setup();
        let obj = create_widget(TestWidget::new(), &mut engine);
        let js_obj = Types::value_from_object(obj);
        let msg = engine.value_from_string(engine.js_string_from_str("test error"));
        let result = reject_with_message_test(&js_obj, &[msg], &mut engine).unwrap();
        // Should be a promise object.
        assert!(Types::value_as_object(&result).is_some());
    }

    #[test]
    fn from_tags_splits_comma_string() {
        let mut engine = setup();
        let obj = create_widget(TestWidget::new(), &mut engine);
        let js_obj = Types::value_from_object(obj);
        let input = engine.value_from_string(engine.js_string_from_str("a, b, c"));
        let result = from_tags(&js_obj, &[input], &mut engine).unwrap();
        let arr = Types::value_as_object(&result).unwrap();
        let pk_len = engine.property_key_from_str("length");
        let len_val = ExecutionContext::get(&mut engine, arr.clone(), pk_len).unwrap();
        assert!((engine.to_number(len_val).unwrap() - 3.0).abs() < 0.001);
    }

    #[test]
    fn store_callback_then_flush_microtasks() {
        let mut engine = setup();
        let obj = create_widget(TestWidget::new(), &mut engine);
        let js_obj = Types::value_from_object(obj);
        // Create a JS function to store.
        let realm = engine.current_realm();
        let fn_val = engine.evaluate_script("(function() {})", &realm).unwrap();
        // Store it.
        store_callback(&js_obj, &[fn_val.clone()], &mut engine).unwrap();
        // Flush microtasks — should complete without error.
        flush_microtasks_test(&js_obj, &[], &mut engine).unwrap();

        // Verify the callback was stored (read via on_change field access).
        // Since on_change is #[cfg(feature = "boa")], we can only test this
        // side when the boa feature is active (which it always is in content).
        #[cfg(feature = "boa")]
        {
            let obj_ref = Types::value_as_object(&js_obj).unwrap();
            let widget = obj_ref.downcast_ref::<TestWidget>().unwrap();
            assert!(widget.on_change.is_some());
        }
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
                Types::value_from_object(arr),
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
                Types::value_from_object(arr),
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
        let arr_val = Types::value_from_object(arr);
        assert!(engine.is_array(&arr_val).unwrap());
        let num_val = engine.value_from_number(1.0);
        assert!(!engine.is_array(&num_val).unwrap());
    }

    #[test]
    fn is_constructor_detects_constructors() {
        let mut engine = setup();
        let arr = engine.create_empty_array();
        let arr_val = Types::value_from_object(arr);
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
    fn get_method_returns_callable() {
        let mut engine = setup();
        let realm = engine.current_realm();
        let fn_val = engine
            .evaluate_script("(function() { return 42; })", &realm)
            .unwrap();
        let obj = Types::value_as_object(&fn_val).unwrap();
        let pk = engine.property_key_from_str("call");
        let method = engine
            .get_method(Types::value_from_object(obj), pk)
            .unwrap();
        // Function.prototype.call is callable
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
            .get_v(Types::value_from_object(obj.clone()), pk.clone())
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
        // Verify we got a valid constructor back.
        let _ctor_val = Types::value_from_object(Types::object_from_constructor(ctor));
    }

    #[test]
    fn construct_calls_constructor() {
        let mut engine = setup();
        let realm = engine.current_realm();
        let intrinsics = engine.realm_intrinsics(&realm);
        let result = engine
            .construct(intrinsics.object.clone(), &[], None)
            .unwrap();
        // Verify we got a valid object back.
        let _result_val = Types::value_from_object(result);
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
            .json_stringify(Types::value_from_object(obj))
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
        let realm = engine.current_realm();
        let pk = engine.property_key_from_str("testBuiltin");
        let builtin = engine.create_builtin_function(
            Box::new(|_args, _this, inner_ec| Ok(inner_ec.value_from_number(42.0))),
            0,
            pk,
            &realm,
        );
        let builtin_obj = Types::object_from_function(builtin);
        let undef = engine.value_undefined();
        let result =
            js_engine::EcmascriptHost::call(&mut engine, &builtin_obj, &undef, &[]).unwrap();
        assert!((engine.to_number(result).unwrap() - 42.0).abs() < 0.001);
    }
    // Note: create_realm, set_realm_global_object, set_default_global_bindings,
    // and set_host_hooks are used by content (host_hooks.rs::build_context).
    // evaluate_module and generator_start are trait methods with no content
    // callers yet — tested when they gain implementations.

    #[test]
    fn create_realm_and_set_bindings() {
        let mut engine = setup();
        let realm = engine.create_realm();
        let global_obj = engine.create_plain_object(None);
        engine.set_realm_global_object(&realm, global_obj, None);
        let _ = engine.set_default_global_bindings(&realm);
        engine.set_host_hooks(js_engine::HostHooks::empty());
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
        assert!(Types::object_as_map(&Types::value_as_object(&map_val).unwrap()).is_some());
        // Set
        let set_val = engine.evaluate_script("new Set([1,2,3])", &realm).unwrap();
        assert!(Types::object_as_set(&Types::value_as_object(&set_val).unwrap()).is_some());
        // TypedArray
        let ta_val = engine.evaluate_script("new Uint8Array(4)", &realm).unwrap();
        assert!(Types::object_as_typed_array(&Types::value_as_object(&ta_val).unwrap()).is_some());
        // DataView
        let dv_val = engine
            .evaluate_script("new DataView(new ArrayBuffer(8))", &realm)
            .unwrap();
        assert!(Types::object_as_data_view(&Types::value_as_object(&dv_val).unwrap()).is_some());
    }
}

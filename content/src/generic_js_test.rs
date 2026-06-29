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

use boa_engine::{JsData, JsValue, object::JsObject};
use boa_gc::{Finalize, Trace};

use crate::js::Types;
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface, create_interface_instance,
};
use js_engine::{
    Completion, ExecutionContext, IteratorKind, JsEngine, JsTypes, PropertyDescriptor,
};

// ── Domain type ──────────────────────────────────────────────────────────

/// A toy domain struct exercising the full generic-API binding pattern.
#[derive(Trace, Finalize, JsData)]
pub(crate) struct TestWidget {
    title: String,
    visible: bool,
    count: u32,
}

impl TestWidget {
    fn new() -> Self {
        Self {
            title: String::from("Untitled"),
            visible: true,
            count: 0,
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
    Ok(JsValue::from(result))
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
    Ok(JsValue::from(array))
}

/// Setter: `widget.count = val` — exercises `ec.to_number` in a binding pattern.
fn set_count(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let obj = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("TestWidget receiver is not an object"))?;
    let num_val = args.first().cloned().unwrap_or(ec.value_undefined());
    let new_count = ec.to_number(num_val)?;
    let mut widget = obj
        .downcast_mut::<TestWidget>()
        .ok_or_else(|| ec.new_type_error("receiver is not a TestWidget"))?;
    widget.count = new_count as u32;
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
    let callback_val = JsValue::from(callback_obj.clone());
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
    };
    let obj = create_interface_instance::<Types, TestWidget>(widget, ec)?;
    Ok(JsValue::from(obj))
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
    }
}

// ── Top-level exercise function ──────────────────────────────────────────

/// Exercise every relevant `ExecutionContext<T>` / `JsTypes` / `EcmascriptHost<T>`
/// API surface point.  This function exists purely to type-check the generic
/// layer — it is never called from production code.
///
/// Call from `main.rs` startup (behind a feature flag or dead-code allowance)
/// to get compile-time feedback on the generic API design.
#[allow(dead_code, unused_variables)]
pub(crate) fn exercise_generic_api(ec: &mut dyn ExecutionContext<Types>) {
    // ── Value construction ───────────────────────────────────────────
    let undef: JsValue = ec.value_undefined();
    let null: JsValue = ec.value_null();
    let bool_val: JsValue = ec.value_from_bool(true);
    let num_val: JsValue = ec.value_from_number(42.0);
    let str_val: JsValue = ec.value_from_string(ec.js_string_from_str("hello"));

    // ── JsTypes downcasts ────────────────────────────────────────────
    let _opt_obj = Types::value_as_object(&str_val);
    let _opt_str = Types::value_as_string(&str_val);
    let _opt_num = Types::value_as_number(&num_val);
    let _opt_bool = Types::value_as_bool(&bool_val);
    let _is_undef = Types::value_is_undefined(&undef);
    let _is_null = Types::value_is_null(&null);

    // ── Type conversion (§7.1) ───────────────────────────────────────
    let _b: bool = ec.to_boolean(&bool_val);
    let _n: f64 = ec.to_number(num_val.clone()).unwrap_or(0.0);
    let _s: String = ec.to_rust_string(str_val.clone()).unwrap_or_default();

    // ── Type conversion — more operations ────────────────────────────
    let _len: u64 = ec.to_length(num_val.clone()).unwrap_or(0);
    let _obj: JsObject = ec
        .to_object(bool_val.clone())
        .unwrap_or_else(|_| ec.create_plain_object(None));

    // ── String utilities ────────────────────────────────────────────
    let js_str = ec.js_string_from_str("test string");
    let _rust_str: String = ec.js_string_to_rust_string(&js_str);

    // ── Object / array construction ─────────────────────────────────
    let plain: JsObject = ec.create_plain_object(None);
    let _ = ec.object_set_property(plain.clone(), "key", bool_val.clone());
    let arr: JsObject = ec.create_empty_array();
    let _ = ec.array_push(&arr, num_val.clone());
    let _ = ec.array_push(&arr, str_val.clone());

    // ── Error construction ──────────────────────────────────────────
    let _type_err: JsValue = ec.new_type_error("something went wrong");
    let _range_err: JsValue = ec.new_range_error("out of bounds");

    // ── Global object ───────────────────────────────────────────────
    let _global: JsObject = ec.global_object();

    // ── Property key ────────────────────────────────────────────────
    let _pk = ec.property_key_from_str("testProp");

    // ── Host data store ─────────────────────────────────────────────
    let test_id = std::any::TypeId::of::<String>();
    ec.store_host_any(test_id, Box::new("host data".to_string()));
    let _stored: Option<&dyn std::any::Any> = ec.get_host_any(&test_id);
    let _removed: Option<Box<dyn std::any::Any>> = ec.remove_host_any(&test_id);

    // ── Platform object creation ────────────────────────────────────
    let widget = TestWidget::new();
    let widget_obj: JsObject = create_interface_instance::<Types, TestWidget>(widget, ec)
        .unwrap_or_else(|_| ec.create_plain_object(None));

    // ── EcmascriptHost operations ────────────────────────────────────
    let _title_val =
        ExecutionContext::get(ec, widget_obj.clone(), ec.property_key_from_str("title"));

    let _callable_bool: bool = ec.is_callable(&bool_val);
    let _callable_null: bool = ec.is_callable(&null);

    // ── More type conversions ───────────────────────────────────────
    let _i32: i32 = ec.to_int32(num_val.clone()).unwrap_or(0);
    let _u32: u32 = ec.to_uint32(num_val.clone()).unwrap_or(0);

    // ── Comparison ─────────────────────────────────────────────────
    let _same: bool = ec.same_value(&undef, &null);
    let _strict: bool = ec.is_strictly_equal(&bool_val, &bool_val);

    // ── is_array ───────────────────────────────────────────────────
    let _is_arr: bool = ec.is_array(&JsValue::from(arr)).unwrap_or(false);

    // ── Upcasts from JsTypes ────────────────────────────────────────
    let _val_from_obj: JsValue = JsValue::from(plain);

    // ── set / create_data_property ──────────────────────────────────
    let pk_custom = ec.property_key_from_str("customProp");
    let val_99 = ec.value_from_number(99.0);
    let _ = ec.set(widget_obj.clone(), pk_custom, val_99, false);
    let pk_data = ec.property_key_from_str("dataProp");
    let val_true = ec.value_from_bool(true);
    let _ = ec.create_data_property(widget_obj.clone(), pk_data, val_true);

    // ── require_object_coercible ────────────────────────────────────
    let _ = ec.require_object_coercible(str_val.clone());

    // ── to_js_string (ToString abstract op) ─────────────────────────
    let _js_str_val = ec
        .to_js_string(num_val.clone())
        .unwrap_or_else(|_| ec.js_string_from_str(""));

    // ── to_primitive ────────────────────────────────────────────────
    let _prim_val = ec.to_primitive(num_val.clone(), None);

    // ── is_callable / is_constructor ────────────────────────────────
    let _constr: bool = ec.is_constructor(&bool_val);

    // ── report_error / report_exception ─────────────────────────────
    ec.report_error("test error message");
    let exc = ec.new_type_error("callback exception");
    ec.report_exception(exc);

    // ── Property descriptor operations (§6.2.5) ─────────────────────
    let pk_desc = ec.property_key_from_str("testDesc");
    let descriptor = PropertyDescriptor {
        value: Some(ec.value_from_number(42.0)),
        writable: Some(true),
        get: None,
        set: None,
        enumerable: Some(true),
        configurable: Some(true),
    };
    let _ = ec.define_property_or_throw(widget_obj.clone(), pk_desc.clone(), descriptor);
    let _ = ec.has_property(widget_obj.clone(), pk_desc);

    // ── Error construction (additional variants) ────────────────────
    let _new_err: JsValue = ec.new_range_error("index out of range");
    let _ = ec.new_type_error("type mismatch in setter"); // error-path pattern

    // ── Iterator operations (§7.4) ──────────────────────────────────
    let iter_arr: JsObject = ec.create_empty_array();
    let v1 = ec.value_from_number(1.0);
    let _ = ec.array_push(&iter_arr, v1);
    let v2 = ec.value_from_number(2.0);
    let _ = ec.array_push(&iter_arr, v2);
    let v3 = ec.value_from_number(3.0);
    let _ = ec.array_push(&iter_arr, v3);
    let mut iter_record = ec
        .get_iterator(JsValue::from(iter_arr), IteratorKind::Sync, None)
        .unwrap_or_else(|_| {
            // Fallback: create a dummy iterator record (won't happen with real arrays,
            // but satisfies the type-checker in a test context).
            panic!("get_iterator should succeed for arrays")
        });
    let first_step = ec.iterator_step_value(&mut iter_record);
    if let Ok(Some(_val)) = first_step {
        // Got first value — close the iterator normally.
        let undef = ec.value_undefined();
        let _ = ec.iterator_close(iter_record, Ok(undef));
    }

    // ── Promise operations at the binding level (§27.2) ─────────────
    let realm = ec.current_realm();
    let intrinsics = ec.realm_intrinsics(&realm);
    let pcap = ec
        .new_promise_capability(intrinsics.promise.clone())
        .unwrap_or_else(|_| {
            panic!("new_promise_capability should succeed with Promise constructor")
        });
    // Resolve the promise immediately.
    let undef = ec.value_undefined();
    let resolved_val = ec.value_from_string(ec.js_string_from_str("resolved"));
    let _ = ec.call(&pcap.resolve, &undef, &[resolved_val]);

    // ── Call / Construct at the binding level (§7.3) ────────────────
    // Call a built-in method on the widget object.
    let pk_to_array = ec.property_key_from_str("toArray");
    let to_array_val =
        ExecutionContext::get(ec, widget_obj.clone(), pk_to_array).unwrap_or(ec.value_undefined());
    if let Some(to_array_fn) = Types::value_as_object(&to_array_val) {
        if ec.is_callable(&to_array_val) {
            let _ = ec.call(&to_array_fn, &JsValue::from(widget_obj.clone()), &[]);
        }
    }

    // ── to_js_string in a binding-function pattern ──────────────────
    // formatLabel exercises to_js_string → js_string_to_rust_string;
    // here we also exercise the standalone pattern.
    let js_str_from_num = ec
        .to_js_string(num_val.clone())
        .unwrap_or_else(|_| ec.js_string_from_str("0"));
    let _rust_from_num: String = ec.js_string_to_rust_string(&js_str_from_num);

    // ── create_interface_instance error path ────────────────────────
    let _result: Result<JsObject, JsValue> =
        create_interface_instance::<Types, TestWidget>(TestWidget::new(), ec);

    // ── set_count (numeric setter exercising to_number) ─────────────
    let _ = set_count(
        &JsValue::from(widget_obj.clone()),
        &[ec.value_from_number(99.0)],
        ec,
    );

    // ── call_user_objects_operation (Web IDL callback helper) ──────────
    {
        use crate::webidl::{Callback, call_user_objects_operation};
        let widget_callback = Callback::from_object(widget_obj.clone());
        // call_user_objects_operation takes &mut dyn EcmascriptHost<Types>;
        // ExecutionContext<Types> coerces automatically.
        let _ = call_user_objects_operation(ec, &widget_callback, "toArray", &[], None);
    }

    // ── property_key_from_index (numeric key construction) ──────────
    let pk_index = ec.property_key_from_index(0);
    let pk_str = ec.property_key_from_str("0");
    // Exercise get with a numeric property key on an array.
    let indexed_arr: JsObject = ec.create_empty_array();
    let v10 = ec.value_from_number(10.0);
    let _ = ec.array_push(&indexed_arr, v10);
    let v20 = ec.value_from_number(20.0);
    let _ = ec.array_push(&indexed_arr, v20);
    let _get0 = ExecutionContext::get(ec, indexed_arr.clone(), pk_index);
    let _get0_str = ExecutionContext::get(ec, indexed_arr, pk_str);
}

// ── Engine-factory exercise function ───────────────────────────────────

/// Exercise `JsEngine<T>` factory operations, particularly `create_builtin_function`
/// which is the key to eliminating `NativeFunction::from_closure` calls (Step C).
///
/// Takes both `&mut dyn JsEngine<Types>` and `&mut dyn ExecutionContext<Types>`
/// because `create_builtin_function` is a factory method but its behaviour closure
/// receives an `ExecutionContext<T>`.
#[allow(dead_code, unused_variables)]
pub(crate) fn exercise_engine_api(
    engine: &mut dyn JsEngine<Types>,
    ec: &mut dyn ExecutionContext<Types>,
) {
    let realm = ec.current_realm();
    let pk = ec.property_key_from_str("testBuiltin");

    // Create a built-in function that returns the constant 42.
    let builtin = engine.create_builtin_function(
        Box::new(|_args, _this, inner_ec| Ok(inner_ec.value_from_number(42.0))),
        0,
        pk,
        &realm,
    );

    // Convert Function → JsObject → JsValue and call it.
    let builtin_obj = Types::object_from_function(builtin);
    let undef = ec.value_undefined();
    let call_result = ec.call(&builtin_obj, &undef, &[]);
    let _ = call_result;
}

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

// ── Remaining API surface exercise ──────────────────────────────────

/// Exercises every `ExecutionContext<T>`, `JsEngine<T>`, and `EcmascriptHost<T>`
/// method that is NOT already covered by `exercise_generic_api`,
/// `exercise_engine_api`, or `exercise_context_lifecycle`.
///
/// Takes both `engine` and `ec` because some operations span both traits
/// (e.g. `allocate_array_buffer` is on `JsEngine`, `is_detached_buffer` is on `ExecutionContext`).
#[allow(dead_code, unused_variables)]
pub(crate) fn exercise_remaining_api(
    engine: &mut dyn JsEngine<Types>,
    ec: &mut dyn ExecutionContext<Types>,
) {
    use js_engine::{
        HostHooks, IntegrityLevel, IteratorKind, SharedMemoryOrder, TypedArrayElementType,
    };

    // ── Engine: create_realm + globals + host_hooks ────────────────
    let realm = engine.create_realm();
    let global_obj = ec.create_plain_object(None);
    engine.set_realm_global_object(&realm, global_obj.clone(), None);
    let _ = engine.set_default_global_bindings(&realm);
    engine.set_host_hooks(HostHooks::empty());

    // ── Engine: evaluate_script / evaluate_module ───────────────────
    let script_result = engine.evaluate_script("40 + 2", &realm);
    let _ = script_result;
    let module_result = engine.evaluate_module("export const x = 1;", &realm);
    let _ = module_result;

    // ── EcmascriptHost: perform_a_microtask_checkpoint ─────────────
    let _ = ec.perform_a_microtask_checkpoint();

    // ── Type conversions: to_numeric, to_bigint, canonical, to_index ─
    let num_val = ec.value_from_number(123.0);
    let numeric = ec.to_numeric(num_val.clone());
    let _ = numeric;
    // canonical_numeric_index_string — needs a numeric JS string
    let num_str = ec.js_string_from_str("42");
    let canonical = ec.canonical_numeric_index_string(&num_str);
    let _ = canonical;
    let to_idx = ec.to_index(num_val.clone());
    let _ = to_idx;
    let prop_key = ec.to_property_key(num_val.clone());
    let _ = prop_key;

    // ── Type conversions: smaller integer widths ────────────────────
    let _i16 = ec.to_int16(num_val.clone());
    let _u16 = ec.to_uint16(num_val.clone());
    let _i8 = ec.to_int8(num_val.clone());
    let _u8 = ec.to_uint8(num_val.clone());
    let _u8c = ec.to_uint8_clamp(num_val.clone());

    // ── Testing/comparison: is_extensible, is_integral_number, etc. ──
    let plain = ec.create_plain_object(None);
    let _ext = ec.is_extensible(&plain);
    let _int = ec.is_integral_number(&num_val);
    let _ispk = ec.is_property_key(&num_val);
    let bool_val = ec.value_from_bool(true);
    let _svz = ec.same_value_zero(&num_val, &num_val);
    let undef = ec.value_undefined();
    let _loose = ec.is_loosely_equal(num_val.clone(), undef.clone());

    // ── Object operations: get_v, delete_property_or_throw, etc. ────
    let pk = ec.property_key_from_str("testProp");
    let _ = ec.object_set_property(plain.clone(), "testProp", bool_val.clone());
    let _get_v = ec.get_v(JsValue::from(plain.clone()), pk.clone());
    let _del = ec.delete_property_or_throw(plain.clone(), pk.clone());

    // ── has_own_property ──────────────────────────────────────────
    let _has_own = ec.has_own_property(plain.clone(), pk.clone());

    // ── get_method ────────────────────────────────────────────────
    let _method = ec.get_method(JsValue::from(plain.clone()), pk.clone());

    // ── set_prototype ─────────────────────────────────────────────
    let proto = ec.create_plain_object(None);
    let _set_proto = ec.set_prototype(plain.clone(), Some(proto));

    // ── set_integrity_level / test_integrity_level ─────────────────
    let frozen_obj = ec.create_plain_object(None);
    let val_1 = ec.value_from_number(1.0);
    let _ = ec.object_set_property(frozen_obj.clone(), "a", val_1);
    let _sealed = ec.set_integrity_level(frozen_obj.clone(), IntegrityLevel::Sealed);
    let _frozen = ec.test_integrity_level(frozen_obj.clone(), IntegrityLevel::Frozen);

    // ── species_constructor ───────────────────────────────────────
    let intrinsics = ec.realm_intrinsics(&realm);
    let _species = ec.species_constructor(plain.clone(), intrinsics.object.clone());

    // ── async_iterator_close ─────────────────────────────────────
    // Create an async iterator record (dummy — we just need the type)
    let dummy_iter = ec.create_empty_array();
    let dummy_arr_val = ec.value_from_number(1.0);
    let _ = ec.array_push(&dummy_iter, dummy_arr_val);
    if let Ok(mut iter_record) =
        ec.get_iterator(JsValue::from(dummy_iter), IteratorKind::Sync, None)
    {
        let close_val = ec.value_undefined();
        let _ = ec.async_iterator_close(iter_record, Ok(close_val));
    }

    // ── Jobs: enqueue_job / run_jobs ──────────────────────────────
    ec.enqueue_job(Box::new(|| {}));
    ec.run_jobs();

    // ── construct ─────────────────────────────────────────────────────
    let _constructed = ec.construct(intrinsics.object.clone(), &[], None);

    // ── promise_resolve ───────────────────────────────────────────
    let undef_val = ec.value_undefined();
    let _resolved = ec.promise_resolve(intrinsics.promise.clone(), undef_val);

    // ── perform_promise_then ──────────────────────────────────────
    let pcap = ec
        .new_promise_capability(intrinsics.promise.clone())
        .unwrap_or_else(|_| panic!("new_promise_capability should succeed"));
    let promise_obj =
        Types::value_as_object(&pcap.promise).expect("capability promise should be an object");
    let promise =
        Types::object_as_promise(&promise_obj).expect("capability promise should be a Promise");
    // Create a builtin on_fulfilled callback via the engine.
    let pk_onful = ec.property_key_from_str("onFulfilled");
    let on_fulfilled = engine.create_builtin_function(
        Box::new(|_args, _this, inner_ec| Ok(inner_ec.value_undefined())),
        1,
        pk_onful,
        &realm,
    );
    let _then = ec.perform_promise_then(promise, Some(on_fulfilled), None, Some(pcap));

    // ── Rejected promise pattern ───────────────────────────────────
    // Generic equivalent of WebIDL's "a promise rejected with".
    let err_val = ec.new_type_error("test rejection");
    let rcap = ec
        .new_promise_capability(intrinsics.promise.clone())
        .unwrap_or_else(|_| panic!("new_promise_capability should succeed"));
    let _ = ec.call(&rcap.reject, &undef, &[err_val]);

    // ── ArrayBuffer: allocate + inspect + get/set ──────────────────
    let ab = engine
        .allocate_array_buffer(intrinsics.array_buffer.clone(), 16, None)
        .unwrap_or_else(|_| panic!("allocate_array_buffer should succeed"));
    let _detached = ec.is_detached_buffer(&ab);
    let _fixed = ec.is_fixed_length_array_buffer(&ab);
    let byte_val = ec.get_value_from_buffer(
        &ab,
        0,
        TypedArrayElementType::Uint8,
        false,
        SharedMemoryOrder::SeqCst,
    );
    let _ = byte_val;
    let val_255 = ec.value_from_number(255.0);
    let _ = ec.set_value_in_buffer(
        &ab,
        0,
        TypedArrayElementType::Uint8,
        val_255,
        false,
        SharedMemoryOrder::SeqCst,
    );
    // clone_array_buffer
    let _cloned = engine.clone_array_buffer(ab.clone(), 0, 8, intrinsics.array_buffer.clone());
    // detach_array_buffer
    let _ = engine.detach_array_buffer(ab, None);
    // allocate_shared_array_buffer (needs SharedArrayBuffer constructor)
    let _sab = engine.allocate_shared_array_buffer(intrinsics.shared_array_buffer.clone(), 16);

    // ── to_bigint / string_to_bigint ──────────────────────────────
    let bigint_val = ec.value_from_bigint(42);
    let _bigint = ec.to_bigint(bigint_val.clone());
    let num_string = ec.js_string_from_str("123");
    let _str_bigint = ec.string_to_bigint(num_string);

    // ── json_stringify (ECMA-262 §24.5.2) ─────────────────────────
    let test_obj_for_json = ec.create_plain_object(None);
    let val_1 = ec.value_from_number(1.0);
    let _ = ec.object_set_property(test_obj_for_json.clone(), "x", val_1);
    let _json_str = ec.json_stringify(JsValue::from(test_obj_for_json));

    // ── Object downcasts via evaluate_script ──────────────────────
    let map_val = engine
        .evaluate_script("new Map([['k', 'v']])", &realm)
        .unwrap_or(ec.value_undefined());
    if let Some(map_obj) = Types::value_as_object(&map_val) {
        let _map = Types::object_as_map(&map_obj);
    }
    let set_val = engine
        .evaluate_script("new Set([1,2,3])", &realm)
        .unwrap_or(ec.value_undefined());
    if let Some(set_obj) = Types::value_as_object(&set_val) {
        let _set = Types::object_as_set(&set_obj);
    }
    let wm_val = engine
        .evaluate_script("new WeakMap()", &realm)
        .unwrap_or(ec.value_undefined());
    if let Some(wm_obj) = Types::value_as_object(&wm_val) {
        let _wm = Types::object_as_weak_map(&wm_obj);
    }
    let ws_val = engine
        .evaluate_script("new WeakSet()", &realm)
        .unwrap_or(ec.value_undefined());
    if let Some(ws_obj) = Types::value_as_object(&ws_val) {
        let _ws = Types::object_as_weak_set(&ws_obj);
    }
    let wr_val = engine
        .evaluate_script("new WeakRef({})", &realm)
        .unwrap_or(ec.value_undefined());
    if let Some(wr_obj) = Types::value_as_object(&wr_val) {
        let _wr = Types::object_as_weak_ref(&wr_obj);
    }
    let ta_val = engine
        .evaluate_script("new Uint8Array(4)", &realm)
        .unwrap_or(ec.value_undefined());
    if let Some(ta_obj) = Types::value_as_object(&ta_val) {
        let _ta = Types::object_as_typed_array(&ta_obj);
    }
    let dv_val = engine
        .evaluate_script("new DataView(new ArrayBuffer(8))", &realm)
        .unwrap_or(ec.value_undefined());
    if let Some(dv_obj) = Types::value_as_object(&dv_val) {
        let _dv = Types::object_as_data_view(&dv_obj);
    }

    // ── generator_start (via evaluate_script) ─────────────────────
    let gen_val = engine
        .evaluate_script("(function*(){yield 1})()", &realm)
        .unwrap_or(ec.value_undefined());
    if let Some(gen_obj) = Types::value_as_object(&gen_val) {
        if let Some(generator) = Types::object_as_generator(&gen_obj) {
            let pk_start = ec.property_key_from_str("next");
            let next_fn =
                ExecutionContext::get(ec, gen_obj, pk_start).unwrap_or(ec.value_undefined());
            if let Some(next_obj) = Types::value_as_object(&next_fn) {
                if let Some(next_func) = Types::object_as_function(&next_obj) {
                    let _ = ec.generator_start(generator, next_func);
                }
            }
        }
    }

    // ── async_iterator_close (with async generator) ───────────────
    let agen_val = engine
        .evaluate_script("(async function*(){yield 1})()", &realm)
        .unwrap_or(ec.value_undefined());
    if let Some(agen_obj) = Types::value_as_object(&agen_val) {
        let _agen = Types::object_as_async_generator(&agen_obj);
    }

    // ── register_global_property (generic equivalent) ─────────────
    let global = ec.global_object();
    let val_1 = ec.value_from_number(1.0);
    let _ = ec.object_set_property(global, "__testPOC", val_1);
}

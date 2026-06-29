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
use js_engine::{Completion, ExecutionContext, JsTypes};

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

// ── WebIDL interface definition ─────────────────────────────────────────

impl WebIdlInterface<Types> for TestWidget {
    const NAME: &'static str = "TestWidget";

    fn create_platform_object(
        _new_target: &JsValue,
        _args: &[JsValue],
        _ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        Ok(TestWidget::new())
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
            setter: None,
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
}

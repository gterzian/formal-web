use crate::infra::parse_a_json_string_to_a_javascript_value;
use crate::js::Types;
use crate::webidl::bindings::{InterfaceDefinition, WebIdlInterface};
use crate::webidl::{
    DefaultIteratorKind, PairIterable, create_default_iterator, pair_iterable_for_each,
};
use crate::webrtc::RTCStatsReport;
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::{member, this_as};

type JsValue = <Types as JsTypes>::JsValue;

fn report(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<RTCStatsReport, Types> {
    this_as::<RTCStatsReport>(this, "RTCStatsReport", ec)
}

fn string(value: &str, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
    let string = ec.js_string_from_str(value);
    ec.value_from_string(string)
}

/// A stats object, parsed from its JSON text.
fn stats_value(json: &str, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
    parse_a_json_string_to_a_javascript_value(json, ec).unwrap_or_else(|_| ec.value_undefined())
}

impl WebIdlInterface<Types> for RTCStatsReport {
    const NAME: &'static str = "RTCStatsReport";

    fn create_platform_object(
        _new_target: &JsValue,
        _args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        Err(ec.new_type_error("Illegal constructor"))
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        member!(def, attribute "size", size);
        member!(def, operation "get", 1, get, promise false);
        member!(def, operation "has", 1, has, promise false);
        member!(def, operation "entries", 0, entries, promise false);
        member!(def, operation "keys", 0, keys, promise false);
        member!(def, operation "values", 0, values, promise false);
        member!(def, operation "forEach", 1, for_each, promise false);
    }
}

impl PairIterable for RTCStatsReport {
    fn value_pairs_to_iterate_over(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Vec<(JsValue, JsValue)> {
        self.entries()
            .iter()
            .map(|(id, json)| (string(id, ec), stats_value(json, ec)))
            .collect()
    }
}

fn size(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let size = report(this, ec)?.entries().len();
    Ok(ec.value_from_number(size as f64))
}

fn key_argument(
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<String, Types> {
    let undefined = ec.value_undefined();
    ec.to_rust_string(args.first().cloned().unwrap_or(undefined))
}

fn get(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let report = report(this, ec)?;
    let key = key_argument(args, ec)?;
    Ok(match report.entries().iter().find(|(id, _)| *id == key) {
        Some((_, json)) => stats_value(json, ec),
        None => ec.value_undefined(),
    })
}

fn has(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let report = report(this, ec)?;
    let key = key_argument(args, ec)?;
    let has = report.entries().iter().any(|(id, _)| *id == key);
    Ok(ec.value_from_bool(has))
}

fn entries(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let report = report(this, ec)?;
    let iterator = create_default_iterator(report, DefaultIteratorKind::KeyPlusValue, ec)?;
    Ok(Types::value_from_object(iterator))
}

fn keys(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let report = report(this, ec)?;
    let iterator = create_default_iterator(report, DefaultIteratorKind::Key, ec)?;
    Ok(Types::value_from_object(iterator))
}

fn values(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let report = report(this, ec)?;
    let iterator = create_default_iterator(report, DefaultIteratorKind::Value, ec)?;
    Ok(Types::value_from_object(iterator))
}

fn for_each(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let report = report(this, ec)?;
    pair_iterable_for_each(&report, this, args, ec)
}

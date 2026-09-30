use std::cell::Cell;
use std::rc::Rc;

use js_engine::gc::{Finalize, Trace};
use js_engine::records::PropertyDescriptor;
use js_engine::{Completion, ExecutionContext, JsTypes, gc_struct};

use crate::js::Types;

use super::bindings::WebIdlInterface;
use super::callback::{ExceptionBehavior, callback_function_value, invoke_callback_function};

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;

/// <https://webidl.spec.whatwg.org/#dfn-value-pairs-to-iterate-over>
pub(crate) trait PairIterable:
    WebIdlInterface<Types> + Clone + Trace + Finalize + 'static
{
    fn value_pairs_to_iterate_over(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Vec<(JsValue, JsValue)>;
}

/// <https://webidl.spec.whatwg.org/#default-iterator-object>
#[derive(Clone, Copy)]
pub(crate) enum DefaultIteratorKind {
    Key,
    Value,
    KeyPlusValue,
}

/// <https://webidl.spec.whatwg.org/#default-iterator-object>
#[gc_struct]
struct DefaultIterator<T: PairIterable> {
    target: T,
    #[ignore_trace]
    kind: DefaultIteratorKind,
    #[ignore_trace]
    index: Rc<Cell<usize>>,
}

/// <https://webidl.spec.whatwg.org/#js-iterable>
pub(crate) fn create_default_iterator<T: PairIterable>(
    target: T,
    kind: DefaultIteratorKind,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    // Step 1: Let idlObject be the IDL interface type value that represents
    // a reference to the this value.
    // Step 2: Let iterator be a newly created default iterator object for
    // interface with idlObject as its target and kind as its kind.
    let prototype = iterator_prototype_object::<T>(ec)?;
    let iterator = DefaultIterator {
        target,
        kind,
        index: Rc::new(Cell::new(0)),
    };

    // Step 3: Return iterator.
    Ok(js_engine::create_platform_object(ec, &prototype, iterator))
}

/// <https://webidl.spec.whatwg.org/#es-forEach>
pub(crate) fn pair_iterable_for_each<T: PairIterable>(
    target: &T,
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    // Step 1: Let idlObject be the IDL interface type value that represents
    // a reference to the this value.
    // Step 2: Let idlCallback be the callback argument converted to an IDL
    // value of type Function.
    let undefined = ec.value_undefined();
    let callback = callback_function_value(args.first().unwrap_or(&undefined), ec)?;
    let this_arg = args.get(1).cloned().unwrap_or(undefined);

    // Step 3: Let pairs be idlObject's list of value pairs to iterate over.
    let mut pairs = target.value_pairs_to_iterate_over(ec);

    // Step 4: Let i be 0.
    let mut index = 0;

    // Step 5: While i < pairs' size:
    while index < pairs.len() {
        // Step 5.1: Let pair be pairs[i].
        let (key, value) = pairs[index].clone();

        // Step 5.2: Invoke idlCallback with « pair's value, pair's key,
        // idlObject » and with thisArg as the callback this value.
        invoke_callback_function(
            ec,
            &callback,
            &[value, key, this.clone()],
            ExceptionBehavior::Rethrow,
            Some(&this_arg),
        )?;

        // Step 5.3: Set pairs to idlObject's current list of value pairs to
        // iterate over. (It might have changed.)
        pairs = target.value_pairs_to_iterate_over(ec);

        // Step 5.4: Set i to i + 1.
        index += 1;
    }
    Ok(ec.value_undefined())
}

/// <https://webidl.spec.whatwg.org/#js-iterator-prototype-object>
fn iterator_prototype_object<T: PairIterable>(
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    // Note: The spec has one iterator prototype object per interface and
    // realm; one is created per default iterator object here.
    // The iterator prototype object's [[Prototype]] is %IteratorPrototype%,
    // reached through an array iterator's prototype chain.
    let array = ec.create_empty_array();
    let array_value = Types::value_from_object(array.clone());
    let iterator_key = ec.property_key_from_well_known_symbol("iterator");
    let array_iterator = match ec.get_method(array_value.clone(), iterator_key)? {
        Some(method) => ec.call(&Types::object_from_function(method), &array_value, &[])?,
        None => return Err(ec.new_type_error("arrays are not iterable in this realm")),
    };
    let iterator_prototype = match Types::value_as_object(&array_iterator) {
        Some(array_iterator) => match ec.get_prototype_of(array_iterator)? {
            Some(array_iterator_prototype) => ec.get_prototype_of(array_iterator_prototype)?,
            None => None,
        },
        None => None,
    };
    let prototype = ec.create_plain_object(iterator_prototype.as_ref());

    // The next method.
    let next: <Types as JsTypes>::Function = ec.create_builtin_fn_static(
        |args: &[JsValue], this: JsValue, ec: &mut dyn ExecutionContext<Types>| {
            default_iterator_next::<T>(this, args, ec)
        },
        0,
        ec.property_key_from_str("next"),
        false,
    );
    let next_key = ec.property_key_from_str("next");
    ec.define_property_or_throw(
        prototype.clone(),
        next_key,
        PropertyDescriptor {
            value: Some(Types::value_from_object(Types::object_from_function(next))),
            writable: Some(true),
            enumerable: Some(true),
            configurable: Some(true),
            get: None,
            set: None,
        },
    )?;

    // The class string of an iterator prototype object for a given interface
    // is the result of concatenating the identifier of the interface and the
    // string " Iterator".
    let class_string = ec.js_string_from_str(&format!("{} Iterator", T::NAME));
    let class_string = ec.value_from_string(class_string);
    let to_string_tag_key = ec.property_key_from_well_known_symbol("toStringTag");
    ec.define_property_or_throw(
        prototype.clone(),
        to_string_tag_key,
        PropertyDescriptor {
            value: Some(class_string),
            writable: Some(false),
            enumerable: Some(false),
            configurable: Some(true),
            get: None,
            set: None,
        },
    )?;
    Ok(prototype)
}

/// <https://webidl.spec.whatwg.org/#js-iterator-prototype-object>
fn default_iterator_next<T: PairIterable>(
    this: JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    // Step 1: Let interface be the interface for which the iterator prototype
    // object exists.
    // Step 2: Let thisValue be the this value.
    // Step 3: Let object be ? ToObject(thisValue).
    let object = ec.to_object(this)?;

    // Step 4: If object is a platform object, then perform a security check,
    // passing: the platform object object, the identifier "next", and the
    // type "method".
    // Step 5: If object is not a default iterator object for interface, then
    // throw a TypeError.
    let iterator = ec
        .with_object_any(&object)
        .and_then(|data| data.downcast_ref::<DefaultIterator<T>>().cloned());
    let Some(iterator) = iterator else {
        return Err(ec.new_type_error(&format!(
            "next called on an object that is not a {} Iterator",
            T::NAME
        )));
    };

    // Step 6: Let index be object's index.
    let index = iterator.index.get();

    // Step 7: Let kind be object's kind.
    let kind = iterator.kind;

    // Step 8: Let values be object's target's value pairs to iterate over.
    let values = iterator.target.value_pairs_to_iterate_over(ec);

    // Step 9: Let len be the length of values.
    let len = values.len();

    // Step 10: If index is greater than or equal to len, then return
    // CreateIteratorResultObject(undefined, true).
    if index >= len {
        let undefined = ec.value_undefined();
        let result = create_iterator_result_object(undefined, true, ec)?;
        return Ok(Types::value_from_object(result));
    }

    // Step 11: Let pair be the entry in values at index index.
    let pair = values[index].clone();

    // Step 12: Set object's index to index + 1.
    iterator.index.set(index + 1);

    // Step 13: Return the iterator result for pair and kind.
    iterator_result(pair, kind, ec)
}

/// <https://webidl.spec.whatwg.org/#iterator-result>
fn iterator_result(
    pair: (JsValue, JsValue),
    kind: DefaultIteratorKind,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    // Step 1: Let result be a value determined by the value of kind:
    let (key, value) = pair;
    let result = match kind {
        // "key": Let idlKey be pair's key. Let key be the result of
        // converting idlKey to a JavaScript value. result is key.
        DefaultIteratorKind::Key => key,
        // "value": Let idlValue be pair's value. Let value be the result of
        // converting idlValue to a JavaScript value. result is value.
        DefaultIteratorKind::Value => value,
        // "key+value": Let idlKey be pair's key. Let idlValue be pair's
        // value. Let key be the result of converting idlKey to a JavaScript
        // value. Let value be the result of converting idlValue to a
        // JavaScript value. Let array be ! ArrayCreate(2). Call
        // ! CreateDataProperty(array, "0", key). Call
        // ! CreateDataProperty(array, "1", value). result is array.
        DefaultIteratorKind::KeyPlusValue => {
            let array = ec.create_empty_array();
            let zero = ec.property_key_from_index(0);
            ec.create_data_property(array.clone(), zero, key)?;
            let one = ec.property_key_from_index(1);
            ec.create_data_property(array.clone(), one, value)?;
            Types::value_from_object(array)
        }
    };

    // Step 2: Return CreateIteratorResultObject(result, false).
    let result = create_iterator_result_object(result, false, ec)?;
    Ok(Types::value_from_object(result))
}

/// <https://tc39.es/ecma262/#sec-createiterresultobject>
fn create_iterator_result_object(
    value: JsValue,
    done: bool,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    // Step 1: Let obj be OrdinaryObjectCreate(%Object.prototype%).
    let realm = ec.current_realm();
    let intrinsics = ec.realm_intrinsics(&realm);
    let object = ec.create_plain_object(Some(&intrinsics.object_prototype));

    // Step 2: Perform ! CreateDataPropertyOrThrow(obj, "value", value).
    let value_key = ec.property_key_from_str("value");
    ec.create_data_property(object.clone(), value_key, value)?;

    // Step 3: Perform ! CreateDataPropertyOrThrow(obj, "done", done).
    let done_key = ec.property_key_from_str("done");
    let done = ec.value_from_bool(done);
    ec.create_data_property(object.clone(), done_key, done)?;

    // Step 4: Return obj.
    Ok(object)
}

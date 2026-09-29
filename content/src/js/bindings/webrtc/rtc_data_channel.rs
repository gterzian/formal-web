use crate::js::Types;
use crate::webidl::bindings::{InterfaceDefinition, WebIdlInterface};
use crate::webrtc::RTCDataChannel;
use crate::webrtc::rtc_data_channel::{BinaryType, SendData};
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::{event_handlers, member, this_as};

type JsValue = <Types as JsTypes>::JsValue;

fn channel(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<RTCDataChannel, Types> {
    this_as::<RTCDataChannel>(this, "RTCDataChannel", ec)
}

impl WebIdlInterface<Types> for RTCDataChannel {
    const NAME: &'static str = "RTCDataChannel";

    fn parent_name() -> Option<&'static str> {
        Some("EventTarget")
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        member!(def, attribute "label", label);
        member!(def, attribute "ordered", ordered);
        member!(def, attribute "maxPacketLifeTime", max_packet_life_time);
        member!(def, attribute "maxRetransmits", max_retransmits);
        member!(def, attribute "protocol", protocol);
        member!(def, attribute "negotiated", negotiated);
        member!(def, attribute "id", id);
        member!(def, attribute "readyState", ready_state);
        member!(def, attribute "bufferedAmount", buffered_amount);
        member!(def, attribute "bufferedAmountLowThreshold", get_buffered_amount_low_threshold, set_buffered_amount_low_threshold);
        member!(def, attribute "binaryType", get_binary_type, set_binary_type);
        member!(def, attribute "onopen", get_onopen, set_onopen);
        member!(def, attribute "onbufferedamountlow", get_onbufferedamountlow, set_onbufferedamountlow);
        member!(def, attribute "onerror", get_onerror, set_onerror);
        member!(def, attribute "onclosing", get_onclosing, set_onclosing);
        member!(def, attribute "onclose", get_onclose, set_onclose);
        member!(def, attribute "onmessage", get_onmessage, set_onmessage);
        member!(def, operation "send", 1, send, promise false);
        member!(def, operation "close", 0, close, promise false);
    }
}

fn string(value: &str, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
    let string = ec.js_string_from_str(value);
    ec.value_from_string(string)
}

fn nullable_number(value: Option<u16>, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
    match value {
        Some(value) => ec.value_from_number(f64::from(value)),
        None => ec.value_null(),
    }
}

fn label(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let label = channel(this, ec)?.slots.borrow().label.clone();
    Ok(string(&label, ec))
}

fn ordered(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let ordered = channel(this, ec)?.slots.borrow().ordered;
    Ok(ec.value_from_bool(ordered))
}

fn max_packet_life_time(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value = channel(this, ec)?.slots.borrow().max_packet_life_time;
    Ok(nullable_number(value, ec))
}

fn max_retransmits(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value = channel(this, ec)?.slots.borrow().max_retransmits;
    Ok(nullable_number(value, ec))
}

fn protocol(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let protocol = channel(this, ec)?.slots.borrow().protocol.clone();
    Ok(string(&protocol, ec))
}

fn negotiated(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let negotiated = channel(this, ec)?.slots.borrow().negotiated;
    Ok(ec.value_from_bool(negotiated))
}

fn id(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let id = channel(this, ec)?.slots.borrow().id;
    Ok(nullable_number(id, ec))
}

fn ready_state(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let state = channel(this, ec)?.slots.borrow().ready_state;
    Ok(string(state.as_idl(), ec))
}

fn buffered_amount(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let amount = channel(this, ec)?.slots.borrow().buffered_amount;
    Ok(ec.value_from_number(amount as f64))
}

fn get_buffered_amount_low_threshold(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let threshold = channel(this, ec)?
        .slots
        .borrow()
        .buffered_amount_low_threshold;
    Ok(ec.value_from_number(threshold as f64))
}

fn set_buffered_amount_low_threshold(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let channel = channel(this, ec)?;
    let undefined = ec.value_undefined();
    // unsigned long long.
    let number = ec.to_number(args.first().cloned().unwrap_or(undefined))?;
    let threshold = if number.is_finite() {
        number.trunc().max(0.0) as u64
    } else {
        0
    };
    channel.set_buffered_amount_low_threshold(threshold, ec);
    Ok(ec.value_undefined())
}

fn get_binary_type(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let binary_type = channel(this, ec)?.slots.borrow().binary_type;
    Ok(string(binary_type.as_idl(), ec))
}

fn set_binary_type(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let channel = channel(this, ec)?;
    let undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
    // <https://webidl.spec.whatwg.org/#es-attributes>: an enumeration
    // attribute ignores values outside the enumeration.
    let binary_type = match value.as_str() {
        "blob" => BinaryType::Blob,
        "arraybuffer" => BinaryType::ArrayBuffer,
        _ => return Ok(ec.value_undefined()),
    };
    channel.slots.borrow_mut().binary_type = binary_type;
    Ok(ec.value_undefined())
}

fn send(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let channel = channel(this, ec)?;
    let undefined = ec.value_undefined();
    let data = args.first().cloned().unwrap_or(undefined);
    // <https://webidl.spec.whatwg.org/#dfn-overload-resolution>
    let data = if crate::webidl::is_buffer_source(&data, ec) {
        SendData::Bytes(crate::webidl::get_a_copy_of_the_buffer_source(&data, ec)?)
    } else if Types::value_as_object(&data)
        .is_some_and(|object| Types::object_as_data_view(&object).is_some())
    {
        let object = Types::value_as_object(&data).expect("checked above");
        let view = Types::object_as_data_view(&object).expect("checked above");
        let buffer = ec.data_view_buffer(&view)?;
        let offset = ec.data_view_byte_offset(&view)? as usize;
        let length = ec.data_view_byte_length(&view)? as usize;
        let bytes = ec.array_buffer_data(&buffer).unwrap_or_default();
        SendData::Bytes(
            bytes
                .get(offset..offset + length)
                .unwrap_or_default()
                .to_vec(),
        )
    } else {
        // Note: Blob is not implemented, so no value selects a Blob overload.
        SendData::Text(ec.to_rust_string(data)?)
    };
    channel.send(data, ec)?;
    Ok(ec.value_undefined())
}

fn close(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let channel = channel(this, ec)?;
    channel.close(ec);
    Ok(ec.value_undefined())
}

event_handlers!(
    get_onopen, set_onopen, "open";
    get_onbufferedamountlow, set_onbufferedamountlow, "bufferedamountlow";
    get_onerror, set_onerror, "error";
    get_onclosing, set_onclosing, "closing";
    get_onclose, set_onclose, "close";
    get_onmessage, set_onmessage, "message";
);

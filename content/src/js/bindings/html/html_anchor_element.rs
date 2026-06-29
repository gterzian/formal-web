use boa_engine::{JsArgs, JsNativeError, JsResult, JsString, JsValue};
use std::marker::PhantomData;

use crate::html::HTMLAnchorElement;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};

use super::hyperlink_element_utils::document_creation_url;

use js_engine::{Completion, ExecutionContext, JsTypes};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface<crate::js::Types> for HTMLAnchorElement {
    const NAME: &'static str = "HTMLAnchorElement";

    fn parent_name() -> Option<&'static str> {
        Some("HTMLElement")
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        // HTMLAnchorElement own attributes
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

            id: "href",
            getter: get_href,
            setter: Some(set_href),
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

            id: "target",
            getter: get_target,
            setter: Some(set_target),
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

            id: "download",
            getter: get_download,
            setter: Some(set_download),
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

            id: "rel",
            getter: get_rel,
            setter: Some(set_rel),
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

            id: "referrerPolicy",
            getter: get_referrer_policy,
            setter: Some(set_referrer_policy),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
    }
}

fn with_html_anchor_element_ref<R>(
    this: &JsValue,
    f: impl FnOnce(&HTMLAnchorElement) -> R,
) -> JsResult<R> {
    let object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("HTMLAnchorElement receiver is not an object")
    })?;
    let html_anchor_element = object
        .downcast_ref::<HTMLAnchorElement>()
        .ok_or_else(|| JsNativeError::typ().with_message("receiver is not an HTMLAnchorElement"))?;
    Ok(f(&html_anchor_element))
}

fn try_with_html_anchor_element_ref<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&HTMLAnchorElement) -> R,
) -> Completion<R, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("HTMLAnchorElement receiver is not an object"))?;
    let html_anchor_element = obj
        .downcast_ref::<HTMLAnchorElement>()
        .ok_or_else(|| ec.new_type_error("receiver is not an HTMLAnchorElement"))?;
    Ok(f(&html_anchor_element))
}

fn get_href(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let base_url = document_creation_url(ctx)?;
        with_html_anchor_element_ref(this, |anchor| {
            JsValue::from(JsString::from(anchor.href(&base_url)))
        })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_href(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let href = args
            .get_or_undefined(0)
            .to_string(ctx)?
            .to_std_string_escaped();
        with_html_anchor_element_ref(this, |anchor| anchor.set_href(&href))?;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_target(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let target = try_with_html_anchor_element_ref(this, ec, |anchor| anchor.target())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&target)))
}

fn set_target(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let target = args
            .get_or_undefined(0)
            .to_string(ctx)?
            .to_std_string_escaped();
        with_html_anchor_element_ref(this, |anchor| anchor.set_target(&target))?;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_download(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let download = try_with_html_anchor_element_ref(this, ec, |anchor| anchor.download())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&download)))
}

fn set_download(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let download = args
            .get_or_undefined(0)
            .to_string(ctx)?
            .to_std_string_escaped();
        with_html_anchor_element_ref(this, |anchor| anchor.set_download(&download))?;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_rel(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let rel = try_with_html_anchor_element_ref(this, ec, |anchor| anchor.rel())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&rel)))
}

fn set_rel(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let rel = args
            .get_or_undefined(0)
            .to_string(ctx)?
            .to_std_string_escaped();
        with_html_anchor_element_ref(this, |anchor| anchor.set_rel(&rel))?;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_referrer_policy(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let referrer_policy =
        try_with_html_anchor_element_ref(this, ec, |anchor| anchor.referrer_policy())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&referrer_policy)))
}

fn set_referrer_policy(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let referrer_policy = args
            .get_or_undefined(0)
            .to_string(ctx)?
            .to_std_string_escaped();
        with_html_anchor_element_ref(this, |anchor| anchor.set_referrer_policy(&referrer_policy))?;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

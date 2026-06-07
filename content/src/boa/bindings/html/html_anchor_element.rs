use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsString, JsValue,
    class::{Class, ClassBuilder},
};

use crate::html::HTMLAnchorElement;
use crate::webidl::binding::{
    AttributeDef, InterfaceDefinition, WebIdlInterface, register_interface,
};

use super::hyperlink_element_utils::document_creation_url;

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface for HTMLAnchorElement {
    const NAME: &'static str = "HTMLAnchorElement";

    fn parent_name() -> Option<&'static str> {
        Some("HTMLElement")
    }

    fn define_members(def: &mut InterfaceDefinition) {
        // HTMLAnchorElement own attributes
        def.add_attribute(AttributeDef {
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

// ── Boa Class glue ──

impl Class for HTMLAnchorElement {
    const NAME: &'static str = "HTMLAnchorElement";

    fn data_constructor(
        _this: &JsValue,
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<Self> {
        Err(JsNativeError::typ()
            .with_message("Illegal constructor")
            .into())
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        register_interface::<HTMLAnchorElement>(class)?;
        // HTMLHyperlinkElementUtils members
        super::hyperlink_element_utils::register_hyperlink_element_utils_methods(class)?;
        Ok(())
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

fn get_href(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let base_url = document_creation_url(context)?;
    with_html_anchor_element_ref(this, |anchor| {
        JsValue::from(JsString::from(anchor.href(&base_url)))
    })
}

fn set_href(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let href = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_html_anchor_element_ref(this, |anchor| anchor.set_href(&href))?;
    Ok(JsValue::undefined())
}

fn get_target(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_anchor_element_ref(this, |anchor| {
        JsValue::from(JsString::from(anchor.target()))
    })
}

fn set_target(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let target = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_html_anchor_element_ref(this, |anchor| anchor.set_target(&target))?;
    Ok(JsValue::undefined())
}

fn get_download(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_anchor_element_ref(this, |anchor| {
        JsValue::from(JsString::from(anchor.download()))
    })
}

fn set_download(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let download = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_html_anchor_element_ref(this, |anchor| anchor.set_download(&download))?;
    Ok(JsValue::undefined())
}

fn get_rel(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_anchor_element_ref(this, |anchor| JsValue::from(JsString::from(anchor.rel())))
}

fn set_rel(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let rel = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_html_anchor_element_ref(this, |anchor| anchor.set_rel(&rel))?;
    Ok(JsValue::undefined())
}

fn get_referrer_policy(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_anchor_element_ref(this, |anchor| {
        JsValue::from(JsString::from(anchor.referrer_policy()))
    })
}

fn set_referrer_policy(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let referrer_policy = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_html_anchor_element_ref(this, |anchor| anchor.set_referrer_policy(&referrer_policy))?;
    Ok(JsValue::undefined())
}

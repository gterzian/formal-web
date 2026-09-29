type JsValue = <crate::js::Types as JsTypes>::JsValue;
type Types = crate::js::Types;

use crate::html::{
    HTMLAnchorElement, HTMLCanvasElement, HTMLElement, HTMLIFrameElement, HTMLInputElement,
    HTMLLinkElement, HTMLScriptElement,
};
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};

use js_engine::{Completion, ExecutionContext, JsTypes};

impl WebIdlInterface<crate::js::Types> for HTMLElement {
    const NAME: &'static str = "HTMLElement";

    fn parent_name() -> Option<&'static str> {
        Some("Element")
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        def.add_attribute(AttributeDef {
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
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "lang",
            getter: get_lang,
            setter: Some(set_lang),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "dir",
            getter: get_dir,
            setter: Some(set_dir),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "hidden",
            getter: get_hidden,
            setter: Some(set_hidden),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "style",
            getter: get_style,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "click",
            length: 0,
            method: click_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
    }
}

fn click_method(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let object = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("click receiver is not an object"))?;
    // Clone the HTMLElement out of the object registry so the domain method
    // can use `ec` (it creates and dispatches the click event).
    let html_element = ec.with_object_any(&object).and_then(|data| {
        data.downcast_ref::<HTMLElement>()
            .cloned()
            .or_else(|| {
                data.downcast_ref::<HTMLAnchorElement>()
                    .map(|anchor| anchor.html_element.clone())
            })
            .or_else(|| {
                data.downcast_ref::<HTMLCanvasElement>()
                    .map(|canvas| canvas.html_element.clone())
            })
            .or_else(|| {
                data.downcast_ref::<HTMLInputElement>()
                    .map(|input| input.html_element.clone())
            })
            .or_else(|| {
                data.downcast_ref::<HTMLIFrameElement>()
                    .map(|iframe| iframe.html_element.clone())
            })
            .or_else(|| {
                data.downcast_ref::<HTMLScriptElement>()
                    .map(|script| script.html_element.clone())
            })
            .or_else(|| {
                data.downcast_ref::<HTMLLinkElement>()
                    .map(|link| link.html_element.clone())
            })
    });
    let Some(html_element) = html_element else {
        return Err(ec.new_type_error("receiver is not an HTMLElement"));
    };
    html_element.click(ec)?;
    Ok(ec.value_undefined())
}

fn try_with_html_element_ref<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&HTMLElement) -> R,
) -> Completion<R, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("HTMLElement receiver is not an object"))?;

    if let Some(data) = ec.with_object_any(&obj) {
        if let Some(html_element) = data.downcast_ref::<HTMLElement>() {
            return Ok(f(html_element));
        }
        if let Some(anchor) = data.downcast_ref::<HTMLAnchorElement>() {
            return Ok(f(&anchor.html_element));
        }
        if let Some(canvas) = data.downcast_ref::<HTMLCanvasElement>() {
            return Ok(f(&canvas.html_element));
        }
        if let Some(input) = data.downcast_ref::<HTMLInputElement>() {
            return Ok(f(&input.html_element));
        }
        if let Some(iframe) = data.downcast_ref::<HTMLIFrameElement>() {
            return Ok(f(&iframe.html_element));
        }
        if let Some(script) = data.downcast_ref::<HTMLScriptElement>() {
            return Ok(f(&script.html_element));
        }
        if let Some(link) = data.downcast_ref::<HTMLLinkElement>() {
            return Ok(f(&link.html_element));
        }
    }
    Err(ec.new_type_error("receiver is not an HTMLElement"))
}

fn get_title(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let title = try_with_html_element_ref(this, ec, |html_element| html_element.title())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&title)))
}

fn set_title(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undef = ec.value_undefined();
    let title = ec.to_rust_string(args.first().cloned().unwrap_or(undef))?;
    try_with_html_element_ref(this, ec, |html_element| html_element.set_title(&title))?;
    Ok(ec.value_undefined())
}

fn get_lang(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let lang = try_with_html_element_ref(this, ec, |html_element| html_element.lang())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&lang)))
}

fn set_lang(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undef = ec.value_undefined();
    let lang = ec.to_rust_string(args.first().cloned().unwrap_or(undef))?;
    try_with_html_element_ref(this, ec, |html_element| html_element.set_lang(&lang))?;
    Ok(ec.value_undefined())
}

fn get_dir(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let dir = try_with_html_element_ref(this, ec, |html_element| html_element.dir())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&dir)))
}

fn set_dir(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undef = ec.value_undefined();
    let dir = ec.to_rust_string(args.first().cloned().unwrap_or(undef))?;
    try_with_html_element_ref(this, ec, |html_element| html_element.set_dir(&dir))?;
    Ok(ec.value_undefined())
}

fn get_hidden(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let hidden = try_with_html_element_ref(this, ec, |html_element| html_element.hidden())?;
    Ok(ec.value_from_bool(hidden))
}

fn set_hidden(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let hidden = args.first().is_some_and(|v| ec.to_boolean(v));
    try_with_html_element_ref(this, ec, |html_element| html_element.set_hidden(hidden))?;
    Ok(ec.value_undefined())
}

fn get_style(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let element = try_with_html_element_ref(this, ec, |html_element| html_element.element.clone())?;
    let declaration_block = element.style(ec)?;
    declaration_block
        .reflector
        .clone()
        .map(crate::js::Types::value_from_object)
        .ok_or_else(|| ec.new_type_error("CSSStyleDeclaration has no reflector"))
}

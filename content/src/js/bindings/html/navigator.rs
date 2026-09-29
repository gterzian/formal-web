use crate::html::Navigator;
use crate::js::Types;
use crate::js::platform_objects::with_global_scope;
#[cfg(feature = "webrtc")]
use crate::mediacapture_streams::MediaDevices;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};
use crate::webidl::create_a_frozen_array_of_strings;
use js_engine::{Completion, ExecutionContext, JsTypes};

type JsValue = <Types as JsTypes>::JsValue;

fn navigator(this: &JsValue, ec: &mut dyn ExecutionContext<Types>) -> Completion<Navigator, Types> {
    let object = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Navigator receiver is not an object"))?;
    ec.with_object_any(&object)
        .and_then(|data| data.downcast_ref::<Navigator>().cloned())
        .ok_or_else(|| ec.new_type_error("receiver is not a Navigator"))
}

macro_rules! readonly_attribute {
    ($def:ident, $id:literal, $getter:ident) => {
        $def.add_attribute(AttributeDef {
            id: $id,
            getter: $getter,
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
    };
}

macro_rules! operation {
    ($def:ident, $id:literal, $method:ident) => {
        $def.add_operation(OperationDef {
            id: $id,
            length: 0,
            method: $method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
    };
}

impl WebIdlInterface<Types> for Navigator {
    const NAME: &'static str = "Navigator";

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        readonly_attribute!(def, "appCodeName", get_app_code_name);
        #[cfg(feature = "webrtc")]
        readonly_attribute!(def, "mediaDevices", get_media_devices);
        readonly_attribute!(def, "appName", get_app_name);
        readonly_attribute!(def, "appVersion", get_app_version);
        readonly_attribute!(def, "platform", get_platform);
        readonly_attribute!(def, "product", get_product);
        readonly_attribute!(def, "productSub", get_product_sub);
        readonly_attribute!(def, "userAgent", get_user_agent);
        readonly_attribute!(def, "vendor", get_vendor);
        readonly_attribute!(def, "vendorSub", get_vendor_sub);
        readonly_attribute!(def, "oscpu", get_oscpu);
        operation!(def, "taintEnabled", taint_enabled);
        readonly_attribute!(def, "language", get_language);
        readonly_attribute!(def, "languages", get_languages);
        readonly_attribute!(def, "onLine", get_on_line);
        readonly_attribute!(def, "cookieEnabled", get_cookie_enabled);
        readonly_attribute!(def, "hardwareConcurrency", get_hardware_concurrency);
        readonly_attribute!(def, "pdfViewerEnabled", get_pdf_viewer_enabled);
        operation!(def, "javaEnabled", java_enabled);
    }
}

macro_rules! string_getter {
    ($name:ident, $method:ident) => {
        fn $name(
            this: &JsValue,
            _args: &[JsValue],
            ec: &mut dyn ExecutionContext<Types>,
        ) -> Completion<JsValue, Types> {
            let value = navigator(this, ec)?.$method();
            let string = ec.js_string_from_str(&value);
            Ok(ec.value_from_string(string))
        }
    };
}

macro_rules! boolean_member {
    ($name:ident, $method:ident) => {
        fn $name(
            this: &JsValue,
            _args: &[JsValue],
            ec: &mut dyn ExecutionContext<Types>,
        ) -> Completion<JsValue, Types> {
            let value = navigator(this, ec)?.$method();
            Ok(ec.value_from_bool(value))
        }
    };
}

string_getter!(get_app_code_name, app_code_name);
string_getter!(get_app_name, app_name);
string_getter!(get_app_version, app_version);
string_getter!(get_platform, platform);
string_getter!(get_product, product);
string_getter!(get_product_sub, product_sub);
string_getter!(get_user_agent, user_agent);
string_getter!(get_vendor, vendor);
string_getter!(get_vendor_sub, vendor_sub);
string_getter!(get_oscpu, oscpu);
string_getter!(get_language, language);
boolean_member!(taint_enabled, taint_enabled);
boolean_member!(get_on_line, on_line);
boolean_member!(get_cookie_enabled, cookie_enabled);
boolean_member!(get_pdf_viewer_enabled, pdf_viewer_enabled);
boolean_member!(java_enabled, java_enabled);

fn get_languages(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let languages = navigator(this, ec)?.languages();
    // The realm's global scope keeps the FrozenArray this list was
    // converted to, so the getter returns the same object on every access.
    let array = with_global_scope(ec, |global_scope, ec| {
        if let Some(array) = global_scope.navigator_languages_object(ec) {
            return Ok(array);
        }
        let array = create_a_frozen_array_of_strings(&languages, ec)?;
        global_scope.store_navigator_languages_object(array.clone(), ec);
        Ok(array)
    })?;
    Ok(Types::value_from_object(array))
}

fn get_hardware_concurrency(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let concurrency = navigator(this, ec)?.hardware_concurrency();
    Ok(ec.value_from_number(concurrency as f64))
}

#[cfg(feature = "webrtc")]
fn get_media_devices(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let navigator = navigator(this, ec)?;
    let media_devices: MediaDevices = navigator.media_devices_value(ec)?;
    media_devices
        .event_target
        .reflector
        .clone()
        .map(Types::value_from_object)
        .ok_or_else(|| ec.new_type_error("MediaDevices without its object"))
}

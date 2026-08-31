use crate::html::WorkerNavigator;
use crate::js::Types;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};
use js_engine::{Completion, ExecutionContext, JsTypes};

type JsValue = <Types as JsTypes>::JsValue;

fn with_worker_navigator_ref(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&WorkerNavigator, &mut dyn ExecutionContext<Types>) -> Completion<JsValue, Types>,
) -> Completion<JsValue, Types> {
    let object = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("WorkerNavigator receiver is not an object"))?;
    let navigator = ec
        .with_object_any(&object)
        .and_then(|data| data.downcast_ref::<WorkerNavigator>().cloned());
    let Some(navigator) = navigator else {
        return Err(ec.new_type_error("receiver is not a WorkerNavigator"));
    };
    f(&navigator, ec)
}

/// The NavigatorID / NavigatorLanguage / NavigatorOnLine /
/// NavigatorConcurrentHardware mixin members of WorkerNavigator.
/// <https://html.spec.whatwg.org/#the-workernavigator-object>
impl WebIdlInterface<Types> for WorkerNavigator {
    const NAME: &'static str = "WorkerNavigator";

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        def.add_attribute(AttributeDef {
            id: "userAgent",
            getter: get_user_agent,
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
        def.add_attribute(AttributeDef {
            id: "platform",
            getter: get_platform,
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
        def.add_attribute(AttributeDef {
            id: "language",
            getter: get_language,
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
        def.add_attribute(AttributeDef {
            id: "onLine",
            getter: get_on_line,
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
        def.add_attribute(AttributeDef {
            id: "hardwareConcurrency",
            getter: get_hardware_concurrency,
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
    }
}

macro_rules! define_worker_navigator_getter {
    ($name:ident, $method:ident) => {
        fn $name(
            this: &JsValue,
            _args: &[JsValue],
            ec: &mut dyn ExecutionContext<Types>,
        ) -> Completion<JsValue, Types> {
            with_worker_navigator_ref(this, ec, |navigator, ec| {
                Ok(ec.value_from_string(ec.js_string_from_str(&navigator.$method())))
            })
        }
    };
}

define_worker_navigator_getter!(get_user_agent, user_agent);
define_worker_navigator_getter!(get_platform, platform);
define_worker_navigator_getter!(get_language, language);

fn get_on_line(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    with_worker_navigator_ref(this, ec, |navigator, ec| {
        Ok(ec.value_from_bool(navigator.on_line()))
    })
}

fn get_hardware_concurrency(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    with_worker_navigator_ref(this, ec, |navigator, ec| {
        Ok(ec.value_from_number(navigator.hardware_concurrency() as f64))
    })
}

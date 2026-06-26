use crate::JsTypes;

/// <https://tc39.es/ecma262/#sec-iterator-record>
#[derive(Debug, Clone)]
pub struct IteratorRecord<T: JsTypes> {
    pub iterator: T::JsObject,
    pub next_method: T::Function,
    pub done: bool,
}

/// <https://tc39.es/ecma262/#sec-promisecapability-records>
#[derive(Debug, Clone)]
pub struct PromiseCapability<T: JsTypes> {
    pub promise: T::JsValue,
    pub resolve: T::Function,
    pub reject: T::Function,
}

/// <https://tc39.es/ecma262/#sec-property-descriptor-specification-type>
#[derive(Debug, Clone)]
pub struct PropertyDescriptor<T: JsTypes> {
    pub value: Option<T::JsValue>,
    pub writable: Option<bool>,
    pub get: Option<T::Function>,
    pub set: Option<T::Function>,
    pub enumerable: Option<bool>,
    pub configurable: Option<bool>,
}

/// <https://tc39.es/ecma262/#table-basic-intrinsics>
#[derive(Debug, Clone)]
pub struct RealmIntrinsics<T: JsTypes> {
    pub array_buffer: T::Constructor,
    pub shared_array_buffer: T::Constructor,
    pub promise: T::Constructor,
    pub object: T::Constructor,
    pub function: T::Constructor,
    pub error: T::Constructor,
    pub type_error: T::Constructor,
    pub range_error: T::Constructor,
    pub syntax_error: T::Constructor,
    pub reference_error: T::Constructor,
    pub uri_error: T::Constructor,
    pub eval_error: T::Constructor,
    pub array: T::Constructor,
    pub object_prototype: T::JsObject,
    pub function_prototype: T::JsObject,
}

/// <https://html.spec.whatwg.org/#hostloadimportedmodule>
#[derive(Debug, Clone)]
pub struct ModuleRequest<T: JsTypes> {
    pub specifier: T::JsString,
    pub attributes: Vec<(T::JsString, T::JsValue)>,
}

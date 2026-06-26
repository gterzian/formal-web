/// <https://tc39.es/ecma262/#sec-tonumeric>
#[derive(Debug, Clone, PartialEq)]
pub enum Numeric<T: crate::JsTypes> {
    Number(f64),
    BigInt(T::JsBigInt),
}

/// <https://tc39.es/ecma262/#sec-toprimitive>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferredType {
    String,
    Number,
}

/// <https://tc39.es/ecma262/#sec-setintegritylevel>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrityLevel {
    Sealed,
    Frozen,
}

/// <https://tc39.es/ecma262/#sec-iterator-interface>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IteratorKind {
    Sync,
    Async,
}

/// <https://tc39.es/ecma262/#table-typedarray-element-types>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedArrayElementType {
    Int8,
    Uint8,
    Uint8Clamped,
    Int16,
    Uint16,
    Int32,
    Uint32,
    Float16,
    Float32,
    Float64,
    BigInt64,
    BigUint64,
}

/// <https://tc39.es/ecma262/#sec-getvaluefrombuffer>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedMemoryOrder {
    SeqCst,
    Unordered,
    Init,
}

/// <https://html.spec.whatwg.org/#hostpromiserejectiontracker>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromiseRejectionOperation {
    Reject,
    Handle,
}

impl PreferredType {
    pub fn is_string(self) -> bool {
        matches!(self, PreferredType::String)
    }
    pub fn is_number(self) -> bool {
        matches!(self, PreferredType::Number)
    }
}

impl IntegrityLevel {
    pub fn is_sealed(self) -> bool {
        matches!(self, IntegrityLevel::Sealed)
    }
    pub fn is_frozen(self) -> bool {
        matches!(self, IntegrityLevel::Frozen)
    }
}

impl IteratorKind {
    pub fn is_sync(self) -> bool {
        matches!(self, IteratorKind::Sync)
    }
    pub fn is_async(self) -> bool {
        matches!(self, IteratorKind::Async)
    }
}

use js_engine::{Completion, ExecutionContext, JsTypes, gc_struct};

use crate::js::Types;
use crate::webidl::bindings::create_interface_instance;

type JsObject = <Types as JsTypes>::JsObject;

/// <https://drafts.csswg.org/resize-observer/#resizeobserversize>
// Note: the value pair a box size calculation yields; a ResizeObserverSize
// platform object is created from it when an entry exposes it to script.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Size {
    pub(crate) inline_size: f64,
    pub(crate) block_size: f64,
}

/// <https://drafts.csswg.org/resize-observer/#resizeobserversize>
#[gc_struct]
pub struct ResizeObserverSize {
    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobserversize-inlinesize>
    #[ignore_trace]
    inline_size: f64,

    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobserversize-blocksize>
    #[ignore_trace]
    block_size: f64,

    pub(crate) reflector: Option<JsObject>,
}

impl ResizeObserverSize {
    /// <https://drafts.csswg.org/resize-observer/#resizeobserversize>
    pub(crate) fn new(
        size: Size,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<ResizeObserverSize, Types> {
        let object_size = ResizeObserverSize {
            inline_size: size.inline_size,
            block_size: size.block_size,
            reflector: None,
        };
        let object = create_interface_instance::<Types, ResizeObserverSize>(object_size, ec)?;
        ec.with_object_any(&object)
            .and_then(|data| data.downcast_ref::<ResizeObserverSize>().cloned())
            .ok_or_else(|| {
                ec.new_type_error("ResizeObserverSize instance is not a ResizeObserverSize")
            })
    }

    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobserversize-inlinesize>
    pub(crate) fn inline_size(&self) -> f64 {
        self.inline_size
    }

    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobserversize-blocksize>
    pub(crate) fn block_size(&self) -> f64 {
        self.block_size
    }
}

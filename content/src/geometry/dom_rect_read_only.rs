use js_engine::{Completion, ExecutionContext, JsTypes, gc_struct};

use crate::js::Types;
use crate::webidl::bindings::create_interface_instance;

type JsObject = <Types as JsTypes>::JsObject;

/// <https://drafts.fxtf.org/geometry/#domrectreadonly>
#[gc_struct]
pub struct DOMRectReadOnly {
    /// <https://drafts.fxtf.org/geometry/#dom-domrectreadonly-x>
    #[ignore_trace]
    x: f64,

    /// <https://drafts.fxtf.org/geometry/#dom-domrectreadonly-y>
    #[ignore_trace]
    y: f64,

    /// <https://drafts.fxtf.org/geometry/#dom-domrectreadonly-width>
    #[ignore_trace]
    width: f64,

    /// <https://drafts.fxtf.org/geometry/#dom-domrectreadonly-height>
    #[ignore_trace]
    height: f64,

    pub(crate) reflector: Option<JsObject>,
}

impl DOMRectReadOnly {
    /// <https://drafts.fxtf.org/geometry/#dom-domrectreadonly-domrectreadonly>
    pub(crate) fn new(
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<DOMRectReadOnly, Types> {
        // "The DOMRectReadOnly(x, y, width, height) constructor, when invoked, must create a new DOMRectReadOnly object with its x coordinate, y coordinate, width dimension and height dimension set to x, y, width and height respectively."
        let rect = DOMRectReadOnly {
            x,
            y,
            width,
            height,
            reflector: None,
        };
        let object = create_interface_instance::<Types, DOMRectReadOnly>(rect, ec)?;
        ec.with_object_any(&object)
            .and_then(|data| data.downcast_ref::<DOMRectReadOnly>().cloned())
            .ok_or_else(|| ec.new_type_error("DOMRectReadOnly instance is not a DOMRectReadOnly"))
    }

    /// <https://drafts.fxtf.org/geometry/#dom-domrectreadonly-x>
    pub(crate) fn x(&self) -> f64 {
        // "The x attribute, on getting, must return the x coordinate."
        self.x
    }

    /// <https://drafts.fxtf.org/geometry/#dom-domrectreadonly-y>
    pub(crate) fn y(&self) -> f64 {
        // "The y attribute, on getting, must return the y coordinate."
        self.y
    }

    /// <https://drafts.fxtf.org/geometry/#dom-domrectreadonly-width>
    pub(crate) fn width(&self) -> f64 {
        // "The width attribute, on getting, must return the width dimension."
        self.width
    }

    /// <https://drafts.fxtf.org/geometry/#dom-domrectreadonly-height>
    pub(crate) fn height(&self) -> f64 {
        // "The height attribute, on getting, must return the height dimension."
        self.height
    }

    /// <https://drafts.fxtf.org/geometry/#dom-domrectreadonly-top>
    pub(crate) fn top(&self) -> f64 {
        // "The top attribute, on getting, must return the minimum of the y coordinate and the sum of the y coordinate and the height dimension."
        self.y.min(self.y + self.height)
    }

    /// <https://drafts.fxtf.org/geometry/#dom-domrectreadonly-right>
    pub(crate) fn right(&self) -> f64 {
        // "The right attribute, on getting, must return the maximum of the x coordinate and the sum of the x coordinate and the width dimension."
        self.x.max(self.x + self.width)
    }

    /// <https://drafts.fxtf.org/geometry/#dom-domrectreadonly-bottom>
    pub(crate) fn bottom(&self) -> f64 {
        // "The bottom attribute, on getting, must return the maximum of the y coordinate and the sum of the y coordinate and the height dimension."
        self.y.max(self.y + self.height)
    }

    /// <https://drafts.fxtf.org/geometry/#dom-domrectreadonly-left>
    pub(crate) fn left(&self) -> f64 {
        // "The left attribute, on getting, must return the minimum of the x coordinate and the sum of the x coordinate and the width dimension."
        self.x.min(self.x + self.width)
    }
}

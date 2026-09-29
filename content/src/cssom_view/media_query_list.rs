use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use blitz_dom::BaseDocument;
use js_engine::{ExecutionContext, gc_struct};
use style::context::QuirksMode;
use style::media_queries::MediaList;
use style::parser::ParserContext;
use style::servo_arc::Arc as ServoArc;
use style::stylesheets::{CssRuleType, CustomMediaEvaluator, Origin, UrlExtraData};
use style_traits::{ParsingMode, ToCss};
use url::Url;

use crate::dom::event::{EventTarget, EventTargetAccess};
use crate::html::Window;
use crate::js::Types;
use crate::webidl::Callback;

/// <https://drafts.csswg.org/cssom-view/#mediaquerylist>
#[gc_struct]
pub struct MediaQueryList {
    /// <https://dom.spec.whatwg.org/#interface-eventtarget>
    pub event_target: EventTarget,

    /// <https://drafts.csswg.org/cssom-view/#mediaquerylist-document>
    #[ignore_trace]
    document: Rc<RefCell<BaseDocument>>,

    /// <https://drafts.csswg.org/cssom-view/#mediaquerylist-media-query-list>
    #[ignore_trace]
    media_query_list: Rc<MediaList>,
}

impl EventTargetAccess for MediaQueryList {
    fn get_event_target(&self, _ec: &mut dyn ExecutionContext<Types>) -> EventTarget {
        self.event_target.clone()
    }
}

impl MediaQueryList {
    /// <https://drafts.csswg.org/cssom-view/#dom-mediaquerylist-media>
    pub(crate) fn media(&self) -> String {
        // "The media attribute must return the serialized form of the associated media query list."
        self.media_query_list.to_css_string()
    }

    /// <https://drafts.csswg.org/cssom-view/#dom-mediaquerylist-matches>
    pub(crate) fn matches(&self) -> bool {
        // "The matches attribute must return true if the associated media query list matches the state of the rendered Document and false otherwise."
        let mut document = self.document.borrow_mut();
        let device = document.stylist_device();
        self.media_query_list.evaluate(
            device,
            QuirksMode::NoQuirks,
            &mut CustomMediaEvaluator::none(),
        )
    }

    /// <https://drafts.csswg.org/cssom-view/#dom-mediaquerylist-addlistener>
    pub(crate) fn add_listener(
        &self,
        callback: Option<Callback>,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        // "The addListener(callback) method must run these steps: append an event listener to this's event listener list whose type is "change", callback is callback, and capture is false."
        self.event_target.add_event_listener(
            self.event_target.clone(),
            String::from("change"),
            callback,
            false,
            false,
            None,
            None,
            ec,
        );
    }

    /// <https://drafts.csswg.org/cssom-view/#dom-mediaquerylist-removelistener>
    pub(crate) fn remove_listener(
        &self,
        callback: Option<Callback>,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        // "The removeListener(callback) method must run these steps: remove an event listener from this's event listener list, whose type is "change", callback is callback, and capture is false."
        let Some(callback) = callback else {
            return;
        };
        self.event_target
            .remove_event_listener_entry("change", &callback, false, ec);
    }
}

impl Window {
    /// <https://drafts.csswg.org/cssom-view/#dom-window-matchmedia>
    pub(crate) fn match_media(
        &self,
        query: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> MediaQueryList {
        // Step 1: "Let parsed media query list be the result of parsing query."
        let url_data = UrlExtraData(ServoArc::new(
            Url::parse("about:blank").expect("about:blank is a valid URL"),
        ));
        let context = ParserContext::new(
            Origin::Author,
            &url_data,
            Some(CssRuleType::Media),
            ParsingMode::DEFAULT,
            QuirksMode::NoQuirks,
            Cow::Owned(Default::default()),
            None,
            None,
            Default::default(),
        );
        let mut input = cssparser::ParserInput::new(query);
        let mut parser = cssparser::Parser::new(&mut input);
        let parsed_media_query_list = MediaList::parse(&context, &mut parser);

        // Step 2: "Return a new MediaQueryList object, with this's associated Document as the document, with parsed media query list as its associated media query list."
        // Note: the document is the blitz document the Window's realm renders;
        // "evaluate media queries and report changes" is not run, so a change
        // event never fires.
        MediaQueryList {
            event_target: EventTarget::new(ec),
            document: self.global_scope.document(),
            media_query_list: Rc::new(parsed_media_query_list),
        }
    }
}

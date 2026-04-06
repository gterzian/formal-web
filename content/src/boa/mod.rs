mod bindings;
mod event_handler;
mod execution_context;
mod html_parser;
mod runtime_data;
mod task_queue;

use std::{cell::RefCell, rc::Rc};

use blitz_dom::{BaseDocument, EventDriver};
use blitz_traits::events::UiEvent;
use boa_engine::class::Class;

use crate::dom::Event;

pub use event_handler::BlitzEventHandler;
pub use execution_context::{JsExecutionContext, JsState};
pub use html_parser::{JsHtmlParserProvider, parse_html_into_document};

/// <https://html.spec.whatwg.org/#event-firing>
pub fn dispatch_ui_event(
    document: &Rc<RefCell<BaseDocument>>,
    execution_context: &mut JsExecutionContext,
    event: UiEvent,
) -> Result<(), String> {
    {
        let mut document_guard = document.borrow_mut();
        let handler = BlitzEventHandler::new(execution_context);
        let mut driver = EventDriver::new(&mut *document_guard, handler);
        driver.handle_ui_event(event);
    }
    execution_context.run_microtasks()
}

/// <https://html.spec.whatwg.org/#event-firing>
pub fn fire_load_event(execution_context: &mut JsExecutionContext) -> Result<(), String> {
    let event = Event::from_data(
        Event::new("load".to_owned(), false, false, false, true, 0.0),
        &mut execution_context.context,
    )
    .map_err(|error| error.to_string())?;
    let window = execution_context.context.global_object();
    bindings::dispatch(&window, &event, &mut execution_context.context)
        .map_err(|error| error.to_string())?;
    execution_context.run_microtasks()
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use blitz_dom::{BaseDocument, DocumentConfig};
    use boa_engine::Source;
    use url::Url;

    use super::{JsState, parse_html_into_document};

    #[test]
    fn window_is_the_global_this() {
        let document = Rc::new(RefCell::new(BaseDocument::new(DocumentConfig::default())));
        let mut js_state = JsState::new(Rc::clone(&document), Url::parse("about:blank").unwrap())
            .unwrap();

        let result = js_state
            .settings
            .execution_context
            .context
            .eval(Source::from_bytes("window === self && window === this"))
            .unwrap();

        assert_eq!(result.as_boolean(), Some(true));
    }

    #[test]
    fn dispatches_programmatic_event_listeners() {
        let document = Rc::new(RefCell::new(BaseDocument::new(DocumentConfig::default())));
        let mut js_state = JsState::new(Rc::clone(&document), Url::parse("about:blank").unwrap())
            .unwrap();

        let result = js_state
            .settings
            .execution_context
            .context
            .eval(Source::from_bytes(
                "let called = 0; window.addEventListener('load', () => { called += 1; }); window.dispatchEvent(new Event('load')); called;",
            ))
            .unwrap();

        assert_eq!(result.as_number(), Some(1.0));
    }

    #[test]
    fn inline_script_updates_document_title() {
        let document = Rc::new(RefCell::new(BaseDocument::new(DocumentConfig::default())));
        let mut js_state = JsState::new(
            Rc::clone(&document),
            Url::parse("https://example.com/").unwrap(),
        )
        .unwrap();

        {
            let mut document_ref = document.borrow_mut();
            parse_html_into_document(
                &mut document_ref,
                "<html><head><title>before</title><script>document.title = 'after';</script></head><body></body></html>",
                &mut js_state.settings.execution_context,
            );
        }
        js_state.settings.execution_context.drain_tasks().unwrap();

        let title = document
            .borrow()
            .find_title_node()
            .map(|node| node.text_content())
            .unwrap();
        assert_eq!(title, "after");
    }
}
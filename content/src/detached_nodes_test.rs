//! JS-level tests for reclaiming detached DOM nodes.

use std::cell::RefCell;
use std::rc::Rc;

use blitz_dom::BaseDocument;
use crossbeam_channel::Receiver as CrossbeamReceiver;
use ipc::{IpcReceiver, channel};
use ipc_messages::content::{
    CanvasId, Command, DocumentId, Event as ContentEvent, EventLoopId, FrameId, NavigableId,
};
use ipc_messages::network::Request as NetworkRequest;
use js_engine::EcmascriptHost;
use serde_json::{Value, json};

use crate::ContentProcess;
use crate::html::EnvironmentSettingsObject;

struct TestProcess {
    process: ContentProcess,
    document_id: DocumentId,
    event_receiver: IpcReceiver<ContentEvent>,
    network_receiver: IpcReceiver<NetworkRequest>,
    command_receiver: IpcReceiver<Command>,
    wasm_receiver: CrossbeamReceiver<()>,
}

impl Drop for TestProcess {
    fn drop(&mut self) {
        while self.event_receiver.try_recv().is_ok() {}
        while self.network_receiver.try_recv().is_ok() {}
        while self.command_receiver.try_recv().is_ok() {}
        while self.wasm_receiver.try_recv().is_ok() {}
    }
}

impl TestProcess {
    fn new() -> Self {
        let (event_sender, event_receiver) =
            channel::<ContentEvent>().expect("create content event channel");
        let (network_sender, network_receiver) =
            channel::<NetworkRequest>().expect("create network channel");
        let (command_sender, command_receiver) =
            channel::<Command>().expect("create command channel");
        let (wasm_sender, wasm_receiver) = crossbeam_channel::unbounded();
        let mut process = ContentProcess::new(
            event_sender,
            wasm_sender,
            EventLoopId::new(),
            network_sender,
            None,
            command_sender,
            None,
            Vec::new(),
        );
        let traversable_id = NavigableId::new();
        let document_id = DocumentId::new();
        process
            .create_empty_document(
                traversable_id,
                document_id,
                Some(FrameId::new()),
                None,
                traversable_id,
                Vec::new(),
            )
            .expect("create empty document");
        Self {
            process,
            document_id,
            event_receiver,
            network_receiver,
            command_receiver,
            wasm_receiver,
        }
    }

    fn settings(&mut self) -> &mut EnvironmentSettingsObject {
        &mut self
            .process
            .documents
            .get_mut(&self.document_id)
            .expect("document")
            .settings
    }

    fn evaluate(&mut self, source: &str) {
        self.settings()
            .evaluate_script(source)
            .expect("evaluate script");
    }

    fn evaluate_json(&mut self, source: &str) -> Value {
        self.settings()
            .evaluate_script_to_json(source)
            .expect("evaluate script to JSON")
    }

    fn collect_garbage(&mut self) {
        self.settings().realm_execution_context.gc();
    }

    fn settle(&mut self) {
        self.process.drain_dom_hook_steps();
        self.process.drain_detached_nodes();
    }

    fn node_count(&self) -> usize {
        self.process
            .documents
            .get(&self.document_id)
            .expect("document")
            .document
            .borrow()
            .tree()
            .len()
    }

    fn document(&self) -> Rc<RefCell<BaseDocument>> {
        Rc::clone(
            &self
                .process
                .documents
                .get(&self.document_id)
                .expect("document")
                .document,
        )
    }
}

fn with_host(test: &mut TestProcess, markup: &str) {
    test.evaluate(&format!(
        "globalThis.host = document.createElement('div');\
         host.innerHTML = '{markup}';\
         document.body.appendChild(host);\
         globalThis.held = host.firstChild;"
    ));
    test.settle();
}

#[test]
fn held_child_survives_text_content_replacement() {
    let mut test = TestProcess::new();
    with_host(&mut test, "<span>one</span><em>two</em>");
    test.evaluate("host.textContent = '';");
    test.settle();

    let state = test.evaluate_json(
        "[held.parentNode === null, held.nodeName, held.textContent, host.childNodes.length]",
    );
    assert_eq!(state, json!([true, "SPAN", "one", 0]));

    test.evaluate("host.appendChild(held);");
    test.settle();
    let reattached = test.evaluate_json("held.parentNode === host");
    assert_eq!(reattached, json!(true));
}

#[test]
fn held_child_survives_inner_html_replacement() {
    let mut test = TestProcess::new();
    with_host(&mut test, "<span>one</span><em>two</em>");
    test.evaluate("host.innerHTML = '<b>new</b>';");
    test.settle();

    let state = test.evaluate_json("[held.parentNode === null, held.nodeName, held.textContent]");
    assert_eq!(state, json!([true, "SPAN", "one"]));
}

#[test]
fn held_child_survives_removal() {
    let mut test = TestProcess::new();
    with_host(&mut test, "<span>one</span><em>two</em>");
    test.evaluate("host.removeChild(held);");
    test.settle();

    let state = test.evaluate_json("[held.parentNode === null, held.nodeName, held.textContent]");
    assert_eq!(state, json!([true, "SPAN", "one"]));
}

#[test]
fn a_reused_node_id_never_aliases_a_held_child() {
    let mut test = TestProcess::new();
    with_host(&mut test, "<span>one</span><em>two</em>");
    test.evaluate("host.textContent = '';");
    test.settle();
    for _ in 0..16 {
        test.evaluate("document.body.appendChild(document.createElement('div'));");
    }
    test.settle();

    let state = test.evaluate_json("[held.nodeName, held.textContent]");
    assert_eq!(state, json!(["SPAN", "one"]));
}

#[test]
fn a_move_within_one_task_keeps_the_node_attached() {
    let mut test = TestProcess::new();
    test.evaluate(
        "globalThis.host = document.createElement('div');\
         document.body.appendChild(host);\
         globalThis.child = document.createElement('span');\
         host.appendChild(child);\
         host.removeChild(child);\
         host.appendChild(child);",
    );
    test.settle();

    let state = test.evaluate_json("child.parentNode === host");
    assert_eq!(state, json!(true));
}

#[cfg(v8_backend)]
#[test]
fn a_wrapper_on_a_descendant_keeps_the_whole_detached_subtree() {
    let mut test = TestProcess::new();
    test.evaluate(
        "globalThis.host = document.createElement('div');\
         host.innerHTML = '<section><article><span>deep</span></article></section>';\
         document.body.appendChild(host);\
         globalThis.deep = host.querySelector('span');",
    );
    test.settle();
    let attached_count = test.node_count();

    test.evaluate("host.removeChild(host.firstChild);");
    test.settle();
    let state = test.evaluate_json(
        "[deep.parentNode.nodeName, deep.parentNode.parentNode.nodeName, deep.textContent]",
    );
    assert_eq!(state, json!(["ARTICLE", "SECTION", "deep"]));
    assert_eq!(test.node_count(), attached_count);

    test.evaluate("delete globalThis.deep;");
    test.collect_garbage();
    test.settle();
    assert!(
        test.node_count() < attached_count,
        "the detached subtree must be dropped once its last wrapper is collected"
    );
}

#[cfg(v8_backend)]
#[test]
fn repeated_detached_subtrees_shrink_back_to_the_document_size() {
    let mut test = TestProcess::new();
    let baseline = test.node_count();
    test.evaluate(
        "globalThis.host = document.createElement('div');\
         document.body.appendChild(host);\
         for (let index = 0; index < 100000; index++) {\
           const child = document.createElement('span');\
           child.textContent = 'x';\
           host.appendChild(child);\
           host.removeChild(child);\
         }",
    );
    test.settle();
    test.collect_garbage();
    test.settle();

    let after = test.node_count();
    assert!(
        after <= baseline + 16,
        "detached subtrees must not accumulate: baseline {baseline}, after {after}"
    );
}

#[cfg(v8_backend)]
#[test]
fn the_drain_clears_node_id_keyed_registries() {
    let mut test = TestProcess::new();
    test.evaluate(
        "globalThis.host = document.createElement('div');\
         host.innerHTML = '<span id=\"image-node\"></span><span id=\"canvas-node\"></span>';\
         document.body.appendChild(host);",
    );
    test.settle();

    let document = test.document();
    let image_node_id = document
        .borrow()
        .query_selector("#image-node")
        .expect("parse image selector")
        .expect("image node");
    let canvas_node_id = document
        .borrow()
        .query_selector("#canvas-node")
        .expect("parse canvas selector")
        .expect("canvas node");
    drop(document);

    test.process
        .image_registry
        .borrow_mut()
        .element_id(test.document_id, image_node_id);
    test.process
        .canvas_registry
        .borrow_mut()
        .insert((test.document_id, canvas_node_id), CanvasId::new());
    let iframe_navigable_id = NavigableId::new();
    {
        let settings = test.settings();
        crate::js::platform_objects::with_global_scope(
            &mut settings.realm_execution_context,
            |global_scope, ec| {
                global_scope.register_iframe_content_navigable(
                    image_node_id,
                    iframe_navigable_id,
                    None,
                    ec,
                )?;
                Ok(())
            },
        )
        .expect("register iframe content navigable");
    }
    test.process
        .documents
        .get_mut(&test.document_id)
        .expect("document")
        .navigable_container_states
        .insert(
            image_node_id,
            crate::NavigableContainerState {
                content_navigable: None,
                content_frame_id: FrameId::new(),
                current_key: String::new(),
                child_document_loaded: false,
            },
        );

    test.evaluate("host.removeChild(host.firstChild); host.removeChild(host.firstChild);");
    test.collect_garbage();
    test.settle();

    assert!(
        test.process
            .image_registry
            .borrow()
            .existing_element_id(test.document_id, image_node_id)
            .is_none(),
        "the image registry must forget the dropped element"
    );
    assert!(
        !test
            .process
            .canvas_registry
            .borrow()
            .contains_key(&(test.document_id, canvas_node_id)),
        "the canvas registry must forget the dropped element"
    );
    {
        let settings = test.settings();
        let navigable = crate::js::platform_objects::with_global_scope(
            &mut settings.realm_execution_context,
            |global_scope, ec| Ok(global_scope.content_navigable_for_iframe(image_node_id, ec)),
        )
        .expect("read iframe content navigable");
        assert!(
            navigable.is_none(),
            "the window proxy cache must forget the dropped iframe node"
        );
    }
    assert!(
        !test
            .process
            .documents
            .get(&test.document_id)
            .expect("document")
            .navigable_container_states
            .contains_key(&image_node_id),
        "the iframe container state must forget the dropped node"
    );

    let baseline = test.node_count();
    test.evaluate(
        "let fresh = document.createElement('span'); fresh.setAttribute('id', 'image-node');\
         document.body.appendChild(fresh);",
    );
    test.settle();
    assert!(test.node_count() > baseline);
    assert!(
        test.process
            .image_registry
            .borrow()
            .existing_element_id(
                test.document_id,
                test.document()
                    .borrow()
                    .query_selector("#image-node")
                    .expect("parse image selector")
                    .expect("reused image node"),
            )
            .is_none(),
        "a new element must not inherit the dropped element's registry entry"
    );
}

#[cfg(v8_backend)]
#[test]
fn repeated_inner_html_assignments_do_not_accumulate_parser_roots() {
    let mut test = TestProcess::new();
    let baseline = test.node_count();
    test.evaluate(
        "globalThis.host = document.createElement('div');\
         document.body.appendChild(host);\
         for (let index = 0; index < 1000; index++) {\
           host.innerHTML = '<span>x</span>';\
         }",
    );
    test.settle();
    test.collect_garbage();
    test.settle();

    let after = test.node_count();
    assert!(
        after <= baseline + 16,
        "repeated innerHTML assignments must not accumulate parser fragment roots: \
         baseline {baseline}, after {after}"
    );
}

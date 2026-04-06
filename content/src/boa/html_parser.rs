use std::{
    borrow::Cow,
    cell::{Cell, Ref, RefCell, RefMut},
};

use blitz_dom::{BaseDocument, DocumentMutator, HtmlParserProvider, Node};
use blitz_dom::node::Attribute;
use html5ever::{
    ParseOpts, QualName,
    tendril::{StrTendril, TendrilSink},
    tokenizer::TokenizerOpts,
    tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeBuilderOpts, TreeSink},
};

use super::execution_context::JsExecutionContext;

fn html5ever_to_blitz_attr(attr: html5ever::Attribute) -> Attribute {
    Attribute {
        name: attr.name,
        value: attr.value.to_string(),
    }
}

#[derive(Copy, Clone, Default)]
/// <https://html.spec.whatwg.org/#html-parser>
pub struct JsHtmlParserProvider;

impl HtmlParserProvider for JsHtmlParserProvider {
    fn parse_inner_html<'m2, 'doc2>(
        &self,
        mutr: &'m2 mut DocumentMutator<'doc2>,
        element_id: usize,
        html: &str,
    ) {
        JsTreeSink::parse_inner_html_into_mutator(mutr, element_id, html);
    }
}

/// <https://html.spec.whatwg.org/#parse-html-from-a-string>
pub struct JsTreeSink<'m, 'doc> {
    document_mutator: RefCell<&'m mut DocumentMutator<'doc>>,
    errors: RefCell<Vec<Cow<'static, str>>>,
    quirks_mode: Cell<QuirksMode>,
}

impl<'m, 'doc> JsTreeSink<'m, 'doc> {
    fn mutr(&self) -> RefMut<'_, &'m mut DocumentMutator<'doc>> {
        self.document_mutator.borrow_mut()
    }

    pub fn new(mutr: &'m mut DocumentMutator<'doc>) -> Self {
        Self {
            document_mutator: RefCell::new(mutr),
            errors: RefCell::new(Vec::new()),
            quirks_mode: Cell::new(QuirksMode::NoQuirks),
        }
    }

    pub fn parse_into_mutator<'a, 'd>(mutr: &'a mut DocumentMutator<'d>, html: &str) {
        let sink = JsTreeSink::new(mutr);
        let opts = ParseOpts {
            tokenizer: TokenizerOpts::default(),
            tree_builder: TreeBuilderOpts {
                exact_errors: false,
                scripting_enabled: true,
                iframe_srcdoc: false,
                drop_doctype: true,
                quirks_mode: QuirksMode::NoQuirks,
            },
        };
        html5ever::parse_document(sink, opts)
            .from_utf8()
            .read_from(&mut html.as_bytes())
            .unwrap();
    }

    pub fn parse_inner_html_into_mutator<'a, 'd>(
        mutr: &'a mut DocumentMutator<'d>,
        element_id: usize,
        html: &str,
    ) {
        let sink = JsTreeSink::new(mutr);
        let opts = ParseOpts {
            tokenizer: TokenizerOpts::default(),
            tree_builder: TreeBuilderOpts {
                exact_errors: false,
                scripting_enabled: false,
                iframe_srcdoc: false,
                drop_doctype: true,
                quirks_mode: QuirksMode::NoQuirks,
            },
        };
        html5ever::driver::parse_fragment_for_element(sink, opts, element_id, false, None)
            .from_utf8()
            .read_from(&mut html.as_bytes())
            .unwrap();

        let fragment_root_id = mutr.last_child_id(0).unwrap();
        let child_ids = mutr.child_ids(fragment_root_id);
        mutr.append_children(element_id, &child_ids);
        mutr.remove_node(fragment_root_id);
    }
}

impl<'m, 'doc> TreeSink for JsTreeSink<'m, 'doc> {
    type Output = ();
    type Handle = usize;
    type ElemName<'a>
        = Ref<'a, QualName>
    where
        Self: 'a;

    fn finish(self) -> Self::Output {}

    fn parse_error(&self, msg: Cow<'static, str>) {
        self.errors.borrow_mut().push(msg);
    }

    fn get_document(&self) -> Self::Handle {
        0
    }

    fn elem_name<'a>(&'a self, target: &'a Self::Handle) -> Self::ElemName<'a> {
        Ref::map(self.document_mutator.borrow(), |docm| {
            docm.element_name(*target)
                .expect("TreeSink::elem_name requires an element node")
        })
    }

    fn create_element(
        &self,
        name: QualName,
        attrs: Vec<html5ever::Attribute>,
        _flags: ElementFlags,
    ) -> Self::Handle {
        let attrs = attrs.into_iter().map(html5ever_to_blitz_attr).collect();
        self.mutr().create_element(name, attrs)
    }

    fn create_comment(&self, _text: StrTendril) -> Self::Handle {
        self.mutr().create_comment_node()
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> Self::Handle {
        self.mutr().create_comment_node()
    }

    fn append(&self, parent_id: &Self::Handle, child: NodeOrText<Self::Handle>) {
        match child {
            NodeOrText::AppendNode(id) => self.mutr().append_children(*parent_id, &[id]),
            NodeOrText::AppendText(text) => {
                let last_child_id = self.mutr().last_child_id(*parent_id);
                let appended = if let Some(last_child_id) = last_child_id {
                    self.mutr().append_text_to_node(last_child_id, &text).is_ok()
                } else {
                    false
                };
                if !appended {
                    let child_id = self.mutr().create_text_node(&text);
                    self.mutr().append_children(*parent_id, &[child_id]);
                }
            }
        }
    }

    fn append_before_sibling(&self, sibling_id: &Self::Handle, child: NodeOrText<Self::Handle>) {
        match child {
            NodeOrText::AppendNode(id) => self.mutr().insert_nodes_before(*sibling_id, &[id]),
            NodeOrText::AppendText(text) => {
                let previous_sibling_id = self.mutr().previous_sibling_id(*sibling_id);
                let appended = if let Some(previous_sibling_id) = previous_sibling_id {
                    self.mutr().append_text_to_node(previous_sibling_id, &text).is_ok()
                } else {
                    false
                };
                if !appended {
                    let child_id = self.mutr().create_text_node(&text);
                    self.mutr().insert_nodes_before(*sibling_id, &[child_id]);
                }
            }
        }
    }

    fn append_based_on_parent_node(
        &self,
        element: &Self::Handle,
        prev_element: &Self::Handle,
        child: NodeOrText<Self::Handle>,
    ) {
        if self.mutr().node_has_parent(*element) {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(
        &self,
        _name: StrTendril,
        _public_id: StrTendril,
        _system_id: StrTendril,
    ) {
    }

    fn get_template_contents(&self, target: &Self::Handle) -> Self::Handle {
        *target
    }

    fn same_node(&self, x: &Self::Handle, y: &Self::Handle) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, mode: QuirksMode) {
        self.quirks_mode.set(mode);
    }

    fn add_attrs_if_missing(&self, target: &Self::Handle, attrs: Vec<html5ever::Attribute>) {
        let attrs = attrs.into_iter().map(html5ever_to_blitz_attr).collect();
        self.mutr().add_attrs_if_missing(*target, attrs);
    }

    fn remove_from_parent(&self, target: &Self::Handle) {
        self.mutr().remove_node(*target);
    }

    fn reparent_children(&self, old_parent_id: &Self::Handle, new_parent_id: &Self::Handle) {
        self.mutr().reparent_children(*old_parent_id, *new_parent_id);
    }

    fn clone_subtree(&self, target: &Self::Handle) -> Self::Handle {
        self.mutr().deep_clone_node(*target)
    }
}

pub fn parse_html_into_document(
    document: &mut BaseDocument,
    html: &str,
    execution_context: &mut JsExecutionContext,
) {
    {
        let mut mutator = document.mutate();
        JsTreeSink::parse_into_mutator(&mut mutator, html);
    }

    let mut inline_scripts = Vec::new();
    document.visit(|node_id, node: &Node| {
        if !node
            .data
            .is_element_with_tag_name(&blitz_dom::local_name!("script"))
        {
            return;
        }
        inline_scripts.push(node_id);
    });

    for node_id in inline_scripts {
        let Some(node) = document.get_node(node_id) else {
            continue;
        };
        let Some(element) = node.element_data() else {
            continue;
        };
        if element.attr(blitz_dom::local_name!("type")) == Some("module")
            || element.attr(blitz_dom::local_name!("async")).is_some()
            || element.attr(blitz_dom::local_name!("defer")).is_some()
        {
            continue;
        }
        if let Some(src) = element.attr(blitz_dom::local_name!("src")) {
            let src = src.to_owned();
            execution_context.enqueue_task(move |_| {
                eprintln!("external script fetch is not implemented yet: {src}");
                Ok(())
            });
            continue;
        }
        let source = node.text_content();
        if source.trim().is_empty() {
            continue;
        }
        execution_context.enqueue_task(move |execution_context| {
            execution_context.evaluate_script(&source)
        });
    }
}
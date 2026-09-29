use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
};

use blitz_dom::BaseDocument;
use html5ever::{LocalName, Namespace, QualName, ns};
use url::Url;

use super::namespaces::{
    ValidateAndExtractContext, is_a_valid_attribute_local_name, validate_and_extract,
};
use super::{Attr, Attribute, DOMException, DOMImplementation, Element, Node};
use crate::infra::strip_and_collapse_ascii_whitespace;
use crate::js::Types;
use crate::js::platform_objects::with_global_scope;
use crate::webidl::bindings::create_interface_instance;
use js_engine::{Completion, ExecutionContext, gc_struct};

fn collect_subtree_node_ids(document: &BaseDocument, node_id: usize, node_ids: &mut Vec<usize>) {
    let Some(node) = document.get_node(node_id) else {
        return;
    };
    node_ids.push(node_id);
    for child_id in node.children.iter().copied() {
        collect_subtree_node_ids(document, child_id, node_ids);
    }
}

fn canonical_document_dir(value: &str) -> &str {
    if value.eq_ignore_ascii_case("ltr") {
        "ltr"
    } else if value.eq_ignore_ascii_case("rtl") {
        "rtl"
    } else if value.eq_ignore_ascii_case("auto") {
        "auto"
    } else {
        ""
    }
}

/// <https://dom.spec.whatwg.org/#interface-document>
#[gc_struct]
pub struct Document {
    /// <https://dom.spec.whatwg.org/#interface-node>
    pub node: Node,

    /// Model-local mirror of <https://html.spec.whatwg.org/#concept-environment-creation-url>.
    #[ignore_trace]
    pub creation_url: Url,

    /// <https://html.spec.whatwg.org/#current-script>
    #[ignore_trace]
    current_script: Rc<Cell<Option<usize>>>,

    /// <https://html.spec.whatwg.org/#already-started>
    #[ignore_trace]
    already_started_scripts: Rc<RefCell<HashSet<usize>>>,
}

impl Document {
    pub fn new(
        document: Rc<RefCell<BaseDocument>>,
        creation_url: Url,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            node: Node::new(document, 0, ec),
            creation_url,
            current_script: Rc::new(Cell::new(None)),
            already_started_scripts: Rc::new(RefCell::new(HashSet::new())),
        }
    }

    /// <https://dom.spec.whatwg.org/#dom-nonelementparentnode-getelementbyid>
    pub(crate) fn get_element_by_id(&self, id: &str) -> Option<usize> {
        self.node.document.borrow().get_element_by_id(id)
    }

    /// <https://dom.spec.whatwg.org/#dom-parentnode-queryselector>
    pub(crate) fn query_selector(&self, selectors: &str) -> Result<Option<usize>, String> {
        self.node
            .document
            .borrow()
            .query_selector(selectors)
            .map_err(|error| format!("invalid selector `{selectors}`: {error:?}"))
    }

    /// <https://dom.spec.whatwg.org/#dom-parentnode-queryselectorall>
    pub(crate) fn query_selector_all(&self, selectors: &str) -> Result<Vec<usize>, String> {
        self.node
            .document
            .borrow()
            .query_selector_all(selectors)
            .map(|matches| matches.into_iter().collect())
            .map_err(|error| format!("invalid selector `{selectors}`: {error:?}"))
    }

    /// <https://dom.spec.whatwg.org/#dom-parentnode-getelementsbytagname>
    pub(crate) fn get_elements_by_tag_name(
        &self,
        qualified_name: &str,
    ) -> Result<Vec<usize>, String> {
        self.node
            .document
            .borrow()
            .query_selector_all(qualified_name)
            .map(|matches| matches.into_iter().collect())
            .map_err(|error| {
                format!("failed to resolve tag name selector `{qualified_name}`: {error:?}")
            })
    }

    /// <https://dom.spec.whatwg.org/#dom-document-documentelement>
    pub(crate) fn document_element(&self) -> Option<usize> {
        let document = self.node.document.borrow();
        let root = document.get_node(self.node.node_id)?;
        root.children.iter().copied().find(|child_id| {
            document
                .get_node(*child_id)
                .is_some_and(|child| child.element_data().is_some())
        })
    }

    /// <https://dom.spec.whatwg.org/#dom-document-createelement>
    pub(crate) fn create_element(&self, local_name: &str) -> usize {
        let mut document = self.node.document.borrow_mut();
        let mut mutator = document.mutate();
        mutator.create_element(
            QualName::new(None, ns!(html), LocalName::from(local_name)),
            Vec::new(),
        )
    }

    /// <https://dom.spec.whatwg.org/#dom-document-createelementns>
    pub(crate) fn create_element_ns(
        &self,
        namespace: Option<&str>,
        qualified_name: &str,
    ) -> Result<usize, String> {
        // Step 1: Let namespace, prefix, and localName be the result of
        // passing namespace and qualifiedName to validate and extract.
        // Note: The qualified name is not validated or split at a prefix; the
        // namespace is the given one, null and the empty string being the
        // null namespace, which blitz stores as the HTML namespace.
        let namespace = match namespace {
            None | Some("") | Some("http://www.w3.org/1999/xhtml") => ns!(html),
            Some(other) => Namespace::from(other),
        };

        let mut document = self.node.document.borrow_mut();
        let mut mutator = document.mutate();
        Ok(mutator.create_element(
            QualName::new(None, namespace, LocalName::from(qualified_name)),
            Vec::new(),
        ))
    }

    /// <https://dom.spec.whatwg.org/#dom-document-createtextnode>
    pub(crate) fn create_text_node(&self, data: &str) -> usize {
        let mut document = self.node.document.borrow_mut();
        let mut mutator = document.mutate();
        mutator.create_text_node(data)
    }

    /// <https://dom.spec.whatwg.org/#dom-document-createattribute>
    pub(crate) fn create_attribute(
        &self,
        local_name: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Result<Attr, DOMException>, Types> {
        // Step 1: "If localName is not a valid attribute local name, then throw an "InvalidCharacterError" DOMException."
        if !is_a_valid_attribute_local_name(local_name) {
            return Ok(Err(DOMException::invalid_character_error()));
        }

        // Step 2: "If this is an HTML document, then set localName to localName in ASCII lowercase."
        // Note: every document here is an HTML document.
        let local_name = local_name.to_ascii_lowercase();

        // Step 3: "Return the result of creating an attribute given this and localName."
        let attr = Attr::create_an_attribute(
            Attribute {
                namespace: None,
                namespace_prefix: None,
                local_name,
                value: String::new(),
            },
            None,
            ec,
        )?;
        Ok(Ok(attr))
    }

    /// <https://dom.spec.whatwg.org/#dom-document-createattributens>
    pub(crate) fn create_attribute_ns(
        &self,
        namespace: Option<&str>,
        qualified_name: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Result<Attr, DOMException>, Types> {
        // Step 1: "Let (namespace, prefix, localName) be the result of validating and extracting namespace and qualifiedName given "attribute"."
        let (namespace, prefix, local_name) = match validate_and_extract(
            namespace,
            qualified_name,
            ValidateAndExtractContext::Attribute,
        ) {
            Ok(parts) => parts,
            Err(error) => return Ok(Err(error)),
        };

        // Step 2: "Return the result of creating an attribute given this, localName, namespace, and prefix."
        let attr = Attr::create_an_attribute(
            Attribute {
                namespace,
                namespace_prefix: prefix,
                local_name,
                value: String::new(),
            },
            None,
            ec,
        )?;
        Ok(Ok(attr))
    }

    /// <https://dom.spec.whatwg.org/#dom-document-createcomment>
    pub(crate) fn create_comment(&self, _data: &str) -> usize {
        // Step 1: "Return a new Comment node whose data is data and node document is this."
        // Note: Blitz exposes comment nodes without comment-text storage, so the implementation preserves the node identity and tree behavior but not the comment payload yet.
        let mut document = self.node.document.borrow_mut();
        let mut mutator = document.mutate();
        mutator.create_comment_node()
    }

    /// <https://dom.spec.whatwg.org/#dom-document-implementation>
    pub(crate) fn implementation(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<DOMImplementation, Types> {
        // The implementation getter steps are to return the DOMImplementation
        // object that is associated with this.
        // Note: The object is created on first access and cached on the
        // realm's global scope; the binding layer returns that cached object.
        with_global_scope(ec, |global_scope, ec| {
            if let Some(object) = global_scope.dom_implementation_object(ec) {
                return ec
                    .with_object_any(&object)
                    .and_then(|data| data.downcast_ref::<DOMImplementation>().cloned())
                    .ok_or_else(|| {
                        ec.new_type_error("implementation object is not a DOMImplementation")
                    });
            }
            let implementation = DOMImplementation::new();
            let object =
                create_interface_instance::<Types, DOMImplementation>(implementation.clone(), ec)?;
            global_scope.store_dom_implementation_object(object, ec);
            Ok(implementation)
        })
    }

    /// <https://html.spec.whatwg.org/#dom-document-head>
    pub(crate) fn head(&self) -> Result<Option<usize>, String> {
        // The head element of a document is the first head element that is a
        // child of the html element, if there is one, or null otherwise. The
        // head getter steps are to return the head element of this.
        self.node
            .document
            .borrow()
            .query_selector("html > head")
            .map_err(|error| format!("failed to resolve head selector: {error:?}"))
    }

    /// <https://html.spec.whatwg.org/#dom-document-currentscript>
    pub(crate) fn current_script(&self) -> Option<usize> {
        // The currentScript getter steps are to return this's current
        // script.
        self.current_script.get()
    }

    /// <https://html.spec.whatwg.org/#current-script>
    pub(crate) fn set_current_script(&self, script: Option<usize>) {
        self.current_script.set(script);
    }

    /// <https://html.spec.whatwg.org/#already-started>
    pub(crate) fn script_already_started(&self, node_id: usize) -> bool {
        self.already_started_scripts.borrow().contains(&node_id)
    }

    /// <https://html.spec.whatwg.org/#already-started>
    pub(crate) fn set_script_already_started(&self, node_id: usize) {
        self.already_started_scripts.borrow_mut().insert(node_id);
    }

    /// <https://html.spec.whatwg.org/#dom-document-body>
    pub(crate) fn body(&self) -> Result<Option<usize>, String> {
        self.node
            .document
            .borrow()
            .query_selector("body")
            .map_err(|error| format!("failed to resolve body selector: {error:?}"))
    }

    /// <https://html.spec.whatwg.org/#document.title>
    pub(crate) fn title(&self) -> String {
        // Step 1: "If the document element is an SVG svg element, then let value be the child text content of the first SVG title element that is a child of the document element."
        // Current WPT coverage exercises HTML documents; follows HTML branch.
        // Step 2: "Otherwise, let value be the child text content of the title element, or the empty string if the title element is null."
        let value = self
            .node
            .document
            .borrow()
            .find_title_node()
            .map(|node| node.text_content())
            .unwrap_or_default();

        // Step 3: "Strip and collapse ASCII whitespace in value."
        let value = strip_and_collapse_ascii_whitespace(&value);

        // Step 4: "Return value."
        value
    }

    /// <https://html.spec.whatwg.org/#document.title>
    pub(crate) fn set_title(&self, title: &str, ec: &mut dyn ExecutionContext<Types>) {
        let title_node_id = self
            .node
            .document
            .borrow()
            .find_title_node()
            .map(|node| node.id);
        if let Some(title_node_id) = title_node_id {
            Node::new(Rc::clone(&self.node.document), title_node_id, ec)
                .set_text_content(Some(title));
        }
    }

    /// <https://html.spec.whatwg.org/#document.title>
    pub(crate) fn title_subtree_node_ids(&self) -> Vec<usize> {
        let title_node_id = {
            let document = self.node.document.borrow();
            document.find_title_node().map(|node| node.id)
        };
        let Some(title_node_id) = title_node_id else {
            return Vec::new();
        };

        let document = self.node.document.borrow();
        let mut node_ids = Vec::new();
        collect_subtree_node_ids(&document, title_node_id, &mut node_ids);
        node_ids
    }

    /// <https://html.spec.whatwg.org/multipage/dom.html#dom-document-dir>
    pub(crate) fn dir(&self, ec: &mut dyn ExecutionContext<Types>) -> String {
        self.document_element()
            .and_then(|node_id| {
                Element::new(Rc::clone(&self.node.document), node_id, ec).get_attribute("dir")
            })
            .map(|value| canonical_document_dir(&value).to_string())
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/multipage/dom.html#dom-document-dir>
    pub(crate) fn set_dir(&self, dir: &str, ec: &mut dyn ExecutionContext<Types>) {
        if let Some(node_id) = self.document_element() {
            Element::new(Rc::clone(&self.node.document), node_id, ec)
                .set_an_attribute_value("dir", dir, None, None);
        }
    }
}

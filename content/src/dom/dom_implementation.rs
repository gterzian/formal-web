use std::{cell::RefCell, rc::Rc};

use blitz_dom::{BaseDocument, DocumentConfig};
use html5ever::{LocalName, QualName, ns};
use js_engine::{ExecutionContext, gc_struct};

use super::Document;
use crate::js::Types;

/// <https://dom.spec.whatwg.org/#domimplementation>
#[gc_struct]
pub(crate) struct DOMImplementation {
    /// <https://dom.spec.whatwg.org/#domimplementation-document>
    pub(crate) document: Document,
}

impl DOMImplementation {
    pub(crate) fn new(document: Document) -> Self {
        Self { document }
    }

    /// <https://dom.spec.whatwg.org/#dom-domimplementation-hasfeature>
    pub(crate) fn has_feature(&self) -> bool {
        // The hasFeature() method steps are to return true.
        true
    }

    /// <https://dom.spec.whatwg.org/#dom-domimplementation-createhtmldocument>
    pub(crate) fn create_html_document(
        &self,
        title: Option<String>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Document {
        // Step 1: "Let doc be a new document that is an HTML document."
        let blitz_document = Rc::new(RefCell::new(BaseDocument::new(DocumentConfig::default())));

        // Step 2: "Set doc’s content type to "text/html"."
        // Note: blitz documents carry no content type; every document here is
        // an HTML document.

        // Step 3: "Append a new doctype, with "html" as its name and with its node document set to doc, to doc."
        // Note: blitz has no doctype node kind, so the doctype is not
        // represented in the tree.

        // Step 4: "Append the result of creating an element given doc, "html", and the HTML namespace, to doc."
        // Step 5: "Append the result of creating an element given doc, "head", and the HTML namespace, to the html element created earlier."
        // Step 6: "If title is given:"
        // Step 6.1: "Append the result of creating an element given doc, "title", and the HTML namespace, to the head element created earlier."
        // Step 6.2: "Append a new Text node, with its data set to title (which could be the empty string) and its node document set to doc, to the title element created earlier."
        // Step 7: "Append the result of creating an element given doc, "body", and the HTML namespace, to the html element created earlier."
        {
            let mut document = blitz_document.borrow_mut();
            let mut mutator = document.mutate();
            let html = mutator.create_element(html_qual_name("html"), Vec::new());
            mutator.append_children(0, &[html]);
            let head = mutator.create_element(html_qual_name("head"), Vec::new());
            mutator.append_children(html, &[head]);
            if let Some(title) = title {
                let title_element = mutator.create_element(html_qual_name("title"), Vec::new());
                mutator.append_children(head, &[title_element]);
                let text = mutator.create_text_node(&title);
                mutator.append_children(title_element, &[text]);
            }
            let body = mutator.create_element(html_qual_name("body"), Vec::new());
            mutator.append_children(html, &[body]);
        }

        // Step 8: "doc’s origin is this’s associated document’s origin."
        // Note: the origin is derived from the creation URL, which the new
        // document copies from the associated document.
        // Step 9: "Return doc."
        Document::new(blitz_document, self.document.creation_url.clone(), ec)
    }
}

fn html_qual_name(local_name: &str) -> QualName {
    QualName::new(None, ns!(html), LocalName::from(local_name))
}

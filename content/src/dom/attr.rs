use std::cell::RefCell;
use std::rc::Rc;

use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::{Completion, ExecutionContext, gc_struct};

use crate::js::Types;
use crate::webidl::bindings::create_interface_instance;

use super::Element;
use super::event::{EventTarget, EventTargetAccess};

/// <https://dom.spec.whatwg.org/#concept-attribute>
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribute {
    /// <https://dom.spec.whatwg.org/#concept-attribute-namespace>
    pub(crate) namespace: Option<String>,
    /// <https://dom.spec.whatwg.org/#concept-attribute-namespace-prefix>
    pub(crate) namespace_prefix: Option<String>,
    /// <https://dom.spec.whatwg.org/#concept-attribute-local-name>
    pub(crate) local_name: String,
    /// <https://dom.spec.whatwg.org/#concept-attribute-value>
    pub(crate) value: String,
}

impl Attribute {
    /// <https://dom.spec.whatwg.org/#concept-attribute-qualified-name>
    pub(crate) fn qualified_name(&self) -> String {
        // "An attribute’s qualified name is its local name if its namespace prefix is null; otherwise its namespace prefix, followed by ":", followed by its local name."
        match &self.namespace_prefix {
            Some(prefix) => format!("{prefix}:{}", self.local_name),
            None => self.local_name.clone(),
        }
    }

    pub(crate) fn has_namespace_and_local_name(
        &self,
        namespace: Option<&str>,
        local_name: &str,
    ) -> bool {
        self.namespace.as_deref() == namespace && self.local_name == local_name
    }
}

/// <https://dom.spec.whatwg.org/#interface-attr>
#[gc_struct]
pub struct Attr {
    /// <https://dom.spec.whatwg.org/#interface-eventtarget>
    pub event_target: EventTarget,

    /// <https://dom.spec.whatwg.org/#concept-attribute>
    // Note: while the attribute has an element, its value lives in that
    // element's attribute storage and this copy is refreshed on every read;
    // the copy is the value once the element is null.
    #[ignore_trace]
    attribute: Rc<RefCell<Attribute>>,

    /// <https://dom.spec.whatwg.org/#concept-attribute-element>
    element: GcCell<Option<Element>>,
}

impl EventTargetAccess for Attr {
    fn get_event_target(&self, _ec: &mut dyn ExecutionContext<Types>) -> EventTarget {
        self.event_target.clone()
    }
}

impl Attr {
    /// <https://dom.spec.whatwg.org/#create-an-attribute>
    // Note: the document is the realm's document, which every node here
    // belongs to, and the attribute's element is given at creation when the
    // attribute is already in an element's attribute list.
    pub(crate) fn create_an_attribute(
        attribute: Attribute,
        element: Option<Element>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Attr, Types> {
        // Step 1: "Let attribute be the result of creating a node that implements Attr, given document."
        // Step 2: "Set attribute’s namespace to namespace, namespace prefix to prefix, local name to localName, and value to value."
        let attr = Attr {
            event_target: EventTarget::new(ec),
            attribute: Rc::new(RefCell::new(attribute)),
            element: gc_cell_new(element, ec),
        };
        let object = create_interface_instance::<Types, Attr>(attr, ec)?;

        // Step 3: "Return attribute."
        ec.with_object_any(&object)
            .and_then(|data| data.downcast_ref::<Attr>().cloned())
            .ok_or_else(|| ec.new_type_error("Attr instance is not an Attr"))
    }

    pub(crate) fn ptr_eq(&self, other: &Attr) -> bool {
        Rc::ptr_eq(&self.attribute, &other.attribute)
    }

    pub(crate) fn is_attribute(&self, attribute: &Attribute) -> bool {
        let own = self.attribute.borrow();
        own.namespace == attribute.namespace && own.local_name == attribute.local_name
    }

    /// <https://dom.spec.whatwg.org/#concept-attribute>
    pub(crate) fn attribute(&self, ec: &mut dyn ExecutionContext<Types>) -> Attribute {
        let mut attribute = self.attribute.borrow().clone();
        if let Some(element) = self.element(ec) {
            match element.get_an_attribute_by_namespace_and_local_name(
                attribute.namespace.as_deref(),
                &attribute.local_name,
            ) {
                Some(stored) => {
                    attribute.value = stored.value;
                    self.attribute.borrow_mut().value = attribute.value.clone();
                }
                None => self.element.set(None, ec),
            }
        }
        attribute
    }

    /// <https://dom.spec.whatwg.org/#concept-attribute-element>
    pub(crate) fn element(&self, ec: &mut dyn ExecutionContext<Types>) -> Option<Element> {
        self.element.borrow(ec).clone()
    }

    /// <https://dom.spec.whatwg.org/#concept-attribute-element>
    pub(crate) fn set_element(
        &self,
        element: Option<Element>,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        if element.is_none() {
            let attribute = self.attribute(ec);
            self.attribute.borrow_mut().value = attribute.value;
        }
        self.element.set(element, ec);
    }

    /// <https://dom.spec.whatwg.org/#set-an-existing-attribute-value>
    pub(crate) fn set_an_existing_attribute_value(
        &self,
        value: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        // Step 1: "If attribute’s element is null, then set attribute’s value to value and return."
        // Step 2: "Let element be attribute’s element."
        let Some(element) = self.element(ec) else {
            self.attribute.borrow_mut().value = value.to_owned();
            return;
        };

        // Step 3: "Let verifiedValue be the result of calling get trusted type compliant attribute value with attribute’s local name, attribute’s namespace, element, and value."
        // Step 4: "If attribute’s element is null, then set attribute’s value to verifiedValue and return."
        // TODO: Trusted Types is not implemented; verifiedValue is value.

        // Step 5: "Change attribute to verifiedValue."
        let attribute = self.attribute(ec);
        element.change_an_attribute(&attribute, value);
    }

    /// <https://dom.spec.whatwg.org/#dom-attr-namespaceuri>
    pub(crate) fn namespace_uri(&self) -> Option<String> {
        // "The namespaceURI getter steps are to return this’s namespace."
        self.attribute.borrow().namespace.clone()
    }

    /// <https://dom.spec.whatwg.org/#dom-attr-prefix>
    pub(crate) fn prefix(&self) -> Option<String> {
        // "The prefix getter steps are to return this’s namespace prefix."
        self.attribute.borrow().namespace_prefix.clone()
    }

    /// <https://dom.spec.whatwg.org/#dom-attr-localname>
    pub(crate) fn local_name(&self) -> String {
        // "The localName getter steps are to return this’s local name."
        self.attribute.borrow().local_name.clone()
    }

    /// <https://dom.spec.whatwg.org/#dom-attr-name>
    pub(crate) fn name(&self) -> String {
        // "The name getter steps are to return this’s qualified name."
        self.attribute.borrow().qualified_name()
    }

    /// <https://dom.spec.whatwg.org/#dom-attr-value>
    pub(crate) fn value(&self, ec: &mut dyn ExecutionContext<Types>) -> String {
        // "The value getter steps are to return this’s value."
        self.attribute(ec).value
    }

    /// <https://dom.spec.whatwg.org/#dom-attr-value>
    pub(crate) fn set_value(&self, value: &str, ec: &mut dyn ExecutionContext<Types>) {
        // "The value setter steps are to set an existing attribute value with this and the given value."
        self.set_an_existing_attribute_value(value, ec);
    }

    /// <https://dom.spec.whatwg.org/#dom-attr-ownerelement>
    pub(crate) fn owner_element(&self, ec: &mut dyn ExecutionContext<Types>) -> Option<Element> {
        // "The ownerElement getter steps are to return this’s element."
        self.attribute(ec);
        self.element(ec)
    }

    /// <https://dom.spec.whatwg.org/#dom-attr-specified>
    pub(crate) fn specified(&self) -> bool {
        // "The specified getter steps are to return true."
        true
    }

    /// <https://dom.spec.whatwg.org/#dom-node-nodetype>
    pub(crate) fn node_type(&self) -> u16 {
        // "Attr node: ATTRIBUTE_NODE (2)."
        2
    }

    /// <https://dom.spec.whatwg.org/#dom-node-nodename>
    pub(crate) fn node_name(&self) -> String {
        // "Attr: Its qualified name."
        self.attribute.borrow().qualified_name()
    }

    /// <https://dom.spec.whatwg.org/#dom-node-nodevalue>
    pub(crate) fn node_value(&self, ec: &mut dyn ExecutionContext<Types>) -> String {
        // "Attr: this’s value."
        self.attribute(ec).value
    }

    /// <https://dom.spec.whatwg.org/#dom-node-nodevalue>
    pub(crate) fn set_node_value(&self, value: Option<&str>, ec: &mut dyn ExecutionContext<Types>) {
        // Step 1: "If the given value is null, act as if it was the empty string instead."
        let value = value.unwrap_or("");

        // Step 2: "Attr: Set an existing attribute value with this and the given value."
        self.set_an_existing_attribute_value(value, ec);
    }

    /// <https://dom.spec.whatwg.org/#get-text-content>
    pub(crate) fn text_content(&self, ec: &mut dyn ExecutionContext<Types>) -> String {
        // "Attr: node’s value."
        self.attribute(ec).value
    }

    /// <https://dom.spec.whatwg.org/#set-text-content>
    pub(crate) fn set_text_content(
        &self,
        value: Option<&str>,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        // "The textContent setter steps are to, if the given value is null, act as if it was the empty string instead, and then run set text content with this and the given value."
        let value = value.unwrap_or("");

        // "Attr: Set an existing attribute value with node and value."
        self.set_an_existing_attribute_value(value, ec);
    }
}

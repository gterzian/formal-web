use js_engine::{Completion, ExecutionContext, JsTypes, gc_struct};

use crate::js::Types;
use crate::webidl::create_legacy_platform_object;

use super::{Attr, DOMException, Element};

type JsObject = <Types as JsTypes>::JsObject;

/// <https://dom.spec.whatwg.org/#interface-namednodemap>
#[gc_struct]
pub struct NamedNodeMap {
    /// <https://dom.spec.whatwg.org/#concept-namednodemap-element>
    pub(crate) element: Element,
    pub(crate) reflector: Option<JsObject>,
}

impl NamedNodeMap {
    /// <https://dom.spec.whatwg.org/#interface-namednodemap>
    pub(crate) fn new(
        element: Element,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<NamedNodeMap, Types> {
        let map = NamedNodeMap {
            element,
            reflector: None,
        };
        let object = create_legacy_platform_object::<NamedNodeMap>(map, ec)?;
        ec.with_object_any(&object)
            .and_then(|data| data.downcast_ref::<NamedNodeMap>().cloned())
            .ok_or_else(|| ec.new_type_error("NamedNodeMap instance is not a NamedNodeMap"))
    }

    /// <https://dom.spec.whatwg.org/#concept-namednodemap-attribute>
    pub(crate) fn attribute_list(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Vec<Attr>, Types> {
        // "A NamedNodeMap object’s attribute list is its element’s attribute list."
        self.element.attribute_list(ec)
    }

    /// <https://dom.spec.whatwg.org/#interface-namednodemap>
    pub(crate) fn supported_property_indices(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<u32, Types> {
        // "A NamedNodeMap object’s supported property indices are the numbers in the range zero to its attribute list’s size − 1, unless the attribute list is empty, in which case there are no supported property indices."
        Ok(self.attribute_list(ec)?.len() as u32)
    }

    /// <https://dom.spec.whatwg.org/#interface-namednodemap>
    pub(crate) fn supported_property_names(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Vec<String>, Types> {
        // Step 1: "Let names be the qualified names of the attributes in this NamedNodeMap object’s attribute list, with duplicates omitted, in order."
        let mut names: Vec<String> = Vec::new();
        for attr in self.attribute_list(ec)? {
            let name = attr.name();
            if !names.contains(&name) {
                names.push(name);
            }
        }

        // Step 2: "If this NamedNodeMap object’s element is in the HTML namespace and its node document is an HTML document, then for each name of names:"
        if self.element.is_in_the_html_namespace_in_an_html_document() {
            names.retain(|name| {
                // Step 2.1: "Let lowercaseName be name, in ASCII lowercase."
                let lowercase_name = name.to_ascii_lowercase();

                // Step 2.2: "If lowercaseName is not equal to name, then remove name from names."
                lowercase_name == *name
            });
        }

        // Step 3: "Return names."
        Ok(names)
    }

    /// <https://dom.spec.whatwg.org/#dom-namednodemap-length>
    pub(crate) fn length(&self, ec: &mut dyn ExecutionContext<Types>) -> Completion<u32, Types> {
        // "The length getter steps are to return this’s attribute list’s size."
        Ok(self.attribute_list(ec)?.len() as u32)
    }

    /// <https://dom.spec.whatwg.org/#dom-namednodemap-item>
    pub(crate) fn item(
        &self,
        index: u32,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Option<Attr>, Types> {
        let attribute_list = self.attribute_list(ec)?;
        // Step 1: "If index is equal to or greater than this’s attribute list’s size, then return null."
        // Step 2: "Otherwise, return this’s attribute list[index]."
        Ok(attribute_list.get(index as usize).cloned())
    }

    /// <https://dom.spec.whatwg.org/#dom-namednodemap-getnameditem>
    pub(crate) fn get_named_item(
        &self,
        qualified_name: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Option<Attr>, Types> {
        // "The getNamedItem(qualifiedName) method steps are to return the result of getting an attribute given qualifiedName and element."
        match self.element.get_an_attribute_by_name(qualified_name) {
            Some(attribute) => self.element.attribute_node(&attribute, ec),
            None => Ok(None),
        }
    }

    /// <https://dom.spec.whatwg.org/#dom-namednodemap-getnameditemns>
    pub(crate) fn get_named_item_ns(
        &self,
        namespace: Option<&str>,
        local_name: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Option<Attr>, Types> {
        // "The getNamedItemNS(namespace, localName) method steps are to return the result of getting an attribute given namespace, localName, and element."
        match self
            .element
            .get_an_attribute_by_namespace_and_local_name(namespace, local_name)
        {
            Some(attribute) => self.element.attribute_node(&attribute, ec),
            None => Ok(None),
        }
    }

    /// <https://dom.spec.whatwg.org/#dom-namednodemap-setnameditem>
    pub(crate) fn set_named_item(
        &self,
        attr: &Attr,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Result<Option<Attr>, DOMException>, Types> {
        // "The setNamedItem(attr) and setNamedItemNS(attr) method steps are to return the result of setting an attribute given attr and element."
        self.element.set_an_attribute(attr, ec)
    }

    /// <https://dom.spec.whatwg.org/#dom-namednodemap-removenameditem>
    pub(crate) fn remove_named_item(
        &self,
        qualified_name: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Result<Attr, DOMException>, Types> {
        // Step 1: "Let attr be the result of removing an attribute given qualifiedName and element."
        let attr = self.element.remove_attribute_returning_node(
            self.element.get_an_attribute_by_name(qualified_name),
            ec,
        )?;

        // Step 2: "If attr is null, then throw a "NotFoundError" DOMException."
        let Some(attr) = attr else {
            return Ok(Err(DOMException::not_found_error()));
        };

        // Step 3: "Return attr."
        Ok(Ok(attr))
    }

    /// <https://dom.spec.whatwg.org/#dom-namednodemap-removenameditemns>
    pub(crate) fn remove_named_item_ns(
        &self,
        namespace: Option<&str>,
        local_name: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Result<Attr, DOMException>, Types> {
        // Step 1: "Let attr be the result of removing an attribute given namespace, localName, and element."
        let attr = self.element.remove_attribute_returning_node(
            self.element
                .get_an_attribute_by_namespace_and_local_name(namespace, local_name),
            ec,
        )?;

        // Step 2: "If attr is null, then throw a "NotFoundError" DOMException."
        let Some(attr) = attr else {
            return Ok(Err(DOMException::not_found_error()));
        };

        // Step 3: "Return attr."
        Ok(Ok(attr))
    }
}

use std::collections::BTreeMap;
use std::rc::Rc;

use js_engine::{Completion, ExecutionContext, JsTypes, gc_struct};
use log::error;
use style::context::QuirksMode;
use style::properties::{
    Importance, PropertyDeclarationBlock, PropertyId, SourcePropertyDeclaration,
    SourcePropertyDeclarationUpdate, parse_one_declaration_into, parse_style_attribute,
};
use style::servo_arc::Arc as ServoArc;
use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
use style_traits::ParsingMode;
use url::Url;

use crate::dom::{DOMException, Element};
use crate::js::Types;
use crate::webidl::create_legacy_platform_object;

type JsObject = <Types as JsTypes>::JsObject;

/// <https://drafts.csswg.org/cssom/#cssstyledeclaration>
#[gc_struct]
pub struct CSSStyleDeclaration {
    /// <https://drafts.csswg.org/cssom/#cssstyledeclaration-owner-node>
    pub(crate) owner_node: Option<Element>,

    /// <https://drafts.csswg.org/cssom/#cssstyledeclaration-computed-flag>
    #[ignore_trace]
    computed_flag: bool,

    /// <https://drafts.csswg.org/cssom/#cssstyledeclaration-readonly-flag>
    #[ignore_trace]
    readonly_flag: bool,

    /// <https://drafts.csswg.org/cssom/#cssstyledeclaration-declarations>
    // Note: a block with an owner node reads its declarations from the owner
    // node's style attribute on each access and writes them back through
    // "update style attribute for", so the attribute is the only storage;
    // a computed block holds its resolved values here, in property order.
    #[ignore_trace]
    computed_declarations: Rc<BTreeMap<String, String>>,

    pub(crate) reflector: Option<JsObject>,
}

impl CSSStyleDeclaration {
    /// <https://drafts.csswg.org/cssom/#cssstyledeclaration>
    pub(crate) fn new(
        owner_node: Option<Element>,
        computed_declarations: Option<BTreeMap<String, String>>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<CSSStyleDeclaration, Types> {
        let computed_flag = computed_declarations.is_some();
        let declaration_block = CSSStyleDeclaration {
            owner_node,
            computed_flag,
            readonly_flag: computed_flag,
            computed_declarations: Rc::new(computed_declarations.unwrap_or_default()),
            reflector: None,
        };
        let object = create_legacy_platform_object::<CSSStyleDeclaration>(declaration_block, ec)?;
        ec.with_object_any(&object)
            .and_then(|data| data.downcast_ref::<CSSStyleDeclaration>().cloned())
            .ok_or_else(|| {
                ec.new_type_error("CSSStyleDeclaration instance is not a CSSStyleDeclaration")
            })
    }

    /// <https://drafts.csswg.org/cssom/#cssstyledeclaration-declarations>
    fn declarations(&self) -> PropertyDeclarationBlock {
        match &self.owner_node {
            Some(owner_node) => parse_a_css_declaration_block(
                &owner_node.get_attribute("style").unwrap_or_default(),
            ),
            None => PropertyDeclarationBlock::new(),
        }
    }

    /// <https://drafts.csswg.org/cssom/#update-style-attribute-for>
    fn update_style_attribute_for(&self, declarations: &PropertyDeclarationBlock) {
        // Step 1: "Assert: declaration block’s computed flag is unset."
        debug_assert!(!self.computed_flag);

        // Step 2: "Let owner node be declaration block’s owner node."
        // Step 3: "If owner node is null, then return."
        let Some(owner_node) = &self.owner_node else {
            return;
        };

        // Step 4: "Set declaration block’s updating flag."
        // Step 5: "Set an attribute value for owner node using "style" and the result of serializing declaration block."
        // Step 6: "Unset declaration block’s updating flag."
        // Note: the updating flag guards the reparse of a changed style
        // attribute; the declarations here are reparsed from the attribute on
        // each access instead, so the flag has no reader.
        owner_node.set_an_attribute_value(
            "style",
            &serialize_a_css_declaration_block(declarations),
            None,
            None,
        );
    }

    /// <https://drafts.csswg.org/cssom/#dom-cssstyledeclaration-length>
    pub(crate) fn length(&self) -> u32 {
        // "The length attribute must return the number of CSS declarations in the declarations."
        if self.computed_flag {
            self.computed_declarations.len() as u32
        } else {
            self.declarations().len() as u32
        }
    }

    /// <https://drafts.csswg.org/cssom/#dom-cssstyledeclaration-item>
    pub(crate) fn item(&self, index: u32) -> String {
        // "The item(index) method must return the property name of the CSS declaration at position index in the declarations, or the empty string if there is no such CSS declaration."
        if self.computed_flag {
            return self
                .computed_declarations
                .keys()
                .nth(index as usize)
                .cloned()
                .unwrap_or_default();
        }
        self.declarations()
            .declarations()
            .get(index as usize)
            .map(|declaration| declaration.id().name().into_owned())
            .unwrap_or_default()
    }

    /// <https://drafts.csswg.org/cssom/#dom-cssstyledeclaration-getpropertyvalue>
    pub(crate) fn get_property_value(&self, property: &str) -> String {
        // Step 1: "If property is not a custom property, follow these substeps:"
        // Step 1.1: "Let property be property converted to ASCII lowercase."
        let property = lowercase_unless_custom(property);
        if self.computed_flag {
            return self
                .computed_declarations
                .get(&property)
                .cloned()
                .unwrap_or_default();
        }

        // Step 1.2: "If property is a shorthand property, then follow these substeps:"
        // Step 1.2.1: "Let list be a new empty array."
        // Step 1.2.2: "Let prefix be the empty string."
        // Step 1.2.3: "For each longhand property longhand that property maps to, in canonical order, follow these substeps:"
        // Step 1.2.4: "If list is not empty, then follow these substeps:"
        // Step 1.2.5: "Return the empty string."
        // Step 2: "If property is a case-sensitive match for a property name of a CSS declaration in the declarations, then return the result of invoking serialize a CSS value of that declaration."
        // Step 3: "Return the empty string."
        // Note: steps 1.2 to 3 run in stylo's property_value_to_css, over the
        // property id that its property database resolved.
        let Ok(property_id) = PropertyId::parse_enabled_for_all_content(&property) else {
            return String::new();
        };
        let mut value = String::new();
        if let Err(fmt_error) = self
            .declarations()
            .property_value_to_css(&property_id, &mut value)
        {
            error!("[cssom] failed to serialize the value of {property}: {fmt_error}");
        }
        value
    }

    /// <https://drafts.csswg.org/cssom/#dom-cssstyledeclaration-getpropertypriority>
    pub(crate) fn get_property_priority(&self, property: &str) -> String {
        // Step 1: "If property is not a custom property, follow these substeps:"
        // Step 1.1: "Let property be property converted to ASCII lowercase."
        let property = lowercase_unless_custom(property);
        if self.computed_flag {
            return String::new();
        }

        // Step 1.2: "If property is a shorthand property, follow these substeps:"
        // Step 1.2.1: "Let list be a new array."
        // Step 1.2.2: "For each longhand property longhand that property maps to, append the result of invoking getPropertyPriority() with longhand as argument to list."
        // Step 1.2.3: "If all items in list are the string "important", then return the string "important"."
        // Step 2: "If property is a case-sensitive match for a property name of a CSS declaration in the declarations that has the important flag set, return the string "important"."
        // Step 3: "Return the empty string."
        // Note: steps 1.2 to 3 run in stylo's property_priority.
        let Ok(property_id) = PropertyId::parse_enabled_for_all_content(&property) else {
            return String::new();
        };
        match self.declarations().property_priority(&property_id) {
            Importance::Important => String::from("important"),
            Importance::Normal => String::new(),
        }
    }

    /// <https://drafts.csswg.org/cssom/#dom-cssstyledeclaration-setproperty>
    pub(crate) fn set_property(
        &self,
        property: &str,
        value: &str,
        priority: &str,
    ) -> Result<(), DOMException> {
        // Step 1: "If the computed flag is set, then throw a NoModificationAllowedError exception."
        if self.computed_flag {
            return Err(DOMException::no_modification_allowed_error());
        }

        // Step 2: "If property is not a custom property, follow these substeps:"
        // Step 2.1: "Let property be property converted to ASCII lowercase."
        let property = lowercase_unless_custom(property);

        // Step 2.2: "If property is not a case-sensitive match for a supported CSS property, then return."
        let Ok(property_id) = PropertyId::parse_enabled_for_all_content(&property) else {
            return Ok(());
        };

        // Step 3: "If value is the empty string, invoke removeProperty() with property as argument and return."
        if value.is_empty() {
            self.remove_property(&property)?;
            return Ok(());
        }

        // Step 4: "If priority is not the empty string and is not an ASCII case-insensitive match for the string "important", then return."
        if !priority.is_empty() && !priority.eq_ignore_ascii_case("important") {
            return Ok(());
        }

        // Step 5: "Let component value list be the result of parsing value for property property."
        // Step 6: "If component value list is null, then return."
        let mut component_value_list = SourcePropertyDeclaration::default();
        let url_data = url_extra_data();
        if parse_one_declaration_into(
            &mut component_value_list,
            property_id,
            value,
            Origin::Author,
            &url_data,
            None,
            ParsingMode::DEFAULT,
            QuirksMode::NoQuirks,
            CssRuleType::Style,
        )
        .is_err()
        {
            return Ok(());
        }

        // Step 7: "Let updated be false."
        // Step 8: "If property is a shorthand property, then for each longhand property longhand that property maps to, in canonical order, follow these substeps:"
        // Step 8.1: "Let longhand result be the result of set the CSS declaration longhand with the appropriate value(s) from component value list, with the important flag set if priority is not the empty string, and unset otherwise, and with the list of declarations being the declarations."
        // Step 8.2: "If longhand result is true, let updated be true."
        // Step 9: "Otherwise, let updated be the result of set the CSS declaration property with value component value list, with the important flag set if priority is not the empty string, and unset otherwise, and with the list of declarations being the declarations."
        // Note: the parsed component value list already holds one declaration
        // per longhand a shorthand maps to, so steps 8 and 9 are the same
        // call.
        let importance = if priority.is_empty() {
            Importance::Normal
        } else {
            Importance::Important
        };
        let mut declarations = self.declarations();
        let updated = set_the_css_declaration(&mut declarations, component_value_list, importance);

        // Step 10: "If updated is true, update style attribute for the CSS declaration block."
        if updated {
            self.update_style_attribute_for(&declarations);
        }
        Ok(())
    }

    /// <https://drafts.csswg.org/cssom/#dom-cssstyledeclaration-removeproperty>
    pub(crate) fn remove_property(&self, property: &str) -> Result<String, DOMException> {
        // Step 1: "If the readonly flag is set, then throw a NoModificationAllowedError exception."
        if self.readonly_flag {
            return Err(DOMException::no_modification_allowed_error());
        }

        // Step 2: "If property is not a custom property, let property be property converted to ASCII lowercase."
        let property = lowercase_unless_custom(property);

        // Step 3: "Let value be the return value of invoking getPropertyValue() with property as argument."
        let value = self.get_property_value(&property);

        // Step 4: "Let removed be false."
        let mut removed = false;

        // Step 5: "If property is a shorthand property, for each longhand property longhand that property maps to:"
        // Step 5.1: "If longhand is not a property name of a CSS declaration in the declarations, continue."
        // Step 5.2: "Remove that CSS declaration and let removed be true."
        // Step 6: "Otherwise, if property is a case-sensitive match for a property name of a CSS declaration in the declarations, remove that CSS declaration and let removed be true."
        // Note: stylo's first_declaration_to_remove finds the next declaration
        // of the property or of a longhand it maps to, so steps 5 and 6 are
        // the same loop.
        let mut declarations = self.declarations();
        if let Ok(property_id) = PropertyId::parse_enabled_for_all_content(&property) {
            while let Some(first_declaration) =
                declarations.first_declaration_to_remove(&property_id)
            {
                declarations.remove_property(&property_id, first_declaration);
                removed = true;
            }
        }

        // Step 7: "If removed is true, Update style attribute for the CSS declaration block."
        if removed {
            self.update_style_attribute_for(&declarations);
        }

        // Step 8: "Return value."
        Ok(value)
    }

    /// <https://drafts.csswg.org/cssom/#dom-cssstyledeclaration-csstext>
    pub(crate) fn css_text(&self) -> String {
        // Step 1: "If the computed flag is set, then return the empty string."
        if self.computed_flag {
            return String::new();
        }

        // Step 2: "Return the result of serializing the declarations."
        serialize_a_css_declaration_block(&self.declarations())
    }

    /// <https://drafts.csswg.org/cssom/#dom-cssstyledeclaration-csstext>
    pub(crate) fn set_css_text(&self, value: &str) -> Result<(), DOMException> {
        // Step 1: "If the readonly flag is set, then throw a NoModificationAllowedError exception."
        if self.readonly_flag {
            return Err(DOMException::no_modification_allowed_error());
        }

        // Step 2: "Empty the declarations."
        // Step 3: "Parse the given value and, if the return value is not the empty list, append all the returned CSS declarations to the declarations."
        let declarations = parse_a_css_declaration_block(value);

        // Step 4: "Update style attribute for the CSS declaration block."
        self.update_style_attribute_for(&declarations);
        Ok(())
    }

    /// <https://drafts.csswg.org/cssom/#dom-cssstyledeclaration-camel-cased-attribute>
    pub(crate) fn camel_cased_attribute(&self, attribute: &str, dashed_prefix: bool) -> String {
        // "The camel-cased attribute getter steps are to return the result of invoking getPropertyValue() with the argument being the result of running the IDL attribute to CSS property algorithm for camel-cased attribute."
        self.get_property_value(&idl_attribute_to_css_property(attribute, dashed_prefix))
    }

    /// <https://drafts.csswg.org/cssom/#dom-cssstyledeclaration-camel-cased-attribute>
    pub(crate) fn set_camel_cased_attribute(
        &self,
        attribute: &str,
        dashed_prefix: bool,
        value: &str,
    ) -> Result<(), DOMException> {
        // "The camel-cased attribute setter steps are to invoke setProperty() with the first argument being the result of running the IDL attribute to CSS property algorithm for camel-cased attribute, as second argument the given value, and no third argument."
        self.set_property(
            &idl_attribute_to_css_property(attribute, dashed_prefix),
            value,
            "",
        )
    }

    /// <https://drafts.csswg.org/cssom/#dom-cssstyledeclaration-dashed-attribute>
    pub(crate) fn dashed_attribute(&self, attribute: &str) -> String {
        // "The dashed attribute getter steps are to return the result of invoking getPropertyValue() with the argument being dashed attribute."
        self.get_property_value(attribute)
    }

    /// <https://drafts.csswg.org/cssom/#dom-cssstyledeclaration-dashed-attribute>
    pub(crate) fn set_dashed_attribute(
        &self,
        attribute: &str,
        value: &str,
    ) -> Result<(), DOMException> {
        // "The dashed attribute setter steps are to invoke setProperty() with the first argument being dashed attribute, as second argument the given value, and no third argument."
        self.set_property(attribute, value, "")
    }
}

/// <https://drafts.csswg.org/css-variables/#custom-property>
fn is_a_custom_property(property: &str) -> bool {
    property.starts_with("--")
}

fn lowercase_unless_custom(property: &str) -> String {
    if is_a_custom_property(property) {
        property.to_owned()
    } else {
        property.to_ascii_lowercase()
    }
}

fn url_extra_data() -> UrlExtraData {
    UrlExtraData(ServoArc::new(
        Url::parse("about:blank").expect("about:blank is a valid URL"),
    ))
}

/// <https://drafts.csswg.org/cssom/#parse-a-css-declaration-block>
fn parse_a_css_declaration_block(string: &str) -> PropertyDeclarationBlock {
    // Step 1: "Let declarations be the returned declarations from invoking parse a list of declarations with string."
    // Step 2: "Let parsed declarations be a new empty list."
    // Step 3: "For each item declaration in declarations, follow these substeps:"
    // Step 3.1: "Let parsed declaration be the result of parsing declaration according to the appropriate CSS specifications, dropping parts that are said to be ignored. If the whole declaration is dropped, let parsed declaration be null."
    // Step 3.2: "If parsed declaration is not null, append it to parsed declarations."
    // Step 4: "Return parsed declarations."
    // Note: the four steps run in stylo's style attribute parser.
    parse_style_attribute(
        string,
        &url_extra_data(),
        None,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    )
}

/// <https://drafts.csswg.org/cssom/#serialize-a-css-declaration-block>
fn serialize_a_css_declaration_block(declarations: &PropertyDeclarationBlock) -> String {
    // Note: the algorithm runs in stylo's declaration block serializer.
    let mut serialized = String::new();
    if let Err(fmt_error) = declarations.to_css(&mut serialized) {
        error!("[cssom] failed to serialize a CSS declaration block: {fmt_error}");
    }
    serialized
}

/// <https://drafts.csswg.org/cssom/#set-the-css-declaration>
fn set_the_css_declaration(
    declarations: &mut PropertyDeclarationBlock,
    mut component_value_list: SourcePropertyDeclaration,
    importance: Importance,
) -> bool {
    // Step 1: "If property is a case-sensitive match for a property name of a CSS declaration in declarations, let declaration be that CSS declaration."
    // Step 2: "Otherwise, let declaration be null."
    // Step 3: "Let needsAppend be false."
    // Step 4: "If declaration is null, then set needsAppend to true."
    // Step 5: "Otherwise:"
    // Step 6: "Set declaration’s value to component value list."
    // Step 7: "Set declaration’s important flag."
    // Step 8: "If needsAppend is true, then append declaration to declarations."
    // Step 9: "Return true."
    // Note: stylo's prepare_for_update reports whether the declarations
    // change and update applies the change, so the steps run there and the
    // result is false when the value and flag are already in place.
    let mut updates = SourcePropertyDeclarationUpdate::default();
    if !declarations.prepare_for_update(&component_value_list, importance, &mut updates) {
        return false;
    }
    declarations.update(component_value_list.drain(), importance, &mut updates);
    true
}

/// <https://drafts.csswg.org/cssom/#idl-attribute-to-css-property>
pub(crate) fn idl_attribute_to_css_property(attribute: &str, dashed_prefix: bool) -> String {
    // Step 1: "Let output be the empty string."
    let mut output = String::with_capacity(attribute.len() + 1);

    // Step 2: "If the dashed prefix flag is set, append "-" (U+002D) to output."
    if dashed_prefix {
        output.push('-');
    }

    // Step 3: "For each character c in attribute:"
    for character in attribute.chars() {
        // Step 3.1: "If c is in the range U+0041 to U+005A (ASCII uppercase), append "-" (U+002D) followed by c converted to ASCII lowercase to output."
        if character.is_ascii_uppercase() {
            output.push('-');
            output.push(character.to_ascii_lowercase());
        // Step 3.2: "Otherwise, append c to output."
        } else {
            output.push(character);
        }
    }

    // Step 4: "Return output."
    output
}

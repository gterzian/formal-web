//! The `CSS` namespace.
//!
//! https://drafts.csswg.org/css-conditional-3/#the-css-namespace

use std::borrow::Cow;

use style::context::QuirksMode;
use style::parser::ParserContext;
use style::servo_arc::Arc as ServoArc;
use style::stylesheets::supports_rule::{
    Declaration, SupportsCondition, parse_condition_or_declaration,
};
use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
use style::values::Parser;
use style_traits::ParsingMode;

/// The `CSS` namespace object.
///
/// Holds useful CSS-related functions that do not belong elsewhere.
///
/// https://drafts.csswg.org/css-conditional-3/#the-css-namespace
pub(crate) struct CSS;

impl CSS {
    /// <https://drafts.csswg.org/css-conditional-3/#dom-css-supports-conditiontext-conditiontext>
    pub(crate) fn supports(property: &str, value: &str) -> bool {

        // Step 1: If property is an ASCII case-insensitive match for any defined CSS property
        //          that the UA supports, or is a custom property name string, and value
        //          successfully parses according to that property's grammar, return true.
        // Step 2: Otherwise, return false.
        //
        // Stylo's Declaration::eval() implements both steps in one call.  It parses the
        // declaration string "property: value" by first resolving the property name through
        // PropertyId::parse (checking ASCII case-insensitive matching against the property
        // database) and then parsing the value through PropertyDeclaration::parse_into (checking
        // the value against the property's grammar).  Custom properties (--*) are recognised by
        // PropertyId::parse and their values parsed as arbitrary tokens.
        //
        // The spec notes that no escape or whitespace processing is performed on the property
        // name: Declaration::eval() reads the raw ident before the colon, so " width" (with a
        // leading space) won't match any property.
        let declaration_text = format!("{property}: {value}");
        let declaration = Declaration(declaration_text);
        let url_data = UrlExtraData(ServoArc::new(
            url::Url::parse("about:blank").expect("about:blank is a valid URL"),
        ));
        let context = parser_context_for_supports(&url_data);
        declaration.eval(&context)
    }

    /// <https://drafts.csswg.org/css-conditional-3/#dom-css-supports-conditiontext-conditiontext>
    pub(crate) fn supports_condition(condition_text: &str) -> bool {
        let url_data = UrlExtraData(ServoArc::new(
            url::Url::parse("about:blank").expect("about:blank is a valid URL"),
        ));
        let context = parser_context_for_supports(&url_data);

        // Step 1: If conditionText, parsed and evaluated as a <supports-condition>,
        //          would return true, return true.
        {
            let mut input = cssparser::ParserInput::new(condition_text);
            let mut parser: Parser = cssparser::Parser::new(&mut input);
            if let Ok(condition) = parser.parse_entirely(|input| SupportsCondition::parse(input)) {
                if condition.eval(&context) {
                    return true;
                }
            }
        }

        // Step 2: Otherwise, If conditionText, wrapped in parentheses and then parsed and
        //          evaluated as a <supports-condition>, would return true, return true.
        //
        // The spec says to parse the wrapped text as a <supports-condition>, but the
        // outermost parentheses make the content parseable as either a <supports-condition> or
        // a <declaration> (e.g. "(color: red)" is a parenthesized declaration).  Stylo's
        // parse_condition_or_declaration entry point handles both, matching the spec's intent.
        let wrapped = format!("({condition_text})");
        {
            let mut input = cssparser::ParserInput::new(&wrapped);
            let mut parser: Parser = cssparser::Parser::new(&mut input);
            if let Ok(condition) =
                parser.parse_entirely(|input| parse_condition_or_declaration(input))
            {
                if condition.eval(&context) {
                    return true;
                }
            }
        }

        // Step 3: Otherwise, return false.
        false
    }
}

/// Build a `ParserContext` for evaluating `@supports` conditions.
///
/// The caller must keep `url_data` alive for the lifetime of the returned context.
fn parser_context_for_supports(url_data: &UrlExtraData) -> ParserContext<'_> {
    ParserContext::new(
        Origin::Author,
        url_data,
        Some(CssRuleType::Style),
        ParsingMode::DEFAULT,
        QuirksMode::NoQuirks,
        Cow::Owned(Default::default()),
        None,
        None,
        Default::default(),
    )
}

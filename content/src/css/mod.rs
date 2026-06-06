//! The `CSS` namespace.
//!
//! https://drafts.csswg.org/css-conditional-3/#the-css-namespace

use std::borrow::Cow;

use style::context::QuirksMode;
use style::parser::ParserContext;
use style::servo_arc::Arc as ServoArc;
use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
use style::stylesheets::supports_rule::{Declaration, SupportsCondition, parse_condition_or_declaration};
use style::values::Parser;
use style_traits::ParsingMode;

/// The `CSS` namespace object.
///
/// Holds useful CSS-related functions that do not belong elsewhere.
///
/// https://drafts.csswg.org/css-conditional-3/#the-css-namespace
pub(crate) struct CSS;

impl CSS {
    /// Returns whether a given CSS property and value pair is supported.
    ///
    /// https://drafts.csswg.org/css-conditional-3/#dom-css-supports-conditiontext-conditiontext
    pub(crate) fn supports(property: &str, value: &str) -> bool {
        // Build a declaration string "property: value" as expected by Declaration::eval().
        let declaration_text = format!("{property}: {value}");
        let declaration = Declaration(declaration_text);
        let url_data = UrlExtraData(ServoArc::new(
            url::Url::parse("about:blank").expect("about:blank is a valid URL"),
        ));
        let context = parser_context_for_supports(&url_data);
        declaration.eval(&context)
    }

    /// Returns whether a given supports condition string is supported.
    ///
    /// https://drafts.csswg.org/css-conditional-3/#dom-css-supports-conditiontext-conditiontext
    pub(crate) fn supports_condition(condition_text: &str) -> bool {
        let url_data = UrlExtraData(ServoArc::new(
            url::Url::parse("about:blank").expect("about:blank is a valid URL"),
        ));
        let context = parser_context_for_supports(&url_data);

        // Step 1: Try parsing as a <supports-condition> directly.
        {
            let mut input = cssparser::ParserInput::new(condition_text);
            let mut parser: Parser = cssparser::Parser::new(&mut input);
            if let Ok(condition) =
                parser.parse_entirely(|input| SupportsCondition::parse(input))
            {
                if condition.eval(&context) {
                    return true;
                }
            }
        }

        // Step 2: Wrap in parentheses and try again.
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
/// Uses the given `UrlExtraData` (must outlive the returned context) and
/// author origin, since no real document context is needed for `CSS.supports()`.
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

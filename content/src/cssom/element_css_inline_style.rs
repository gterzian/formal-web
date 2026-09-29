use js_engine::{Completion, ExecutionContext};

use super::CSSStyleDeclaration;
use crate::dom::Element;
use crate::js::Types;

impl Element {
    /// <https://drafts.csswg.org/cssom/#dom-elementcssinlinestyle-style>
    pub(crate) fn style(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<CSSStyleDeclaration, Types> {
        // "The style attribute must return a CSS declaration block object whose readonly flag is unset, whose parent CSS rule is null, whose owner node is the context object, whose computed flag is unset, and whose declarations are the result of parsing the style attribute of the context object."
        // Note: [SameObject]: the block is created on first access and kept
        // on the element.
        if let Some(declaration_block) = self.style.borrow(ec).clone() {
            return Ok(declaration_block);
        }
        let declaration_block = CSSStyleDeclaration::new(Some(self.clone()), None, ec)?;
        *self.style.borrow_mut(ec) = Some(declaration_block.clone());
        Ok(declaration_block)
    }
}

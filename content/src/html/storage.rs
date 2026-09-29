use std::cell::RefCell;
use std::rc::Rc;

use js_engine::{Completion, ExecutionContext, JsTypes, gc_struct};

use crate::dom::DOMException;
use crate::js::Types;
use crate::webidl::create_legacy_platform_object;

type JsObject = <Types as JsTypes>::JsObject;

/// <https://html.spec.whatwg.org/#storage-2>
#[gc_struct]
pub struct Storage {
    /// <https://html.spec.whatwg.org/#concept-storage-map>
    // Note: the map is a storage proxy map over an in-memory bottle map
    // owned by the document; storage partitioning and persistence beyond the
    // document are not implemented, and the document's two holders tell the
    // local map from the session map.
    #[ignore_trace]
    map: Rc<RefCell<Vec<(String, String)>>>,

    pub(crate) reflector: Option<JsObject>,
}

impl Storage {
    /// <https://html.spec.whatwg.org/#storage-2>
    pub(crate) fn new(ec: &mut dyn ExecutionContext<Types>) -> Completion<Storage, Types> {
        let storage = Storage {
            map: Rc::new(RefCell::new(Vec::new())),
            reflector: None,
        };
        let object = create_legacy_platform_object::<Storage>(storage, ec)?;
        ec.with_object_any(&object)
            .and_then(|data| data.downcast_ref::<Storage>().cloned())
            .ok_or_else(|| ec.new_type_error("Storage instance is not a Storage"))
    }

    /// <https://html.spec.whatwg.org/#dom-storage-length>
    pub(crate) fn length(&self) -> u32 {
        // "The length getter steps are to return this's map's size."
        self.map.borrow().len() as u32
    }

    /// <https://html.spec.whatwg.org/#dom-storage-key>
    pub(crate) fn key(&self, index: u32) -> Option<String> {
        // Step 1: "If index is greater than or equal to this's map's size, then return null."
        // Step 2: "Let keys be the result of running get the keys on this's map."
        // Step 3: "Return keys[index]."
        self.map
            .borrow()
            .get(index as usize)
            .map(|(key, _)| key.clone())
    }

    /// <https://html.spec.whatwg.org/#dom-storage-getitem>
    pub(crate) fn get_item(&self, key: &str) -> Option<String> {
        // Step 1: "If this's map[key] does not exist, then return null."
        // Step 2: "Return this's map[key]."
        self.map
            .borrow()
            .iter()
            .find(|(stored_key, _)| stored_key == key)
            .map(|(_, value)| value.clone())
    }

    /// <https://html.spec.whatwg.org/#dom-storage-setitem>
    pub(crate) fn set_item(&self, key: &str, value: &str) -> Result<(), DOMException> {
        // Step 1: "Let oldValue be null."
        // Step 2: "Let reorder be true."
        // Step 3: "If this's map[key] exists:"
        // Step 3.1: "Set oldValue to this's map[key]."
        // Step 3.2: "If oldValue is value, then return."
        // Step 3.3: "Set reorder to false."
        // Step 4: "If value cannot be stored, then throw a "QuotaExceededError" DOMException exception."
        // Note: the in-memory map has no quota.
        // Step 5: "Set this's map[key] to value."
        // Step 6: "If reorder is true, then reorder this."
        // Step 7: "Broadcast this with key, oldValue, and value."
        // Note: reorder keeps the insertion order, and no other Window
        // shares the map, so the broadcast has no recipient.
        let mut map = self.map.borrow_mut();
        match map.iter_mut().find(|(stored_key, _)| stored_key == key) {
            Some((_, stored_value)) => {
                if *stored_value == value {
                    return Ok(());
                }
                *stored_value = value.to_owned();
            }
            None => map.push((key.to_owned(), value.to_owned())),
        }
        Ok(())
    }

    /// <https://html.spec.whatwg.org/#dom-storage-removeitem>
    pub(crate) fn remove_item(&self, key: &str) {
        // Step 1: "If this's map[key] does not exist, then return."
        // Step 2: "Set oldValue to this's map[key]."
        // Step 3: "Remove this's map[key]."
        // Step 4: "Reorder this."
        // Step 5: "Broadcast this with key, oldValue, and null."
        // Note: reorder keeps the remaining order, and no other Window
        // shares the map, so the broadcast has no recipient.
        self.map
            .borrow_mut()
            .retain(|(stored_key, _)| stored_key != key);
    }

    /// <https://html.spec.whatwg.org/#dom-storage-clear>
    pub(crate) fn clear(&self) {
        // Step 1: "Clear this's map."
        self.map.borrow_mut().clear();

        // Step 2: "Broadcast this with null, null, and null."
        // Note: no other Window shares the map, so the broadcast has no
        // recipient.
    }

    /// <https://html.spec.whatwg.org/#the-storage-interface>
    pub(crate) fn supported_property_names(&self) -> Vec<String> {
        // "The supported property names on a Storage object storage are the result of running get the keys on storage's map."
        self.map
            .borrow()
            .iter()
            .map(|(key, _)| key.clone())
            .collect()
    }
}

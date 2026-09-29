use std::cell::RefCell;
use std::rc::Rc;

use js_engine::{Completion, ExecutionContext, JsTypes, gc_struct};
use url::Url;

use crate::js::Types;
use crate::webidl::SequenceOrRecordOrString;
use crate::webidl::bindings::create_interface_instance;

use super::application_x_www_form_urlencoded::{
    application_x_www_form_urlencoded_parser, application_x_www_form_urlencoded_serializer,
};

type JsObject = <Types as JsTypes>::JsObject;

/// <https://url.spec.whatwg.org/#urlsearchparams>
#[gc_struct]
pub(crate) struct URLSearchParams {
    /// <https://url.spec.whatwg.org/#concept-urlsearchparams-list>
    #[ignore_trace]
    list: Rc<RefCell<Vec<(String, String)>>>,

    /// <https://url.spec.whatwg.org/#concept-urlsearchparams-url-object>
    /// Note: The URL record of the URL object, shared with it, in place of
    /// the URL object itself.
    #[ignore_trace]
    url_object: Option<Rc<RefCell<Url>>>,

    pub(crate) reflector: Option<JsObject>,
}

impl URLSearchParams {
    /// <https://url.spec.whatwg.org/#dom-urlsearchparams-urlsearchparams>
    pub(crate) fn constructor(
        mut init: SequenceOrRecordOrString,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: If init is a string and starts with U+003F (?), then remove
        // the first code point from init.
        if let SequenceOrRecordOrString::String(string) = &mut init
            && string.starts_with('?')
        {
            string.remove(0);
        }

        // Step 2: Initialize this with init.
        let query = Self {
            list: Rc::new(RefCell::new(Vec::new())),
            url_object: None,
            reflector: None,
        };
        query.initialize(init, ec)?;
        Ok(query)
    }

    /// The query object of a URL: URL's initialize steps 2 to 4.
    pub(crate) fn new_query_object(
        query: &str,
        url_object: Rc<RefCell<Url>>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        let query_object = Self {
            list: Rc::new(RefCell::new(Vec::new())),
            url_object: Some(url_object),
            reflector: None,
        };
        query_object.initialize(SequenceOrRecordOrString::String(query.to_owned()), ec)?;
        let object = create_interface_instance::<Types, URLSearchParams>(query_object, ec)?;
        // The platform data is cloned back out of the created object; its
        // reflector was set by `create_interface_instance`, so the URL's
        // searchParams getter resolves to this same object.
        ec.with_object_any(&object)
            .and_then(|data| data.downcast_ref::<URLSearchParams>().cloned())
            .ok_or_else(|| ec.new_type_error("URLSearchParams object has no platform data"))
    }

    /// <https://url.spec.whatwg.org/#urlsearchparams-initialize>
    fn initialize(
        &self,
        init: SequenceOrRecordOrString,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let mut list = self.list.borrow_mut();
        match init {
            // Step 1: If init is a sequence, then for each innerSequence of
            // init:
            SequenceOrRecordOrString::Sequence(sequences) => {
                for inner_sequence in sequences {
                    // Step 1.1: If innerSequence's size is not 2, then throw
                    // a TypeError.
                    let [name, value]: [String; 2] = match inner_sequence.try_into() {
                        Ok(pair) => pair,
                        Err(_) => {
                            return Err(ec.new_type_error(
                                "each URLSearchParams init sequence needs exactly two items",
                            ));
                        }
                    };

                    // Step 1.2: Append a new name-value pair whose name is
                    // innerSequence[0] and value is innerSequence[1], to
                    // query's list.
                    list.push((name, value));
                }
            }

            // Step 2: Otherwise, if init is a record, then for each name →
            // value of init, append a new name-value pair whose name is name
            // and value is value, to query's list.
            SequenceOrRecordOrString::Record(pairs) => list.extend(pairs),

            // Step 3: Otherwise:
            SequenceOrRecordOrString::String(string) => {
                // Step 3.1: Assert: init is a string.
                // Step 3.2: Set query's list to the result of parsing init.
                *list = application_x_www_form_urlencoded_parser(string.as_bytes());
            }
        }
        Ok(())
    }

    /// <https://url.spec.whatwg.org/#concept-urlsearchparams-update>
    fn update(&self) {
        // Step 1: If query's URL object is null, then return.
        let Some(url_object) = &self.url_object else {
            return;
        };

        // Step 2: Let serializedQuery be the serialization of query's list.
        let serialized_query = application_x_www_form_urlencoded_serializer(&self.list.borrow());

        // Step 3: If serializedQuery is the empty string, then set
        // serializedQuery to null.
        let serialized_query = if serialized_query.is_empty() {
            None
        } else {
            Some(serialized_query)
        };

        // Step 4: Set query's URL object's URL's query to serializedQuery.
        // Step 5: If serializedQuery is null and query's URL object has an
        // opaque path, then potentially strip trailing spaces from an opaque
        // path with query's URL object.
        // Note: `Url::set_query` strips the trailing spaces of an opaque path
        // when the query becomes null.
        url_object
            .borrow_mut()
            .set_query(serialized_query.as_deref());
    }

    /// URL's href setter step 4 and search setter step 2.2: empty the list.
    pub(crate) fn empty_list(&self) {
        self.list.borrow_mut().clear();
    }

    /// URL's href setter step 6 and search setter step 6: set the list to the
    /// result of parsing a query.
    pub(crate) fn set_list_from_parsing(&self, query: &str) {
        *self.list.borrow_mut() = application_x_www_form_urlencoded_parser(query.as_bytes());
    }

    /// <https://url.spec.whatwg.org/#dom-urlsearchparams-size>
    pub(crate) fn size(&self) -> u32 {
        // The size getter steps are to return this's list's size.
        self.list.borrow().len() as u32
    }

    /// <https://url.spec.whatwg.org/#dom-urlsearchparams-append>
    pub(crate) fn append(&self, name: String, value: String) {
        // Step 1: Append a new name-value pair whose name is name and value
        // is value, to this's list.
        self.list.borrow_mut().push((name, value));

        // Step 2: Update this.
        self.update();
    }

    /// <https://url.spec.whatwg.org/#dom-urlsearchparams-delete>
    pub(crate) fn delete(&self, name: String, value: Option<String>) {
        // Step 1: If value is given, then remove all name-value pairs whose
        // name is name and value is value from this's list.
        // Step 2: Otherwise, remove all name-value pairs whose name is name
        // from this's list.
        self.list.borrow_mut().retain(|(pair_name, pair_value)| {
            *pair_name != name || value.as_ref().is_some_and(|value| pair_value != value)
        });

        // Step 3: Update this.
        self.update();
    }

    /// <https://url.spec.whatwg.org/#dom-urlsearchparams-get>
    pub(crate) fn get(&self, name: String) -> Option<String> {
        // The get(name) method steps are to return the value of the first
        // name-value pair whose name is name in this's list, if there is such
        // a pair; otherwise null.
        self.list
            .borrow()
            .iter()
            .find(|(pair_name, _)| *pair_name == name)
            .map(|(_, value)| value.clone())
    }

    /// <https://url.spec.whatwg.org/#dom-urlsearchparams-getall>
    pub(crate) fn get_all(&self, name: String) -> Vec<String> {
        // The getAll(name) method steps are to return the values of all
        // name-value pairs whose name is name, in this's list, in list order;
        // otherwise the empty sequence.
        self.list
            .borrow()
            .iter()
            .filter(|(pair_name, _)| *pair_name == name)
            .map(|(_, value)| value.clone())
            .collect()
    }

    /// <https://url.spec.whatwg.org/#dom-urlsearchparams-has>
    pub(crate) fn has(&self, name: String, value: Option<String>) -> bool {
        // Step 1: If value is given and there is a name-value pair whose name
        // is name and value is value in this's list, then return true.
        // Step 2: If value is not given and there is a name-value pair whose
        // name is name in this's list, then return true.
        // Step 3: Return false.
        self.list.borrow().iter().any(|(pair_name, pair_value)| {
            *pair_name == name && value.as_ref().is_none_or(|value| pair_value == value)
        })
    }

    /// <https://url.spec.whatwg.org/#dom-urlsearchparams-set>
    pub(crate) fn set(&self, name: String, value: String) {
        {
            let mut list = self.list.borrow_mut();
            // Step 1: If this's list contains any name-value pairs whose name
            // is name, then set the value of the first such name-value pair
            // to value and remove the others.
            if let Some(first) = list.iter().position(|(pair_name, _)| *pair_name == name) {
                list[first].1 = value;
                let mut index = 0;
                list.retain(|(pair_name, _)| {
                    let keep = index == first || *pair_name != name;
                    index += 1;
                    keep
                });
            } else {
                // Step 2: Otherwise, append a new name-value pair whose name
                // is name and value is value, to this's list.
                list.push((name, value));
            }
        }

        // Step 3: Update this.
        self.update();
    }

    /// <https://url.spec.whatwg.org/#dom-urlsearchparams-sort>
    pub(crate) fn sort(&self) {
        // Step 1: Sort all name-value pairs, if any, by their names. Sorting
        // must be done by comparison of code units. The relative order
        // between name-value pairs with equal names must be preserved.
        self.list
            .borrow_mut()
            .sort_by(|(left, _), (right, _)| left.encode_utf16().cmp(right.encode_utf16()));

        // Step 2: Update this.
        self.update();
    }

    /// <https://url.spec.whatwg.org/#urlsearchparams-stringification-behavior>
    pub(crate) fn stringification_behavior(&self) -> String {
        // The stringification behavior steps are to return the serialization
        // of this's list.
        application_x_www_form_urlencoded_serializer(&self.list.borrow())
    }

    /// <https://url.spec.whatwg.org/#interface-urlsearchparams>
    pub(crate) fn value_pairs_to_iterate_over(&self) -> Vec<(String, String)> {
        // The value pairs to iterate over are this's list.
        self.list.borrow().clone()
    }
}

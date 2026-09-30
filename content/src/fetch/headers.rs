use std::cell::{Cell, RefCell};
use std::rc::Rc;

use js_engine::{Completion, ExecutionContext, JsTypes, gc_struct};

use crate::js::Types;

use super::{
    HeaderList, is_forbidden_request_header, is_forbidden_response_header_name, is_header_name,
    is_header_value, is_no_cors_safelisted_request_header,
    is_no_cors_safelisted_request_header_name, is_privileged_no_cors_request_header_name,
    normalize_header_value,
};

type JsObject = <Types as JsTypes>::JsObject;

/// <https://fetch.spec.whatwg.org/#concept-headers-guard>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeadersGuard {
    Immutable,
    Request,
    RequestNoCors,
    Response,
    None,
}

/// <https://fetch.spec.whatwg.org/#typedefdef-headersinit>
pub(crate) enum HeadersInit {
    Sequence(Vec<Vec<String>>),
    Record(Vec<(String, String)>),
}

/// <https://fetch.spec.whatwg.org/#headers-class>
#[gc_struct]
pub(crate) struct Headers {
    /// <https://fetch.spec.whatwg.org/#concept-headers-header-list>
    #[ignore_trace]
    header_list: Rc<RefCell<HeaderList>>,

    /// <https://fetch.spec.whatwg.org/#concept-headers-guard>
    #[ignore_trace]
    guard: Rc<Cell<HeadersGuard>>,

    /// <https://webidl.spec.whatwg.org/#dfn-platform-object>
    pub(crate) reflector: Option<JsObject>,
}

impl Headers {
    /// A new Headers object whose header list is `header_list` and guard is
    /// `guard`.
    pub(crate) fn new(header_list: Rc<RefCell<HeaderList>>, guard: HeadersGuard) -> Self {
        Self {
            header_list,
            guard: Rc::new(Cell::new(guard)),
            reflector: None,
        }
    }

    /// <https://fetch.spec.whatwg.org/#dom-headers>
    pub(crate) fn constructor(
        init: Option<HeadersInit>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: Set this's guard to "none".
        let headers = Self::new(Rc::new(RefCell::new(HeaderList::new())), HeadersGuard::None);

        // Step 2: If init is given, then fill this with init.
        if let Some(init) = init {
            headers.fill(init, ec)?;
        }
        Ok(headers)
    }

    pub(crate) fn header_list(&self) -> Rc<RefCell<HeaderList>> {
        Rc::clone(&self.header_list)
    }

    pub(crate) fn guard(&self) -> HeadersGuard {
        self.guard.get()
    }

    pub(crate) fn set_guard(&self, guard: HeadersGuard) {
        self.guard.set(guard);
    }

    /// <https://fetch.spec.whatwg.org/#concept-headers-fill>
    pub(crate) fn fill(
        &self,
        object: HeadersInit,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        match object {
            // Step 1: If object is a sequence, then for each header of
            // object:
            HeadersInit::Sequence(sequence) => {
                for header in sequence {
                    // Step 1.1: If header's size is not 2, then throw a
                    // TypeError.
                    let [name, value]: [String; 2] = match header.try_into() {
                        Ok(pair) => pair,
                        Err(_) => {
                            return Err(ec.new_type_error(
                                "each Headers init entry needs exactly two items",
                            ));
                        }
                    };

                    // Step 1.2: Append (header[0], header[1]) to headers.
                    self.append(&name, &value, ec)?;
                }
            }
            // Step 2: Otherwise, object is a record, then for each key →
            // value of object, append (key, value) to headers.
            HeadersInit::Record(record) => {
                for (key, value) in record {
                    self.append(&key, &value, ec)?;
                }
            }
        }
        Ok(())
    }

    /// <https://fetch.spec.whatwg.org/#headers-validate>
    fn validate(
        &self,
        name: &str,
        value: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<bool, Types> {
        // Step 1: If name is not a header name or value is not a header
        // value, then throw a TypeError.
        if !is_header_name(name) {
            return Err(ec.new_type_error(&format!("'{name}' is not a valid HTTP header name")));
        }
        if !is_header_value(value) {
            return Err(ec.new_type_error(&format!(
                "'{value}' is not a valid HTTP header value for '{name}'"
            )));
        }

        // Step 2: If headers's guard is "immutable", then throw a TypeError.
        if self.guard.get() == HeadersGuard::Immutable {
            return Err(ec.new_type_error("Headers are immutable"));
        }

        // Step 3: If headers's guard is "request" and (name, value) is a
        // forbidden request-header, then return false.
        if self.guard.get() == HeadersGuard::Request && is_forbidden_request_header(name, value) {
            return Ok(false);
        }

        // Step 4: If headers's guard is "response" and name is a forbidden
        // response-header name, then return false.
        if self.guard.get() == HeadersGuard::Response && is_forbidden_response_header_name(name) {
            return Ok(false);
        }

        // Step 5: Return true.
        Ok(true)
    }

    /// <https://fetch.spec.whatwg.org/#concept-headers-append>
    pub(crate) fn append(
        &self,
        name: &str,
        value: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: Normalize value.
        let value = normalize_header_value(value);

        // Step 2: If validating (name, value) for headers returns false, then
        // return.
        if !self.validate(name, &value, ec)? {
            return Ok(());
        }

        // Step 3: If headers's guard is "request-no-cors":
        if self.guard.get() == HeadersGuard::RequestNoCors {
            // Step 3.1: Let temporaryValue be the result of getting name from
            // headers's header list.
            // Step 3.2: If temporaryValue is null, then set temporaryValue to
            // value.
            // Step 3.3: Otherwise, set temporaryValue to temporaryValue,
            // followed by 0x2C 0x20, followed by value.
            let temporary_value = match self.header_list.borrow().get(name) {
                None => value.clone(),
                Some(existing) => format!("{existing}, {value}"),
            };

            // Step 3.4: If (name, temporaryValue) is not a no-CORS-safelisted
            // request-header, then return.
            if !is_no_cors_safelisted_request_header(name, &temporary_value) {
                return Ok(());
            }
        }

        // Step 4: Append (name, value) to headers's header list.
        self.header_list.borrow_mut().append(name, &value);

        // Step 5: If headers's guard is "request-no-cors", then remove
        // privileged no-CORS request-headers from headers.
        self.remove_privileged_no_cors_request_headers();
        Ok(())
    }

    /// <https://fetch.spec.whatwg.org/#dom-headers-delete>
    pub(crate) fn delete(
        &self,
        name: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: If validating (name, ``) for this returns false, then
        // return.
        if !self.validate(name, "", ec)? {
            return Ok(());
        }

        // Step 2: If this's guard is "request-no-cors", name is not a
        // no-CORS-safelisted request-header name, and name is not a
        // privileged no-CORS request-header name, then return.
        if self.guard.get() == HeadersGuard::RequestNoCors
            && !is_no_cors_safelisted_request_header_name(name)
            && !is_privileged_no_cors_request_header_name(name)
        {
            return Ok(());
        }

        // Step 3: If this's header list does not contain name, then return.
        if !self.header_list.borrow().contains(name) {
            return Ok(());
        }

        // Step 4: Delete name from this's header list.
        self.header_list.borrow_mut().delete(name);

        // Step 5: If this's guard is "request-no-cors", then remove
        // privileged no-CORS request-headers from this.
        self.remove_privileged_no_cors_request_headers();
        Ok(())
    }

    /// <https://fetch.spec.whatwg.org/#dom-headers-get>
    pub(crate) fn get(
        &self,
        name: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Option<String>, Types> {
        // Step 1: If name is not a header name, then throw a TypeError.
        if !is_header_name(name) {
            return Err(ec.new_type_error(&format!("'{name}' is not a valid HTTP header name")));
        }

        // Step 2: Return the result of getting name from this's header list.
        Ok(self.header_list.borrow().get(name))
    }

    /// <https://fetch.spec.whatwg.org/#dom-headers-getsetcookie>
    pub(crate) fn get_set_cookie(&self) -> Vec<String> {
        // Step 1: If this's header list does not contain `Set-Cookie`, then
        // return « ».
        // Step 2: Return the values of all headers in this's header list
        // whose name is a byte-case-insensitive match for `Set-Cookie`, in
        // order.
        self.header_list
            .borrow()
            .pairs()
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("set-cookie"))
            .map(|(_, value)| value.clone())
            .collect()
    }

    /// <https://fetch.spec.whatwg.org/#dom-headers-has>
    pub(crate) fn has(
        &self,
        name: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<bool, Types> {
        // Step 1: If name is not a header name, then throw a TypeError.
        if !is_header_name(name) {
            return Err(ec.new_type_error(&format!("'{name}' is not a valid HTTP header name")));
        }

        // Step 2: Return true if this's header list contains name; otherwise
        // false.
        Ok(self.header_list.borrow().contains(name))
    }

    /// <https://fetch.spec.whatwg.org/#dom-headers-set>
    pub(crate) fn set(
        &self,
        name: &str,
        value: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: Normalize value.
        let value = normalize_header_value(value);

        // Step 2: If validating (name, value) for this returns false, then
        // return.
        if !self.validate(name, &value, ec)? {
            return Ok(());
        }

        // Step 3: If this's guard is "request-no-cors" and (name, value) is
        // not a no-CORS-safelisted request-header, then return.
        if self.guard.get() == HeadersGuard::RequestNoCors
            && !is_no_cors_safelisted_request_header(name, &value)
        {
            return Ok(());
        }

        // Step 4: Set (name, value) in this's header list.
        self.header_list.borrow_mut().set(name, &value);

        // Step 5: If this's guard is "request-no-cors", then remove
        // privileged no-CORS request-headers from this.
        self.remove_privileged_no_cors_request_headers();
        Ok(())
    }

    /// <https://fetch.spec.whatwg.org/#concept-headers-remove-privileged-no-cors-request-headers>
    fn remove_privileged_no_cors_request_headers(&self) {
        if self.guard.get() != HeadersGuard::RequestNoCors {
            return;
        }

        // Step 1: For each headerName of privileged no-CORS request-header
        // names: Delete headerName from headers's header list.
        self.header_list.borrow_mut().delete("range");
    }

    /// <https://fetch.spec.whatwg.org/#headers-class>
    pub(crate) fn value_pairs_to_iterate_over(&self) -> Vec<(String, String)> {
        // The value pairs to iterate over are the return value of running
        // sort and combine with this's header list.
        self.header_list.borrow().sort_and_combine()
    }
}

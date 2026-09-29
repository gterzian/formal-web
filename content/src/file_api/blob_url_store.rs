use js_engine::{Completion, ExecutionContext};
use url::Url;
use uuid::Uuid;

use crate::js::Types;
use crate::js::platform_objects::with_global_scope;

use super::Blob;

/// <https://w3c.github.io/FileAPI/#dfn-createObjectURL>
pub(crate) fn create_object_url(
    object: Blob,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<String, Types> {
    // Step 1: Return the result of adding an entry to the blob URL store for
    // obj.
    add_an_entry(object, ec)
}

/// <https://w3c.github.io/FileAPI/#dfn-revokeObjectURL>
pub(crate) fn revoke_object_url(
    url: String,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    // Step 1: Let url record be the result of parsing url.
    // Step 2: If url record's scheme is not "blob", return.
    let Some(url_record) = Url::parse(&url)
        .ok()
        .filter(|record| record.scheme() == "blob")
    else {
        return Ok(());
    };

    // Step 3: Let origin be the origin of url record.
    let origin = url_record.origin();

    // Step 4: Let settings be the current settings object.
    // Step 5: If origin is not same origin with settings's origin, return.
    // Step 6: Remove an entry from the Blob URL Store for url.
    with_global_scope(ec, move |global_scope, _ec| {
        let settings_origin = global_scope.creation_url().map(|url| url.origin());
        if settings_origin.as_ref() != Some(&origin) {
            return Ok(());
        }
        remove_an_entry(global_scope, &url);
        Ok(())
    })
}

/// <https://w3c.github.io/FileAPI/#add-an-entry>
fn add_an_entry(object: Blob, ec: &mut dyn ExecutionContext<Types>) -> Completion<String, Types> {
    // Step 1: Let store be the user agent's blob URL store.
    // Note: The store is the realm's global scope's; a blob URL is not
    // reachable from another agent.
    with_global_scope(ec, move |global_scope, _ec| {
        // Step 2: Let url be the result of generating a new blob URL.
        let url = generate_a_new_blob_url(global_scope.creation_url());

        // Step 3: Let entry be a new blob URL entry consisting of object and
        // the current settings object.
        // Step 4: Set store[url] to entry.
        global_scope.add_blob_url_entry(url.clone(), object);

        // Step 5: Return url.
        Ok(url)
    })
}

/// <https://w3c.github.io/FileAPI/#unicodeBlobURL>
fn generate_a_new_blob_url(settings_creation_url: Option<Url>) -> String {
    // Step 1: Let result be the empty string.
    let mut result = String::new();

    // Step 2: Append the string "blob:" to result.
    result.push_str("blob:");

    // Step 3: Let settings be the current settings object.
    // Step 4: Let origin be settings's origin.
    // Step 5: Let serialized be the ASCII serialization of origin.
    let mut serialized = settings_creation_url
        .map(|url| url.origin().ascii_serialization())
        .unwrap_or_else(|| String::from("null"));

    // Step 6: If serialized is "null", set it to an implementation-defined
    // value.
    if serialized == "null" {
        serialized = String::from("null");
    }

    // Step 7: Append serialized to result.
    result.push_str(&serialized);

    // Step 8: Append U+0024 SOLIDUS (/) to result.
    result.push('/');

    // Step 9: Append the result of generating a UUID [WEBIDL] to result.
    result.push_str(&Uuid::new_v4().to_string());

    // Step 10: Return result.
    result
}

/// <https://w3c.github.io/FileAPI/#removeTheEntry>
fn remove_an_entry(global_scope: &crate::html::GlobalScope, url: &str) {
    // Step 1: Let store be the user agent's blob URL store;
    // Step 2: Let url string be the result of serializing url.
    // Step 3: Remove store[url string].
    global_scope.remove_blob_url_entry(url);
}

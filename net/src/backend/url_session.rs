//! The Apple URLSession network backend: one session (with no shared cache)
//! per event-loop session (network partition key), stored and reused.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use ipc_messages::content::{FetchRequest, FetchResponse};
use url_session_sys::UrlSession;
use uuid::Uuid;

use super::{FetchReplySender, NetworkBackend, NetworkPartitionKey, handle_local_schemes};

/// <https://fetch.spec.whatwg.org/#http-network-fetch>
pub struct UrlSessionBackend {
    // One NSURLSession with no shared cache per network partition key
    // (event loop id).
    sessions: HashMap<NetworkPartitionKey, UrlSession>,
}

impl UrlSessionBackend {
    pub fn new() -> Self {
        UrlSessionBackend {
            sessions: HashMap::new(),
        }
    }

    /// <https://fetch.spec.whatwg.org/#http-network-fetch>
    fn session_for(&mut self, key: NetworkPartitionKey) -> Result<&UrlSession, String> {
        // The session for the given partition key, creating and storing one
        // on first use.
        match self.sessions.entry(key) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) => {
                let session = UrlSession::new()
                    .map_err(|error| format!("failed to create URLSession: {error}"))?;
                Ok(entry.insert(session))
            }
        }
    }
}

impl NetworkBackend for UrlSessionBackend {
    /// <https://fetch.spec.whatwg.org/#http-network-or-cache-fetch>
    fn http_network_or_cache_fetch(
        &mut self,
        key: NetworkPartitionKey,
        request_id: Uuid,
        request: &FetchRequest,
        reply_sender: FetchReplySender,
    ) -> Result<(), String> {
        // Local schemes never touch the transport.
        match handle_local_schemes(request) {
            Ok(Some(response)) => {
                return reply_sender
                    .send((request_id, Ok(response)))
                    .map_err(|error| format!("failed to send fetch reply: {error}"));
            }
            Err(error) => {
                return reply_sender
                    .send((request_id, Err(error)))
                    .map_err(|error| format!("failed to send fetch reply: {error}"));
            }
            Ok(None) => {}
        }

        let session = match self.session_for(key) {
            Ok(session) => session,
            Err(error) => {
                return reply_sender
                    .send((request_id, Err(error)))
                    .map_err(|error| format!("failed to send fetch reply: {error}"));
            }
        };

        let method = request.method.clone();
        let url = request.url.clone();
        let body = request.body.clone();
        let body_bytes = (!body.is_empty()).then_some(body.as_bytes());
        let completion_reply_sender = reply_sender.clone();
        if let Err(error) = session.fetch(
            &method,
            &url,
            &request.header_list,
            body_bytes,
            move |result| {
                let result = result.map(|response| FetchResponse {
                    final_url: response.final_url,
                    status: response.status,
                    content_type: response.content_type,
                    body: response.body,
                });
                if let Err(send_error) = completion_reply_sender.send((request_id, result)) {
                    log::error!("failed to route URLSession response: {send_error}");
                }
            },
        ) {
            // The task could not be started; the completion callback was not
            // invoked, so deliver the failure to the caller now.
            let result = Err(format!("failed to start URLSession fetch: {error}"));
            return reply_sender
                .send((request_id, result))
                .map_err(|error| format!("failed to send fetch reply: {error}"));
        }
        Ok(())
    }
}

impl Default for UrlSessionBackend {
    fn default() -> Self {
        Self::new()
    }
}

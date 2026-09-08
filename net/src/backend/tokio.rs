//! The `tokio` network backend: reqwest (blocking) behind the
//! [`NetworkBackend`] trait. One client per event-loop session (network
//! partition key), stored and reused.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::net::{Ipv4Addr, SocketAddr};

use ipc_messages::content::{FetchRequest, FetchResponse};
use reqwest::Method;
use reqwest::blocking::Client;
use reqwest::header::CONTENT_TYPE;
use url::Url;
use uuid::Uuid;

use super::{FetchReplySender, NetworkBackend, NetworkPartitionKey, handle_local_schemes};

/// <https://fetch.spec.whatwg.org/#http-network-fetch>
pub struct TokioBackend {
    // One client per network partition key (event loop id).
    clients: HashMap<NetworkPartitionKey, Client>,
}

impl TokioBackend {
    pub fn new() -> Self {
        TokioBackend {
            clients: HashMap::new(),
        }
    }

    /// <https://fetch.spec.whatwg.org/#http-network-fetch>
    fn client_for(&mut self, key: NetworkPartitionKey) -> Result<&Client, String> {
        // The client for the given partition key, creating and storing one
        // on first use.
        match self.clients.entry(key) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) => {
                let client = Client::builder()
                    .resolve("localhost", SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
                    .build()
                    .map_err(|error| format!("failed to build reqwest client: {error}"))?;
                Ok(entry.insert(client))
            }
        }
    }

    /// <https://fetch.spec.whatwg.org/#http-network-or-cache-fetch>
    fn fetch(
        &mut self,
        key: NetworkPartitionKey,
        request: &FetchRequest,
    ) -> Result<FetchResponse, String> {
        // Local schemes never touch the transport.
        if let Some(response) = handle_local_schemes(request)? {
            return Ok(response);
        }

        let client = self.client_for(key)?;
        let parsed = Url::parse(&request.url).map_err(|error| format!("invalid URL: {error}"))?;
        let method = Method::from_bytes(request.method.as_bytes())
            .map_err(|error| format!("invalid HTTP method: {error}"))?;
        let mut builder = client.request(method, parsed);
        for (name, value) in &request.header_list {
            builder = builder.header(name, value);
        }
        if !request.body.is_empty() {
            builder = builder.body(request.body.clone());
        }

        let response = builder
            .send()
            .map_err(|error| format!("network request failed: {error}"))?;
        let final_url = response.url().to_string();
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let body = response
            .bytes()
            .map_err(|error| format!("failed to read response body: {error}"))?
            .to_vec();

        Ok(FetchResponse {
            final_url,
            status,
            content_type,
            body,
        })
    }
}

impl NetworkBackend for TokioBackend {
    /// <https://fetch.spec.whatwg.org/#http-network-or-cache-fetch>
    fn http_network_or_cache_fetch(
        &mut self,
        key: NetworkPartitionKey,
        request_id: Uuid,
        request: &FetchRequest,
        reply_sender: FetchReplySender,
    ) -> Result<(), String> {
        let result = self.fetch(key, request);
        reply_sender
            .send((request_id, result))
            .map_err(|error| format!("failed to send fetch reply: {error}"))
    }
}

impl Default for TokioBackend {
    fn default() -> Self {
        Self::new()
    }
}

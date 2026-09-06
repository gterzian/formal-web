pub mod backend;

use backend::{Backend, FetchReply, NetworkBackend, NetworkPartitionKey};
use ipc_messages::content::{
    Command as ContentCommand, DocumentFetchId, EventLoopId, FetchRequest, FetchResponse,
};
use ipc_messages::network::{Request, Response, ResponseRecipient};
use std::collections::{HashMap, HashSet};
use std::env;
use url::Url;
use uuid::Uuid;

fn net_token_from_args() -> Result<Option<String>, String> {
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--net-token" {
            return args
                .next()
                .map(Some)
                .ok_or_else(|| String::from("missing net token value"));
        }
    }
    Ok(None)
}

/// <https://fetch.spec.whatwg.org/#scheme-fetch>
// Note: the spec's scheme fetch switches on the request's URL scheme; the
// embedder-served schemes are an extension point ahead of that switch, so a
// scheme the embedder claims never reaches a network backend.
fn is_embedder_scheme(embedder_schemes: &HashSet<String>, url: &str) -> bool {
    if embedder_schemes.is_empty() {
        return false;
    }
    Url::parse(url).is_ok_and(|parsed| embedder_schemes.contains(parsed.scheme()))
}

/// <https://fetch.spec.whatwg.org/#queue-a-fetch-task>
fn route_response(
    request_id: Uuid,
    reply_to: ResponseRecipient,
    result: Result<FetchResponse, String>,
    ua_sender: &ipc::IpcSender<Response>,
) -> Result<(), String> {
    match reply_to {
        ResponseRecipient::ContentProcess {
            content_command_sender,
            handler_id,
        } => match result {
            Ok(response) => content_command_sender
                .send(ContentCommand::CompleteDocumentFetch {
                    handler_id,
                    response,
                })
                .map_err(|error| format!("failed to route response to content: {error}")),
            Err(error) => {
                log::error!("fetch failed: {error}");
                content_command_sender
                    .send(ContentCommand::FailDocumentFetch { handler_id })
                    .map_err(|error| format!("failed to route fetch failure to content: {error}"))
            }
        },
        ResponseRecipient::UserAgent => ua_sender
            .send(Response::Fetch { request_id, result })
            .map_err(|error| format!("failed to route response to UA: {error}")),
    }
}

/// Start one fetch: an embedder-served scheme goes to the user agent, which
/// asks the embedder for the response; everything else goes to the network
/// backend.
/// <https://fetch.spec.whatwg.org/#concept-fetch>
#[allow(clippy::too_many_arguments)]
fn start_a_fetch(
    net_backend: &mut Backend,
    embedder_schemes: &HashSet<String>,
    pending: &mut HashMap<Uuid, ResponseRecipient>,
    ua_sender: &ipc::IpcSender<Response>,
    reply_sender: &backend::FetchReplySender,
    event_loop_id: EventLoopId,
    request_id: Uuid,
    request: FetchRequest,
    reply_to: ResponseRecipient,
) -> Result<(), String> {
    pending.insert(request_id, reply_to);

    let outcome = if is_embedder_scheme(embedder_schemes, &request.url) {
        ua_sender
            .send(Response::EmbedderSchemeFetch {
                event_loop_id,
                request_id,
                request,
            })
            .map_err(|error| format!("failed to route an embedder-scheme fetch to the UA: {error}"))
    } else {
        net_backend.http_network_or_cache_fetch(
            NetworkPartitionKey(event_loop_id),
            request_id,
            &request,
            reply_sender.clone(),
        )
    };

    if outcome.is_err() {
        pending.remove(&request_id);
    }
    outcome
}

pub fn run_net_process_v2(token: String) -> Result<(), String> {
    ipc::run_extension::<Request, Response>(&token, move |server| {
        // The persistent net→UA channel: navigation fetch responses are
        // delivered on the sender end of this process's own bootstrap
        // connection.
        let ua_sender = server.connection.sender;
        let request_receiver = ipc::crossbeam_proxy(server.connection.receiver);
        let (reply_sender, reply_receiver) = crossbeam_channel::unbounded::<FetchReply>();
        // The reply_to recipient of each in-flight request, keyed by request
        // id, so a backend reply can be routed to its caller.
        let mut pending: HashMap<Uuid, ResponseRecipient> = HashMap::new();
        let mut embedder_schemes: HashSet<String> = HashSet::new();
        let mut net_backend = Backend::new();

        loop {
            crossbeam_channel::select! {
                recv(request_receiver) -> incoming => {
                    match incoming {
                        Ok(incoming) => {
                            let request = incoming.payload;
                            match request {
                                Request::SetTraceSender(_) => {}
                                Request::SetEmbedderSchemes { schemes } => {
                                    embedder_schemes = schemes.into_iter().collect();
                                }
                                Request::Fetch {
                                    event_loop_id,
                                    request_id,
                                    request,
                                    reply_to,
                                } => {
                                    log::debug!("[net] fetch event_loop={event_loop_id} url={}", request.url);
                                    if let Err(error) = start_a_fetch(
                                        &mut net_backend,
                                        &embedder_schemes,
                                        &mut pending,
                                        &ua_sender,
                                        &reply_sender,
                                        event_loop_id,
                                        request_id,
                                        request,
                                        reply_to,
                                    ) {
                                        log::error!("{error}");
                                        break;
                                    }
                                }
                                Request::NavigationFetch {
                                    event_loop_id,
                                    request_id,
                                    request,
                                    reply_to,
                                } => {
                                    log::debug!(
                                        "[net] navigation fetch event_loop={event_loop_id} url={}",
                                        request.url
                                    );
                                    // Convert NavigationFetchRequest to FetchRequest for HTTP transport.
                                    let fetch_request = FetchRequest {
                                        handler_id: DocumentFetchId::new(),
                                        url: request.url,
                                        method: request.method,
                                        header_list: request.header_list,
                                        body: request.body.unwrap_or_default(),
                                    };
                                    if let Err(error) = start_a_fetch(
                                        &mut net_backend,
                                        &embedder_schemes,
                                        &mut pending,
                                        &ua_sender,
                                        &reply_sender,
                                        event_loop_id,
                                        request_id,
                                        fetch_request,
                                        reply_to,
                                    ) {
                                        log::error!("{error}");
                                        break;
                                    }
                                }
                                Request::CompleteEmbedderSchemeFetch { request_id, result } => {
                                    if let Some(reply_to) = pending.remove(&request_id)
                                        && let Err(error) =
                                            route_response(request_id, reply_to, result, &ua_sender)
                                    {
                                        log::error!("{error}");
                                    }
                                }
                                Request::Shutdown => break,
                            }
                        }
                        Err(_) => break,
                    }
                }
                recv(reply_receiver) -> reply => {
                    match reply {
                        Ok((request_id, result)) => {
                            if let Some(reply_to) = pending.remove(&request_id)
                                && let Err(error) =
                                    route_response(request_id, reply_to, result, &ua_sender)
                            {
                                log::error!("{error}");
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        }

        Ok(())
    })
}

pub fn run_net_process_from_args() -> Result<(), String> {
    let token = net_token_from_args()?;
    run_net_process_v2(token.unwrap_or_default())
}

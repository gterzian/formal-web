pub mod backend;

use backend::{Backend, FetchReply, NetworkBackend, NetworkPartitionKey};
use ipc_messages::content::{
    Command as ContentCommand, DocumentFetchId, FetchRequest, FetchResponse,
};
use ipc_messages::network::{Request, Response, ResponseRecipient};
use std::collections::HashMap;
use std::env;
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
            .send(Response { request_id, result })
            .map_err(|error| format!("failed to route response to UA: {error}")),
    }
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
        let mut net_backend = Backend::new();

        loop {
            crossbeam_channel::select! {
                recv(request_receiver) -> incoming => {
                    match incoming {
                        Ok(incoming) => {
                            let request = incoming.payload;
                            match request {
                                Request::SetTraceSender(_) => {}
                                Request::Fetch {
                                    event_loop_id,
                                    request_id,
                                    request,
                                    reply_to,
                                } => {
                                    log::debug!("[net] fetch event_loop={event_loop_id} url={}", request.url);
                                    let key = NetworkPartitionKey(event_loop_id);
                                    pending.insert(request_id, reply_to);
                                    if let Err(error) = net_backend.http_network_or_cache_fetch(
                                        key,
                                        request_id,
                                        &request,
                                        reply_sender.clone(),
                                    ) {
                                        pending.remove(&request_id);
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
                                    let key = NetworkPartitionKey(event_loop_id);
                                    pending.insert(request_id, reply_to);
                                    if let Err(error) = net_backend.http_network_or_cache_fetch(
                                        key,
                                        request_id,
                                        &fetch_request,
                                        reply_sender.clone(),
                                    ) {
                                        pending.remove(&request_id);
                                        log::error!("{error}");
                                        break;
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
                            let Some(reply_to) = pending.remove(&request_id) else {
                                continue;
                            };
                            if let Err(error) = route_response(request_id, reply_to, result, &ua_sender) {
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

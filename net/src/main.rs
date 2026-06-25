use ipc_messages::content::{
    Command as ContentCommand, DocumentFetchId, FetchRequest, FetchResponse,
};
use ipc_messages::network::{Request, Response, ResponseRecipient};
use reqwest::Method;
use reqwest::blocking::Client;
use reqwest::header::CONTENT_TYPE;
use std::env;
use std::fs;
use std::net::{Ipv4Addr, SocketAddr};
use url::Url;

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

fn fetch_file_url(url: &str) -> Result<FetchResponse, String> {
    let parsed = Url::parse(url).map_err(|error| format!("invalid file URL: {error}"))?;
    let path = parsed
        .to_file_path()
        .map_err(|_| String::from("failed to convert file URL to local path"))?;
    let body = fs::read(&path).map_err(|error| format!("failed to read file URL body: {error}"))?;
    let content_type = mime_guess::from_path(&path)
        .first_raw()
        .unwrap_or("application/octet-stream")
        .to_owned();
    Ok(FetchResponse {
        final_url: url.to_owned(),
        status: 200,
        content_type,
        body,
    })
}

fn fetch_request(client: &Client, request: &FetchRequest) -> Result<FetchResponse, String> {
    let parsed = Url::parse(&request.url).map_err(|error| format!("invalid URL: {error}"))?;
    if parsed.scheme() == "file" {
        return fetch_file_url(&request.url);
    }

    // Handle about:blank locally — return an empty HTML document.
    // Reqwest cannot construct an HTTP request for about: URLs.
    if parsed.scheme() == "about" && parsed.path() == "blank" {
        return Ok(FetchResponse {
            final_url: String::from("about:blank"),
            status: 200,
            content_type: String::from("text/html; charset=utf-8"),
            body: Vec::new(),
        });
    }

    let method = Method::from_bytes(request.method.as_bytes())
        .map_err(|error| format!("invalid HTTP method: {error}"))?;
    let mut builder = client.request(method, parsed);
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

/// Route a fetch result to the caller based on `ResponseRecipient`.
fn route_response(
    request_id: uuid::Uuid,
    reply_to: ResponseRecipient,
    result: Result<FetchResponse, String>,
    ipc_response_sender: &ipc::IpcSender<Response>,
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
        ResponseRecipient::UserAgent => ipc_response_sender
            .send(Response { request_id, result })
            .map_err(|error| format!("failed to route response to UA: {error}")),
    }
}

pub fn run_net_process_v2(token: String) -> Result<(), String> {
    let client = Client::builder()
        .resolve("localhost", SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .build()
        .map_err(|error| format!("failed to build reqwest client: {error}"))?;

    ipc::run_extension::<Request, Response>(&token, move |server| {
        let request_receiver = ipc::crossbeam_proxy(server.connection.receiver);
        let response_sender = server.connection.sender.clone();

        loop {
            match request_receiver.recv() {
                Ok(incoming) => {
                    let request = incoming.payload;
                    match request {
                        Request::SetTraceSender(trace_sender) => {
                            let _ = trace_sender;
                        }
                        Request::Fetch {
                            request_id,
                            request,
                            reply_to,
                        } => {
                            let result = fetch_request(&client, &request);
                            if let Err(error) =
                                route_response(request_id, reply_to, result, &response_sender)
                            {
                                log::error!("{error}");
                                break;
                            }
                        }
                        Request::NavigationFetch {
                            request_id,
                            request,
                            reply_to,
                        } => {
                            // Convert NavigationFetchRequest to FetchRequest for HTTP transport.
                            let fetch_req = FetchRequest {
                                handler_id: DocumentFetchId::new(),
                                url: request.url,
                                method: request.method,
                                body: request.body.unwrap_or_default(),
                            };
                            let result = fetch_request(&client, &fetch_req);
                            if let Err(error) =
                                route_response(request_id, reply_to, result, &response_sender)
                            {
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

        Ok(())
    })
}

pub fn run_net_process_from_args() -> Result<(), String> {
    let token = net_token_from_args()?;
    run_net_process_v2(token.unwrap_or_default())
}

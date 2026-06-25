use ipc::run_extension;
use ipc_messages::content::{FetchRequest, FetchResponse};
use ipc_messages::network::{Request, Response};
use reqwest::Method;
use reqwest::blocking::Client;
use reqwest::header::CONTENT_TYPE;
use std::env;
use std::fs;
use std::net::{Ipv4Addr, SocketAddr};
use url::Url;
use verification::TraceSender;

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

pub fn run_net_process_v2(token: String) -> Result<(), String> {
    let client = Client::builder()
        .resolve("localhost", SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .build()
        .map_err(|error| format!("failed to build reqwest client: {error}"))?;

    ipc::run_extension::<Request, Response>(&token, move |server| {
        let mut _trace_sender: Option<TraceSender> = None;
        let response_sender = server.connection.sender.clone();
        let request_receiver = ipc::crossbeam_proxy(server.connection.receiver);
        // Note: the clone above would fail once IpcReceiver stops being Clone.
        // For now it works via the Clone impl. TODO: move receiver instead.

        loop {
            match request_receiver.recv() {
                Ok(incoming) => {
                    let request = incoming.payload;
                    match request {
                        Request::SetTraceSender(trace_sender) => {
                            _trace_sender = trace_sender;
                        }
                        Request::Fetch {
                            request_id,
                            request,
                        } => {
                            let result = fetch_request(&client, &request);
                            if let Err(error) = response_sender.send(Response { request_id, result }) {
                                log::error!("failed to send fetch response: {error}");
                                break;
                            }
                        }
                        Request::SetContentSender { .. } => {
                            log::info!("net: received SetContentSender (direct response channel)");
                            // TODO: store sender and use for direct responses
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
    // If a token was provided (ipc-channel mode), use it.
    // Otherwise, use the native XPC backend (process launched by launchd).
    run_net_process_v2(token.unwrap_or_default())
}

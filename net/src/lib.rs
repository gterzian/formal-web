use ipc_channel::ipc::{self, IpcSender};
use ipc_messages::content::{FetchRequest, FetchResponse};
use ipc_messages::network::{Bootstrap, Request, Response};
use reqwest::blocking::Client;
use reqwest::header::CONTENT_TYPE;
use reqwest::Method;
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

pub fn run_net_process(token: String) -> Result<(), String> {
    let (request_sender, request_receiver) =
        ipc::channel::<Request>().map_err(|error| error.to_string())?;
    let (response_sender, response_receiver) =
        ipc::channel::<Response>().map_err(|error| error.to_string())?;
    let bootstrap = IpcSender::<Bootstrap>::connect(token).map_err(|error| error.to_string())?;
    bootstrap
        .send(Bootstrap {
            request_sender,
            response_receiver,
        })
        .map_err(|error| error.to_string())?;

    let client = Client::builder()
        .resolve("localhost", SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .build()
        .map_err(|error| format!("failed to build reqwest client: {error}"))?;

    loop {
        match request_receiver.recv() {
            Ok(Request::Fetch { request_id, request }) => {
                let result = fetch_request(&client, &request);
                response_sender
                    .send(Response { request_id, result })
                    .map_err(|error| format!("failed to send fetch response: {error}"))?;
            }
            Ok(Request::Shutdown) => break,
            Err(_error) => break,
        }
    }

    Ok(())
}

pub fn run_net_process_from_args() -> Result<(), String> {
    let token = net_token_from_args()?.ok_or_else(|| String::from("missing --net-token argument"))?;
    run_net_process(token)
}

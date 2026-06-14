use ipc_channel::ipc::{self, IpcSender};
use ipc_messages::content::{FetchRequest, FetchResponse, HeaderList};
use ipc_messages::network::{Bootstrap, Request, Response};
use reqwest::Method;
use reqwest::blocking::Client;
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use std::env;
use std::fs;
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

fn content_type_from_header_list(header_list: &HeaderList) -> String {
    header_list
        .headers
        .iter()
        .find(|(name, _value)| name.eq_ignore_ascii_case(CONTENT_TYPE.as_str()))
        .map(|(_name, value)| value.clone())
        .unwrap_or_default()
}

// Note: Header values are currently carried as Strings for IPC compatibility.
// Non-UTF-8 header bytes are lossy-decoded until the transport can carry bytes.
fn header_list_from_header_map(headers: &HeaderMap) -> HeaderList {
    HeaderList {
        headers: headers
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_owned(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect(),
    }
}

fn header_map_from_header_list(header_list: &HeaderList) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    for (name, value) in &header_list.headers {
        let header_name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| format!("invalid request header name `{name}`: {error}"))?;
        let header_value = HeaderValue::from_bytes(value.as_bytes())
            .map_err(|error| format!("invalid request header value for `{name}`: {error}"))?;
        headers.append(header_name, header_value);
    }
    Ok(headers)
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
    // Note: The Fetch Standard leaves `file:` fetching implementation-defined. Formal-web maps a
    // successful local file read to a 200 OK response with the request URL as the one-item URL list
    // and a synthesized Content-Type header inferred from the path.
    let header_list = HeaderList {
        headers: vec![(String::from("content-type"), content_type.clone())],
    };
    Ok(FetchResponse {
        final_url: url.to_owned(),
        url_list: vec![url.to_owned()],
        status: 200,
        status_text: String::from("OK"),
        header_list,
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
        let content_type = String::from("text/html; charset=utf-8");
        let header_list = HeaderList {
            headers: vec![(String::from("content-type"), content_type.clone())],
        };
        return Ok(FetchResponse {
            final_url: String::from("about:blank"),
            url_list: vec![String::from("about:blank")],
            status: 200,
            status_text: String::from("OK"),
            header_list,
            content_type,
            body: Vec::new(),
        });
    }

    let method = Method::from_bytes(request.method.as_bytes())
        .map_err(|error| format!("invalid HTTP method: {error}"))?;
    let mut builder = client.request(method, parsed);
    let headers = header_map_from_header_list(&request.header_list)?;
    if !headers.is_empty() {
        builder = builder.headers(headers);
    }
    if !request.body.is_empty() {
        builder = builder.body(request.body.clone());
    }

    let response = builder
        .send()
        .map_err(|error| format!("network request failed: {error}"))?;
    let final_url = response.url().to_string();
    let status_code = response.status();
    let status = status_code.as_u16();
    // Note: reqwest exposes the status code and canonical reason phrase, not the HTTP/1 wire
    // reason phrase. HTTP/2 responses do not carry a reason phrase; the empty string remains the
    // fallback when reqwest has no canonical text for the status.
    let status_text = status_code.canonical_reason().unwrap_or("").to_owned();
    let header_list = header_list_from_header_map(response.headers());
    let content_type = content_type_from_header_list(&header_list);
    let body = response
        .bytes()
        .map_err(|error| format!("failed to read response body: {error}"))?
        .to_vec();

    Ok(FetchResponse {
        final_url: final_url.clone(),
        // Note: reqwest follows redirects before returning the response exposed here. Redirect
        // URL-list fidelity is left to future redirect handling work.
        url_list: vec![final_url.clone()],
        status,
        status_text,
        header_list,
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

    let mut _trace_sender: Option<TraceSender> = None;

    let client = Client::builder()
        .build()
        .map_err(|error| format!("failed to build reqwest client: {error}"))?;

    loop {
        match request_receiver.recv() {
            Ok(Request::SetTraceSender(trace_sender)) => {
                _trace_sender = trace_sender;
            }
            Ok(Request::Fetch {
                request_id,
                request,
            }) => {
                let result = fetch_request(&client, &request);
                response_sender
                    .send(Response { request_id, result })
                    .map_err(|error| format!("failed to send fetch response: {error}"))?;
            }
            Ok(Request::Shutdown) => break,
            Err(error) => {
                eprintln!("net request channel closed: {error}");
                break;
            }
        }
    }

    Ok(())
}

pub fn run_net_process_from_args() -> Result<(), String> {
    let token =
        net_token_from_args()?.ok_or_else(|| String::from("missing --net-token argument"))?;
    run_net_process(token)
}

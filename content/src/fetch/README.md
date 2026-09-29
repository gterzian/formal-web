# content/src/fetch

## Guidance

- A `fetch()` call is queued on the realm's `GlobalScope` (`PendingFetch`)
  and handed to the net process once the running task completes
  (`ContentProcess::dispatch_pending_fetches`), so domain code never needs
  the content process.  The response comes back as a
  `PendingNetworkHandler::Fetch` completion, which settles the promise
  through `process_response` and then runs a microtask checkpoint.
- Bodies are byte sequences held in memory (`Body::source`).  The `body`
  attribute creates the stream on first access from those bytes and keeps
  the stream so repeated reads return the same object; consuming a body
  locks and disturbs that stream when one exists.
- Header names and values are Web IDL `ByteString`s: bindings convert them
  with `convert_js_to_byte_string`, never as USVStrings.

## Remaining work

- Foundation reports no reason phrase, so on the URLSession backend the
  status message is the reason phrase of the status code (`reason_phrase`
  in `net/src/backend/mod.rs`).
- NSURLSession adds `Content-Type: application/x-www-form-urlencoded` to a
  request body sent without a content type, and `Content-Length: 0` to a
  body-less request with a custom method; `fetch/api/basic/request-headers`
  records those subtests as failing on macOS.
- The request body crosses IPC as a `String` (`FetchRequest.body`): a body
  that is not UTF-8 is sent lossily.
- Unsupported: `FormData` (and `formData()`), `ReadableStream` bodies,
  request modes other than what the net process does (every response has
  type "basic", redirects are followed by the backend, no CORS checks),
  aborting an in-flight network request (the promise rejects but the
  request completes in net), `fetch()` on worker global scopes (only
  `Window` exposes it: workers have no dispatch step).
- The `Headers` guard "request-no-cors" filters headers, but the fetch
  itself ignores the request's mode.
- `fetch/api/basic/response-null-body.any.js`, subtest "status=304
  (method=POST)", rejected once with "Failed to fetch" on the URLSession
  backend and passed on the next two runs; the cause was not investigated.

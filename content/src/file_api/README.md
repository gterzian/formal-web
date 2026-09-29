# content/src/file_api

## Remaining work

- Blob URLs are minted and revoked (`URL.createObjectURL`,
  `URL.revokeObjectURL`) but nothing resolves them: the store is the realm's
  `GlobalScope`, and `fetch`, `<img src>`, `Worker` and navigation do not
  consult it.
- `Blob.stream()` enqueues the whole byte sequence and closes the stream
  before returning it; `text()`, `arrayBuffer()` and `bytes()` resolve from
  the in-memory bytes without the stream read the spec performs.
- `Blob` and `File` are not `[Serializable]`: structured cloning them throws
  a "DataCloneError" DOMException.
- `FileReader`, `FileList` and `Blob` parts from `SharedArrayBuffer`-backed
  views are not implemented.

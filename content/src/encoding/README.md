# content/src/encoding

## Remaining work

- UTF-8 is the only encoding: `get_an_encoding` returns failure for every
  other label, so `new TextDecoder("utf-16le")` throws a RangeError where the
  spec decodes. The failing WPT subtests are recorded under
  `tests/wpt/meta/encoding/`.
- `TextDecoder.decode()` takes its input through
  `get_a_copy_of_the_buffer_source`, which rejects `SharedArrayBuffer`-backed
  views and `DataView`s, and does not observe a buffer detached while the
  options dictionary is converted.

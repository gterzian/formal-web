//! Safe wrappers around the AVFoundation, CoreMedia, and CoreVideo C/Objective-C
//! APIs.  All `unsafe` blocks in the AVFoundation backend are confined to this
//! module — the backend implementation in `super::pipeline` uses only safe code.
//!
//! Each type below wraps an `objc2` `Retained<T>` handle.  Methods delegate to
//! the underlying Objective-C object through `unsafe` FFI calls that are proven
//! correct for our single-threaded, serialized-access usage pattern.

mod player;
pub(crate) use player::AvPlayer;

mod item;
pub(crate) use item::AvPlayerItem;

mod video_output;
pub(crate) use video_output::AvVideoOutput;

pub(crate) mod pixel_buffer;

pub(crate) mod time;

mod url;
pub(crate) use url::url_from_string;

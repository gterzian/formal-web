//! File API (<https://w3c.github.io/FileAPI/>): the Blob and File interfaces
//! and the blob URL store.

pub(crate) mod blob;
pub(crate) mod blob_url_store;
pub(crate) mod file;

pub(crate) use blob::{Blob, BlobPart, BlobPropertyBag, EndingType};
pub(crate) use blob_url_store::{create_object_url, revoke_object_url};
pub(crate) use file::{File, FilePropertyBag};

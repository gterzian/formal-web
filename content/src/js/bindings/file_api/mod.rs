//! Bindings for the File API (<https://w3c.github.io/FileAPI/>): argument
//! conversion to the IDL types of `crate::file_api`, then the domain call.

pub(crate) mod blob;
mod file;

pub(crate) use blob::blob_from_value;

use std::time::{SystemTime, UNIX_EPOCH};

use js_engine::gc_struct;

use super::blob::{Blob, BlobPart, BlobPropertyBag};

/// <https://w3c.github.io/FileAPI/#dfn-FilePropertyBag>
pub(crate) struct FilePropertyBag {
    pub(crate) blob: BlobPropertyBag,
    /// <https://w3c.github.io/FileAPI/#dfn-FilePropertyBag-lastModified>
    pub(crate) last_modified: Option<i64>,
}

/// <https://w3c.github.io/FileAPI/#dfn-file>
#[gc_struct]
pub(crate) struct File {
    pub(crate) blob: Blob,

    /// <https://w3c.github.io/FileAPI/#dfn-name>
    #[ignore_trace]
    name: String,

    /// <https://w3c.github.io/FileAPI/#dfn-lastModified>
    #[ignore_trace]
    last_modified: i64,
}

impl File {
    /// <https://w3c.github.io/FileAPI/#file-constructor>
    pub(crate) fn constructor(
        file_bits: Vec<BlobPart>,
        file_name: String,
        options: FilePropertyBag,
    ) -> Self {
        // Step 1: Let bytes be the result of processing blob parts given
        // fileBits and options.
        // Step 3: Process FilePropertyBag dictionary argument by running the
        // following substeps:
        // Step 3.1: If the type member is provided and is not the empty
        // string, let t be set to the type dictionary member. If t contains
        // any characters outside the range U+0020 to U+007E, then set t to
        // the empty string and return from these substeps.
        // Step 3.2: Convert every character in t to ASCII lowercase.
        // Note: The Blob constructor runs steps 1, 3.1 and 3.2 over the same
        // members.
        let blob = Blob::constructor(Some(file_bits), options.blob);

        // Step 2: Let n be the fileName argument to the constructor.
        let name = file_name;

        // Step 3.3: If the lastModified member is provided, let d be set to
        // the lastModified dictionary member. If it is not provided, set d to
        // the current date and time represented as the number of
        // milliseconds since the Unix Epoch (which is the equivalent of
        // Date.now() [ECMA-262]).
        let last_modified = options.last_modified.unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis() as i64)
                .unwrap_or(0)
        });

        // Step 4: Return a new File object F such that:
        // Step 4.2: F refers to the bytes byte sequence.
        // Step 4.3: F.size is set to the number of total bytes in bytes.
        // Step 4.4: F.name is set to n.
        // Step 4.5: F.type is set to t.
        // Step 4.6: F.lastModified is set to d.
        Self {
            blob,
            name,
            last_modified,
        }
    }

    /// <https://w3c.github.io/FileAPI/#dfn-name>
    pub(crate) fn name(&self) -> String {
        // The name attribute must return the name of the file as a string.
        self.name.clone()
    }

    /// <https://w3c.github.io/FileAPI/#dfn-lastModified>
    pub(crate) fn last_modified(&self) -> i64 {
        // The lastModified attribute must return the value it was initialized
        // to when the object was created.
        self.last_modified
    }
}

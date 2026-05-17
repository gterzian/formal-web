/// allocator state for the browser-global identifiers owned by the user-agent.
///
/// UUID-backed model identifiers are allocated at the use site from shared newtypes in
/// `ipc_messages`; this struct keeps only the Rust-local handle counter.
#[derive(Clone, Debug)]
pub struct UserAgentIds {
    /// identifier for the content worker handle owned by the user agent.
    pub next_handle: usize,
}

impl Default for UserAgentIds {
    /// seeding the user-agent's browser-global identifier allocators.
    fn default() -> Self {
        Self {
            next_handle: 1,
        }
    }
}

impl UserAgentIds {
    /// allocating Rust handles that own content event loops.
    pub fn allocate_handle(&mut self) -> usize {
        let handle = self.next_handle;
        self.next_handle += 1;
        handle
    }
}
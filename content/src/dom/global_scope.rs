
/// <https://html.spec.whatwg.org/#global-object>
#[derive(Debug, Clone, Copy, Default)]
pub enum GlobalScopeKind {
    #[default]
    Window,
}

/// <https://html.spec.whatwg.org/#global-object>
#[derive(Debug, Clone, Copy, Default)]
pub struct GlobalScope {
    /// <https://html.spec.whatwg.org/#global-object>
    pub kind: GlobalScopeKind,
}
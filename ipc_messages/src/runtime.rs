use serde::{Deserialize, Serialize};

use crate::content::{BeforeUnloadResult, WebviewId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuntimeRequest {
    FreshTopLevelTraversable { destination_url: String },
    DispatchEventFor { traversable_id: u64, event: String },
    RenderingOpportunityFor { traversable_id: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EmbedderEvent {
    NewTopLevelTraversable {
        webview_id: WebviewId,
        target_name: String,
    },
    NavigationRequested {
        webview_id: WebviewId,
        destination_url: String,
    },
    BeforeUnloadCompleted(BeforeUnloadResult),
    FinalizeNavigation {
        webview_id: WebviewId,
        url: String,
    },
}
use ipc::IpcSender;
use ipc_messages::content::{Event as ContentEvent, NavigableId};
use js_engine::JsTypes;
use url::Url;

use crate::html::GlobalScope;
use crate::js::{Engine, Types};

/// <https://dom.spec.whatwg.org/#eventtarget-activation-behavior>
pub(crate) trait ActivationBehavior {
    #[allow(clippy::too_many_arguments)]
    fn activation_behavior(
        &self,
        source_navigable_id: NavigableId,
        parent_navigable_id: Option<NavigableId>,
        top_level_navigable_id: NavigableId,
        document_creation_url: &Url,
        event: &<Types as JsTypes>::JsObject,
        event_sender: &IpcSender<ContentEvent>,
        global_scope: Option<&GlobalScope>,
        window_global: Option<<Types as JsTypes>::JsObject>,
        parent_engine: Option<&mut Engine>,
    ) -> Result<(), String>;
}

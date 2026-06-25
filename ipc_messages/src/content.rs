use crate::media::VideoEmbedData;
use anyrender::{
    Scene,
    recording::{GlyphRunCommand, RenderCommand},
};

use peniko::FontData;
use serde::{Deserialize, Serialize};
use ipc::IpcSender;
use std::collections::{HashMap, HashSet, hash_map::Entry};
use std::fmt;
use uuid::Uuid;
use verification::TraceSender;

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            pub fn parse_str(value: &str) -> Result<Self, uuid::Error> {
                Uuid::parse_str(value).map(Self)
            }

            pub fn from_u128(value: u128) -> Self {
                Self(Uuid::from_u128(value))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

uuid_id!(DocumentFetchId);
uuid_id!(NavigationFetchId);
uuid_id!(WindowTimerKey);
uuid_id!(EventLoopId);
uuid_id!(BrowsingContextId);
uuid_id!(BrowsingContextGroupId);
uuid_id!(DocumentId);
uuid_id!(AgentClusterId);
uuid_id!(AgentId);
uuid_id!(BeforeUnloadCheckId);
uuid_id!(NavigableId);
uuid_id!(FrameId);
uuid_id!(NavigationId);

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ColorScheme {
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewportSnapshot {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub color_scheme: ColorScheme,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversableViewport {
    pub traversable_id: NavigableId,
    pub viewport: ViewportSnapshot,
    pub offset_x: f32,
    pub offset_y: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchRequest {
    pub handler_id: DocumentFetchId,
    pub url: String,
    pub method: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadedDocumentResponse {
    pub final_url: String,
    pub status: u16,
    pub content_type: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchResponse {
    pub final_url: String,
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UserNavigationInvolvement {
    None,
    Activation,
    BrowserUi,
}

/// Information for creating a new child navigable (e.g. iframe).
/// Carried as part of a `NavigateRequest` to fold creation and navigation
/// into a single IPC message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewChildNavigableInfo {
    pub parent_traversable_id: NavigableId,
    pub content_navigable_id: NavigableId,
    pub content_frame_id: FrameId,
    /// Document ID for the document that the content process created locally.
    /// No `CreateEmptyDocument` should be sent for this document.
    pub document_id: DocumentId,
    #[serde(default)]
    pub target_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavigateRequest {
    #[serde(default)]
    pub navigation_id: Option<NavigationId>,
    pub source_navigable_id: NavigableId,
    #[serde(default)]
    pub chosen_navigable_id: Option<NavigableId>,
    pub destination_url: String,
    pub target: String,
    pub user_involvement: UserNavigationInvolvement,
    pub noopener: bool,
    /// Referrer policy override for the navigation. `None` means the default referrer
    /// policy from the source document is used.
    pub referrer_policy: Option<String>,
    /// JSON-serialized tokenized features from `window.open()`. `None` means the request
    /// did not originate from `window.open`. Carried to the user agent for popup detection
    /// and browsing context feature setup.
    pub features_json: Option<String>,
    /// Information about a new top-level traversable that the content process created
    /// locally as part of the "rules for choosing a navigable". When `Some`, the user
    /// agent must set up its navigable, browsing context, and document state without
    /// sending `CreateEmptyDocument` (content already created the document).
    #[serde(default)]
    pub new_traversable_info: Option<NewTraversableInfo>,
    /// Information about a new child navigable that the content process created
    /// locally. When `Some`, the user agent must set up its navigable, browsing
    /// context group membership, and document state. The destination_url carries
    /// the initial navigation URL for the child (about:blank if no navigation is
    /// needed).
    #[serde(default)]
    pub new_child_navigable: Option<NewChildNavigableInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeforeUnloadResult {
    pub document_id: DocumentId,
    pub check_id: BeforeUnloadCheckId,
    pub canceled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewTraversableInfo {
    pub document_id: DocumentId,
    pub target_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalizeNavigation {
    pub document_id: DocumentId,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IframeTraversableRemoval {
    pub parent_traversable_id: NavigableId,
    pub content_navigable_id: NavigableId,
    pub content_frame_id: FrameId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WebviewId(pub NavigableId);

pub fn iframe_target_name(
    parent_traversable_id: NavigableId,
    content_navigable_id: NavigableId,
    content_frame_id: FrameId,
) -> String {
    format!("_iframe|{parent_traversable_id}|{content_navigable_id}|{content_frame_id}")
}

pub fn parse_iframe_target_name(target_name: &str) -> Option<(NavigableId, NavigableId, FrameId)> {
    let payload = target_name.strip_prefix("_iframe|")?;
    let mut parts = payload.split('|');
    let parent_traversable_id = NavigableId::parse_str(parts.next()?).ok()?;
    let content_navigable_id = NavigableId::parse_str(parts.next()?).ok()?;
    let content_frame_id = FrameId::parse_str(parts.next()?).ok()?;
    if parts.next().is_some() {
        return None;
    }

    Some((
        parent_traversable_id,
        content_navigable_id,
        content_frame_id,
    ))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptEvaluationResult {
    pub request_id: u64,
    pub value_json: String,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementClickResult {
    pub request_id: u64,
    #[serde(default)]
    pub error: Option<String>,
}

/// Fire-and-forget clipboard write request.
/// No reply expected — the embedder writes to the system clipboard
/// and does not need to acknowledge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardWriteRequested {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchEventEntry {
    pub document_id: DocumentId,
    pub event: String,
    /// Prefetched clipboard text attached by the embedder when it detects
    /// a paste shortcut before forwarding the event to content.
    /// Content stores this in a local cache so `ShellProvider::get_clipboard_text`
    /// can return it without a blocking IPC round-trip.
    #[serde(default)]
    pub prefetched_clipboard_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// <https://html.spec.whatwg.org/#loading-the-media-resource>
pub struct MediaLoadRequest {
    /// The URL of the media resource to load.
    pub url: String,
    /// The document requesting the load.
    pub document_id: DocumentId,
    /// The traversable containing the media element.
    pub traversable_id: NavigableId,
    /// Paint-layer identifier assigned by content for the video element.
    /// Echoed in EmbedSite::Video so the compositor can route frames.
    pub video_paint_id: crate::media::VideoPaintId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowTimerRequest {
    pub document_id: DocumentId,
    pub timer_id: u32,
    pub timer_key: WindowTimerKey,
    pub timeout_ms: u32,
    pub nesting_level: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowTimerClearRequest {
    pub document_id: DocumentId,
    pub timer_key: WindowTimerKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EmbedSiteId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmbedBackgroundPolicy {
    Transparent,
    OpaqueWhite,
}

/// Layout properties shared by all embedded content (iframes, video, etc.).
/// Used by the compositor to sort and position embed sites in the parent frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedLayout {
    pub transform: [f64; 6],
    pub clip_bounds: [f64; 4],
    pub z_index: i32,
    pub paint_order: u32,
}

/// An iframe embed site, identified by its child frame id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IframeEmbedSite {
    pub embed_site_id: EmbedSiteId,
    pub child_frame_id: FrameId,
    pub background_policy: EmbedBackgroundPolicy,
    pub clip_svg_path: String,
    pub layout: EmbedLayout,
}

/// A single embed site within a parent document's composition.
/// Both iframes and video are [embedded content](https://html.spec.whatwg.org/#embedded-content)
/// and share the same z-order / paint-order space.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EmbedSite {
    Frame(IframeEmbedSite),
    Video(VideoEmbedData),
}

impl EmbedSite {
    pub fn layout(&self) -> &EmbedLayout {
        match self {
            EmbedSite::Frame(s) => &s.layout,
            EmbedSite::Video(s) => &s.layout,
        }
    }

    pub fn z_index(&self) -> i32 {
        self.layout().z_index
    }

    pub fn paint_order(&self) -> u32 {
        self.layout().paint_order
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FrameCompositionMetadata {
    /// All embedded content (iframes, video) sorted by the content process
    /// in document order. The compositor sorts by z_index / paint_order.
    pub embed_sites: Vec<EmbedSite>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FontIdentifier {
    pub namespace: u64,
    pub blob_id: u64,
    pub index: u32,
}

impl FontIdentifier {
    pub fn from_font_data(namespace: u64, font_data: &FontData) -> Self {
        Self {
            namespace,
            blob_id: font_data.data.id(),
            index: font_data.index,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredFont {
    pub id: FontIdentifier,
    /// Key into the IPC shared memory map for this font's binary data.
    /// Set by the sender before serialization; the receiver looks up the
    /// font bytes from the shmem map using this key.
    pub data_shmem_key: usize,
}

impl RegisteredFont {
    fn from_font_data(id: FontIdentifier, font_data: &FontData, key: usize) -> (Self, Vec<u8>) {
        (
            Self {
                id,
                data_shmem_key: key,
            },
            font_data.data.data().to_vec(),
        )
    }

    fn into_font_data_from_bytes(self, data: Vec<u8>) -> FontData {
        FontData::new(data.into(), self.id.index)
    }
}

#[derive(Debug, Clone)]
pub struct PreparedScene {
    pub scene: RecordedScene,
    pub registered_fonts: Vec<RegisteredFont>,
    /// Font binary data indexed by each font's `data_shmem_key`.
    /// The caller must place this data into the IPC shared memory map
    /// before sending the `PaintFrame` message.
    pub font_shmem: HashMap<usize, ipc::IpcSharedRegion>,
}

#[derive(Debug, Default)]
pub struct FontTransportSender {
    sent_fonts: HashSet<FontIdentifier>,
}

impl FontTransportSender {
    /// Assign shmem keys starting from `next_key` and return the font data
    /// alongside the prepared scene.  The caller places the returned font data
    /// into the IPC shared memory map under each font's `data_shmem_key`.
    pub fn prepare_scene(
        &mut self,
        font_namespace: u64,
        scene: Scene,
        next_shmem_key: &mut usize,
    ) -> PreparedScene {
        let mut font_ids = Vec::new();
        let mut scene_font_ids = HashMap::new();
        let mut registered_fonts = Vec::new();
        let mut font_shmem: HashMap<usize, ipc::IpcSharedRegion> = HashMap::new();
        let commands = scene
            .commands
            .into_iter()
            .map(|command| {
                SerializableRenderCommand::from_render_command(command, |font_data_ref| {
                    let font_id = FontIdentifier::from_font_data(font_namespace, font_data_ref);
                    let next_scene_font_id = font_ids.len() as u32;
                    let scene_font_id = match scene_font_ids.entry(font_id) {
                        Entry::Occupied(entry) => *entry.get(),
                        Entry::Vacant(entry) => {
                            font_ids.push(font_id);
                            entry.insert(next_scene_font_id);
                            next_scene_font_id
                        }
                    };

                    if self.sent_fonts.insert(font_id) {
                        let (font, raw_data) =
                            RegisteredFont::from_font_data(font_id, font_data_ref, *next_shmem_key);
                        *next_shmem_key += 1;
                        font_shmem.insert(
                            font.data_shmem_key,
                            ipc::IpcSharedRegion::from_bytes(&raw_data),
                        );
                        registered_fonts.push(font);
                    }

                    scene_font_id
                })
            })
            .collect();

        PreparedScene {
            scene: RecordedScene {
                tolerance: scene.tolerance,
                font_ids,
                commands,
            },
            registered_fonts,
            font_shmem,
        }
    }
}

#[derive(Debug, Default)]
pub struct FontTransportReceiver {
    fonts: HashMap<FontIdentifier, FontData>,
}

impl FontTransportReceiver {
    /// Register fonts, reading each font's binary data from the IPC shared
    /// memory map indexed by `data_shmem_key`.
    pub fn register_fonts(
        &mut self,
        registered_fonts: Vec<RegisteredFont>,
        font_data: &HashMap<usize, Vec<u8>>,
    ) {
        for font in registered_fonts {
            if let Entry::Vacant(entry) = self.fonts.entry(font.id) {
                if let Some(data) = font_data.get(&font.data_shmem_key) {
                    entry.insert(font.into_font_data_from_bytes(data.clone()));
                }
            }
        }
    }

    pub fn resolve_font(&self, font_ids: &[FontIdentifier], scene_font_id: u32) -> FontData {
        let font_id = font_ids
            .get(scene_font_id as usize)
            .copied()
            .unwrap_or_else(|| {
                debug_assert!(false, "recorded scene referenced missing scene font id");
                FontIdentifier::default()
            });

        self.fonts.get(&font_id).cloned().unwrap_or_else(|| {
            debug_assert!(false, "recorded scene referenced an unregistered font");
            FontData::new(Vec::<u8>::new().into(), font_id.index)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SerializableRenderCommand(pub RenderCommand<u32>);

impl SerializableRenderCommand {
    fn from_render_command(
        command: RenderCommand,
        mut scene_font_id: impl FnMut(&FontData) -> u32,
    ) -> Self {
        match command {
            RenderCommand::PushLayer(command) => Self(RenderCommand::PushLayer(command)),
            RenderCommand::PushClipLayer(command) => Self(RenderCommand::PushClipLayer(command)),
            RenderCommand::PopLayer => Self(RenderCommand::PopLayer),
            RenderCommand::Stroke(command) => Self(RenderCommand::Stroke(command)),
            RenderCommand::Fill(command) => Self(RenderCommand::Fill(command)),
            RenderCommand::GlyphRun(command) => {
                let font_data = scene_font_id(&command.font_data);
                Self(RenderCommand::GlyphRun(GlyphRunCommand {
                    font_data,
                    font_size: command.font_size,
                    hint: command.hint,
                    normalized_coords: command.normalized_coords,
                    embolden: command.embolden,
                    style: command.style,
                    brush: command.brush,
                    brush_alpha: command.brush_alpha,
                    transform: command.transform,
                    glyph_transform: command.glyph_transform,
                    glyphs: command.glyphs,
                }))
            }
            RenderCommand::BoxShadow(command) => Self(RenderCommand::BoxShadow(command)),
        }
    }

    fn into_render_command(self, mut font_data: impl FnMut(u32) -> FontData) -> RenderCommand {
        match self.0 {
            RenderCommand::PushLayer(command) => RenderCommand::PushLayer(command),
            RenderCommand::PushClipLayer(command) => RenderCommand::PushClipLayer(command),
            RenderCommand::PopLayer => RenderCommand::PopLayer,
            RenderCommand::Stroke(command) => RenderCommand::Stroke(command),
            RenderCommand::Fill(command) => RenderCommand::Fill(command),
            RenderCommand::GlyphRun(command) => RenderCommand::GlyphRun(GlyphRunCommand {
                font_data: font_data(command.font_data),
                font_size: command.font_size,
                hint: command.hint,
                normalized_coords: command.normalized_coords,
                embolden: command.embolden,
                style: command.style,
                brush: command.brush,
                brush_alpha: command.brush_alpha,
                transform: command.transform,
                glyph_transform: command.glyph_transform,
                glyphs: command.glyphs,
            }),
            RenderCommand::BoxShadow(command) => RenderCommand::BoxShadow(command),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordedScene {
    pub tolerance: f64,
    pub font_ids: Vec<FontIdentifier>,
    pub commands: Vec<SerializableRenderCommand>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SceneSummary {
    pub commands: usize,
    pub glyph_runs: usize,
    pub glyphs: usize,
    pub font_refs: usize,
    pub fill_commands: usize,
    pub stroke_commands: usize,
    pub push_layer_commands: usize,
    pub push_clip_layer_commands: usize,
    pub pop_layer_commands: usize,
    pub box_shadow_commands: usize,
    pub solid_glyph_brush_runs: usize,
    pub gradient_glyph_brush_runs: usize,
    pub image_glyph_brush_runs: usize,
}

impl SceneSummary {
    pub fn describe(&self) -> String {
        format!(
            "commands={} glyph_runs={} glyphs={} font_refs={} fills={} strokes={} push_layers={} push_clip_layers={} pops={} box_shadows={} glyph_brushes(solid={}, gradient={}, image={})",
            self.commands,
            self.glyph_runs,
            self.glyphs,
            self.font_refs,
            self.fill_commands,
            self.stroke_commands,
            self.push_layer_commands,
            self.push_clip_layer_commands,
            self.pop_layer_commands,
            self.box_shadow_commands,
            self.solid_glyph_brush_runs,
            self.gradient_glyph_brush_runs,
            self.image_glyph_brush_runs,
        )
    }
}

impl RecordedScene {
    pub fn summary(&self) -> SceneSummary {
        let mut summary = SceneSummary {
            commands: self.commands.len(),
            font_refs: self.font_ids.len(),
            ..SceneSummary::default()
        };

        for command in &self.commands {
            match &command.0 {
                RenderCommand::PushLayer(_) => summary.push_layer_commands += 1,
                RenderCommand::PushClipLayer(_) => summary.push_clip_layer_commands += 1,
                RenderCommand::PopLayer => summary.pop_layer_commands += 1,
                RenderCommand::Stroke(_) => summary.stroke_commands += 1,
                RenderCommand::Fill(_) => summary.fill_commands += 1,
                RenderCommand::GlyphRun(command) => {
                    summary.glyph_runs += 1;
                    summary.glyphs += command.glyphs.len();
                    use anyrender::Paint;
                    match &command.brush {
                        Paint::Solid(_) => summary.solid_glyph_brush_runs += 1,
                        Paint::Gradient(_) => summary.gradient_glyph_brush_runs += 1,
                        Paint::Image(_) => summary.image_glyph_brush_runs += 1,
                        Paint::Resource(_) | Paint::Custom(_) => {}
                    }
                }
                RenderCommand::BoxShadow(_) => summary.box_shadow_commands += 1,
            }
        }

        summary
    }

    pub fn into_scene(self, font_receiver: &FontTransportReceiver) -> Scene {
        let RecordedScene {
            tolerance,
            font_ids,
            commands,
        } = self;

        Scene {
            tolerance,
            commands: commands
                .into_iter()
                .map(|command| {
                    command.into_render_command(|scene_font_id| {
                        font_receiver.resolve_font(&font_ids, scene_font_id)
                    })
                })
                .collect(),
        }
    }
}

fn serialize_scene_to_vec(scene: &RecordedScene) -> Result<Vec<u8>, String> {
    postcard::to_allocvec(scene)
        .map_err(|error| format!("failed to serialize paint scene: {error}"))
}

fn deserialize_scene_from_slice(scene_bytes: &[u8]) -> Result<RecordedScene, String> {
    postcard::from_bytes(scene_bytes)
        .map_err(|error| format!("failed to deserialize paint scene: {error}"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaintFrame {
    pub traversable_id: WebviewId,
    pub frame_id: FrameId,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub composition: FrameCompositionMetadata,
    font_registrations: Vec<RegisteredFont>,
    /// Key into the IPC shared memory map.  The sender serialized the paint
    /// scene into a byte buffer and placed it under this key; the receiver
    /// reads the bytes from the shmem map using this key and passes them to
    /// `into_recorded_scene()`.
    pub scene_shmem_key: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PaintTransportSummary {
    pub scene_bytes: usize,
    pub registered_fonts: usize,
}

impl PaintFrame {
    /// Create a `PaintFrame` and return it together with the serialised scene
    /// bytes and font data that the caller must place into the IPC shared
    /// memory map.
    ///
    /// `next_shmem_key` is used to assign unique keys for the scene bytes and
    /// each font's data.  On return it is advanced past all assigned keys.
    pub fn new(
        traversable_id: WebviewId,
        frame_id: FrameId,
        viewport_width: u32,
        viewport_height: u32,
        composition: FrameCompositionMetadata,
        scene: PreparedScene,
        next_shmem_key: &mut usize,
    ) -> Result<(Self, HashMap<usize, ipc::IpcSharedRegion>), String> {
        let PreparedScene {
            scene,
            registered_fonts,
            font_shmem,
        } = scene;

        let mut shmem_map: HashMap<usize, ipc::IpcSharedRegion> = font_shmem;

        let scene_shmem_key = *next_shmem_key;
        *next_shmem_key += 1;
        let scene_bytes = serialize_scene_to_vec(&scene)?;
        let scene_region = ipc::IpcSharedRegion::from_bytes(&scene_bytes);
        shmem_map.insert(scene_shmem_key, scene_region);

        Ok((
            Self {
                traversable_id,
                frame_id,
                viewport_width,
                viewport_height,
                composition,
                font_registrations: registered_fonts,
                scene_shmem_key,
            },
            shmem_map,
        ))
    }

    pub fn transport_summary(
        &self,
        shmem_regions: &HashMap<usize, ipc::IpcSharedRegion>,
    ) -> PaintTransportSummary {
        let scene_bytes = shmem_regions
            .get(&self.scene_shmem_key)
            .map(|r| r.size())
            .unwrap_or(0);
        PaintTransportSummary {
            scene_bytes,
            registered_fonts: self.font_registrations.len(),
        }
    }

    /// Consume the paint frame into a `RecordedScene`.  The scene bytes and
    /// font data are read from the IPC shmem regions map, keyed by the
    /// values stored in this frame.  No copy is made beyond the final
    /// deserialization.
    pub fn into_recorded_scene(
        self,
        font_receiver: &mut FontTransportReceiver,
        shmem_regions: &HashMap<usize, ipc::IpcSharedRegion>,
    ) -> Result<RecordedScene, String> {
        let PaintFrame {
            font_registrations, ..
        } = self;

        // Reconstruct font data from the shmem regions for the receiver.
        let mut font_data: HashMap<usize, Vec<u8>> = HashMap::new();
        for font in &font_registrations {
            if let Some(region) = shmem_regions.get(&font.data_shmem_key) {
                font_data.insert(font.data_shmem_key, region.as_slice().to_vec());
            }
        }
        font_receiver.register_fonts(font_registrations, &font_data);

        let scene_bytes = shmem_regions
            .get(&self.scene_shmem_key)
            .map(|r| r.as_slice())
            .unwrap_or_default();
        deserialize_scene_from_slice(scene_bytes)
    }
}

#[derive(Debug, Clone)]
pub enum WebviewProviderMessage {
    /// A paint frame with its associated scene and font data reconstructed
    /// from the IPC shared memory map.  The webview calls
    /// `PaintFrame::into_recorded_scene()` with `scene_bytes` and
    /// `font_data`.
    PaintFrame {
        frame: PaintFrame,
        /// IPC shared memory regions. The caller passes this to
        /// `PaintFrame::into_recorded_scene()`, which reads scene bytes
        /// and font data from the regions keyed by `scene_shmem_key` and
        /// each font's `data_shmem_key`.
        shmem_regions: HashMap<usize, ipc::IpcSharedRegion>,
    },
    RegisterChildNavigableHost {
        child_webview_id: WebviewId,
        parent_traversable_id: WebviewId,
        content_frame_id: FrameId,
    },
    NewWebview {
        webview_id: WebviewId,
    },
    /// A decoded video frame from the media process, ready for the compositor.
    /// Carries the owning webview id, the paint id for compositor lookup,
    /// and the RGBA8 pixel data as shared memory.
    VideoFrameReady {
        webview_id: WebviewId,
        paint_id: crate::media::VideoPaintId,
        /// RGBA8 pixel data as shared memory.
        data: crate::media::VideoFrame,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    SetTraceSender(Option<TraceSender>),
    SetEventLoopId(EventLoopId),
    SetViewport(ViewportSnapshot),
    SetTraversableViewport(TraversableViewport),
    CreateEmptyDocument {
        traversable_id: NavigableId,
        document_id: DocumentId,
        frame_id: Option<FrameId>,
        /// <https://html.spec.whatwg.org/multipage/#navigable>'s parent navigable id, if any.
        parent_traversable_id: Option<NavigableId>,
        /// The root of this navigable's traversable navigable chain.
        top_level_traversable_id: NavigableId,
    },
    CreateLoadedDocument {
        traversable_id: NavigableId,
        document_id: DocumentId,
        frame_id: Option<FrameId>,
        response: LoadedDocumentResponse,
        /// <https://html.spec.whatwg.org/multipage/#navigable>'s parent navigable id, if any.
        parent_traversable_id: Option<NavigableId>,
        /// The root of this navigable's traversable navigable chain.
        top_level_traversable_id: NavigableId,
    },
    DestroyDocument {
        document_id: DocumentId,
    },
    EvaluateScript {
        traversable_id: NavigableId,
        request_id: u64,
        source: String,
    },
    ClickElement {
        traversable_id: NavigableId,
        request_id: u64,
        selector: String,
    },
    DispatchEvent {
        events: Vec<DispatchEventEntry>,
    },
    RunBeforeUnload {
        document_id: DocumentId,
        check_id: BeforeUnloadCheckId,
        navigation_id: NavigationId,
    },
    UpdateTheRendering {
        traversable_id: NavigableId,
        document_id: DocumentId,
    },
    RunWindowTimer {
        document_id: DocumentId,
        timer_id: u32,
        timer_key: WindowTimerKey,
        nesting_level: u32,
    },
    CompleteDocumentFetch {
        handler_id: DocumentFetchId,
        response: FetchResponse,
    },
    FailDocumentFetch {
        handler_id: DocumentFetchId,
    },
    /// Set up direct connections to net and media extensions.
    /// Sent as the first message after bootstrap so content never needs
    /// a fallback path for network requests.
    SetDirectChannels {
        net_sender: ipc::IpcSender<crate::network::Request>,
        media_sender: Option<ipc::IpcSender<crate::media::MediaCommand>>,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    DocumentFetchRequested(FetchRequest),
    WindowTimerRequested(WindowTimerRequest),
    WindowTimerCleared(WindowTimerClearRequest),
    NavigationRequested(NavigateRequest),
    BeforeUnloadCompleted(BeforeUnloadResult),
    FinalizeNavigation(FinalizeNavigation),
    IframeTraversableRemoved(IframeTraversableRemoval),
    ScriptEvaluated(ScriptEvaluationResult),
    ElementClicked(ElementClickResult),
    /// A request from content to write text to the system clipboard.
    /// This is fire-and-forget — no reply is sent.
    ClipboardWriteRequested(ClipboardWriteRequested),
    CommandCompleted,
    MediaLoadRequested(MediaLoadRequest),
    PaintReady(PaintFrame),
    /// Content registers its net response channel (content→net direction).
    /// The user agent forwards the sender to net so responses arrive directly.
    RegisterNetResponseChannel {
        sender: ipc::IpcSender<crate::network::Response>,
    },
    /// Content registers its media event channel.
    RegisterMediaEventChannel {
        sender: ipc::IpcSender<crate::media::MediaEvent>,
    },
    ShutdownCompleted,
}

#[cfg(test)]
mod tests {
    use super::{
        Command, DocumentFetchId, FetchResponse, FontTransportReceiver, FontTransportSender,
        FrameCompositionMetadata, FrameId, LoadedDocumentResponse, NavigableId, PaintFrame,
        PaintTransportSummary, SceneSummary, WebviewId,
    };
    use anyrender::{Glyph, PaintScene, Scene, recording::RenderCommand};
    use peniko::{Color, Fill, FontData, kurbo::Affine};

    fn scene_with_glyph(font: &FontData, glyph_id: u16, x: f32, y: f32) -> Scene {
        let mut scene = Scene::new();
        scene.draw_glyphs(
            font,
            16.0,
            true,
            &[],
            Fill::NonZero,
            Color::BLACK,
            1.0,
            Affine::IDENTITY,
            None,
            [Glyph {
                id: glyph_id.into(),
                x,
                y,
            }]
            .into_iter(),
        );
        scene
    }

    fn assert_single_glyph_run_font(scene: &Scene, expected_bytes: &[u8], expected_glyph_id: u16) {
        assert_eq!(scene.commands.len(), 1);
        match &scene.commands[0] {
            RenderCommand::GlyphRun(command) => {
                assert_eq!(command.font_data.data.data(), expected_bytes);
                assert_eq!(command.font_data.index, 0);
                assert_eq!(command.glyphs.len(), 1);
                assert_eq!(command.glyphs[0].id, expected_glyph_id.into());
            }
            other => panic!("expected glyph run, got {other:?}"),
        }
    }

    #[test]
    fn recorded_scene_round_trips_glyph_runs() {
        let font = FontData::new(vec![1_u8, 2, 3, 4].into(), 0);
        let scene = scene_with_glyph(&font, 7, 12.0, 18.0);
        let mut sender = FontTransportSender::default();
        let prepared = sender.prepare_scene(11, scene.clone());

        assert_eq!(prepared.scene.font_ids.len(), 1);
        assert_eq!(prepared.registered_fonts.len(), 1);
        let summary = prepared.scene.summary();
        assert_eq!(
            summary,
            SceneSummary {
                commands: 1,
                glyph_runs: 1,
                glyphs: 1,
                font_refs: 1,
                fill_commands: 0,
                stroke_commands: 0,
                push_layer_commands: 0,
                push_clip_layer_commands: 0,
                pop_layer_commands: 0,
                box_shadow_commands: 0,
                solid_glyph_brush_runs: 1,
                gradient_glyph_brush_runs: 0,
                image_glyph_brush_runs: 0,
            }
        );

        let mut receiver = FontTransportReceiver::default();
        receiver.register_fonts(prepared.registered_fonts);
        let roundtripped = prepared.scene.into_scene(&receiver);
        assert_single_glyph_run_font(&roundtripped, &[1, 2, 3, 4], 7);
    }

    #[test]
    fn recorded_scene_deduplicates_shared_font_payloads() {
        let font = FontData::new(vec![1_u8, 2, 3, 4].into(), 0);
        let mut scene = scene_with_glyph(&font, 7, 12.0, 18.0);
        scene.draw_glyphs(
            &font,
            16.0,
            true,
            &[],
            Fill::NonZero,
            Color::BLACK,
            1.0,
            Affine::IDENTITY,
            None,
            [Glyph {
                id: 8,
                x: 28.0,
                y: 18.0,
            }]
            .into_iter(),
        );

        let mut sender = FontTransportSender::default();
        let prepared = sender.prepare_scene(17, scene.clone());
        assert_eq!(prepared.scene.font_ids.len(), 1);
        assert_eq!(prepared.registered_fonts.len(), 1);
        assert_eq!(prepared.scene.summary().font_refs, 1);

        let mut receiver = FontTransportReceiver::default();
        receiver.register_fonts(prepared.registered_fonts);
        let roundtripped = prepared.scene.into_scene(&receiver);
        assert_eq!(roundtripped.commands.len(), 2);
    }

    #[test]
    fn paint_frame_round_trips_scene_through_shared_memory() {
        let font = FontData::new(vec![1_u8, 2, 3, 4].into(), 0);
        let scene = scene_with_glyph(&font, 7, 12.0, 18.0);
        let mut sender = FontTransportSender::default();
        let prepared = sender.prepare_scene(23, scene);
        let expected_recorded = prepared.scene.clone();
        let paint_frame = PaintFrame::new(
            WebviewId(NavigableId::from_u128(7)),
            FrameId::from_u128(7),
            320,
            240,
            FrameCompositionMetadata::default(),
            prepared,
        )
        .expect("paint frame should serialize into shared memory");

        let mut receiver = FontTransportReceiver::default();
        let roundtripped = paint_frame
            .into_recorded_scene(&mut receiver)
            .expect("paint frame should deserialize scene bytes");

        assert_eq!(roundtripped, expected_recorded);
    }

    #[test]
    fn paint_frame_omits_previously_registered_font_payloads() {
        let font = FontData::new(vec![1_u8, 2, 3, 4].into(), 0);
        let mut sender = FontTransportSender::default();
        let mut receiver = FontTransportReceiver::default();

        let first_frame = PaintFrame::new(
            WebviewId(NavigableId::from_u128(7)),
            FrameId::from_u128(7),
            320,
            240,
            FrameCompositionMetadata::default(),
            sender.prepare_scene(29, scene_with_glyph(&font, 7, 12.0, 18.0)),
        )
        .expect("first frame should serialize");
        let first_summary = first_frame.transport_summary();
        assert_eq!(
            first_summary,
            PaintTransportSummary {
                scene_bytes: first_summary.scene_bytes,
                registered_fonts: 1,
                registered_font_bytes: 4,
            }
        );

        let first_scene = first_frame
            .into_recorded_scene(&mut receiver)
            .expect("first frame should decode")
            .into_scene(&receiver);
        assert_single_glyph_run_font(&first_scene, &[1, 2, 3, 4], 7);

        let second_frame = PaintFrame::new(
            WebviewId(NavigableId::from_u128(7)),
            FrameId::from_u128(7),
            320,
            240,
            FrameCompositionMetadata::default(),
            sender.prepare_scene(29, scene_with_glyph(&font, 8, 28.0, 18.0)),
        )
        .expect("second frame should serialize");
        assert_eq!(
            second_frame.transport_summary(),
            PaintTransportSummary {
                scene_bytes: second_frame.transport_summary().scene_bytes,
                registered_fonts: 0,
                registered_font_bytes: 0,
            }
        );

        let second_scene = second_frame
            .into_recorded_scene(&mut receiver)
            .expect("second frame should decode")
            .into_scene(&receiver);
        assert_single_glyph_run_font(&second_scene, &[1, 2, 3, 4], 8);
    }

    #[test]
    fn create_loaded_document_command_round_trips_response_metadata() {
        let encoded = postcard::to_allocvec(&Command::CreateLoadedDocument {
            traversable_id: NavigableId::from_u128(3),
            document_id: 7,
            frame_id: None,
            response: LoadedDocumentResponse {
                final_url: String::from("https://example.test/final"),
                status: 201,
                content_type: String::from("text/html; charset=utf-8"),
                body: String::from("<p>ok</p>"),
            },
            parent_traversable_id: Some(NavigableId::from_u128(2)),
            top_level_traversable_id: NavigableId::from_u128(1),
        })
        .expect("create-loaded-document should serialize");
        let decoded: Command =
            postcard::from_bytes(&encoded).expect("create-loaded-document should deserialize");

        match decoded {
            Command::CreateLoadedDocument {
                traversable_id,
                document_id,
                frame_id,
                response,
                parent_traversable_id,
                top_level_traversable_id,
            } => {
                assert_eq!(traversable_id, NavigableId::from_u128(3));
                assert_eq!(document_id, 7);
                assert_eq!(frame_id, None);
                assert_eq!(parent_traversable_id, Some(NavigableId::from_u128(2)));
                assert_eq!(top_level_traversable_id, NavigableId::from_u128(1));
                assert_eq!(
                    response,
                    LoadedDocumentResponse {
                        final_url: String::from("https://example.test/final"),
                        status: 201,
                        content_type: String::from("text/html; charset=utf-8"),
                        body: String::from("<p>ok</p>"),
                    }
                );
            }
            other => panic!("expected CreateLoadedDocument, got {other:?}"),
        }
    }

    #[test]
    fn complete_document_fetch_command_round_trips_response_metadata() {
        let handler_id = DocumentFetchId::from_u128(19);
        let encoded = postcard::to_allocvec(&Command::CompleteDocumentFetch {
            handler_id,
            response: FetchResponse {
                final_url: String::from("https://example.test/script.js"),
                status: 404,
                content_type: String::from("text/html"),
                body: vec![1, 2, 3, 4],
            },
        })
        .expect("complete-document-fetch should serialize");
        let decoded: Command =
            postcard::from_bytes(&encoded).expect("complete-document-fetch should deserialize");

        match decoded {
            Command::CompleteDocumentFetch {
                handler_id,
                response,
            } => {
                assert_eq!(handler_id, DocumentFetchId::from_u128(19));
                assert_eq!(
                    response,
                    FetchResponse {
                        final_url: String::from("https://example.test/script.js"),
                        status: 404,
                        content_type: String::from("text/html"),
                        body: vec![1, 2, 3, 4],
                    }
                );
            }
            other => panic!("expected CompleteDocumentFetch, got {other:?}"),
        }
    }
}

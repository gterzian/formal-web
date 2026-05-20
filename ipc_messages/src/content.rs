use anyrender::{
    Scene,
    recording::{GlyphRunCommand, RenderCommand},
};
use ipc_channel::ipc::{IpcReceiver, IpcSender, IpcSharedMemory};
use peniko::{Brush, FontData};
use serde::{Deserialize, Serialize};
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

#[derive(Debug, Serialize, Deserialize)]
pub struct Bootstrap {
    pub command_sender: IpcSender<Command>,
    pub event_receiver: IpcReceiver<Event>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateChildNavigableRequest {
    pub parent_traversable_id: NavigableId,
    pub content_navigable_id: NavigableId,
    pub content_frame_id: FrameId,
    #[serde(default)]
    pub target_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeforeUnloadResult {
    pub document_id: DocumentId,
    pub check_id: BeforeUnloadCheckId,
    pub canceled: bool,
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

pub fn parse_iframe_target_name(
    target_name: &str,
) -> Option<(NavigableId, NavigableId, FrameId)> {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardReadRequest {
    pub reply_sender: IpcSender<Result<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardWriteRequest {
    pub text: String,
    pub reply_sender: IpcSender<Result<(), String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchEventEntry {
    pub document_id: DocumentId,
    pub event: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceholderFrameMapping {
    pub token: u64,
    pub frame_id: FrameId,
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
    data: IpcSharedMemory,
}

impl RegisteredFont {
    fn from_font_data(id: FontIdentifier, font_data: &FontData) -> Self {
        Self {
            id,
            data: IpcSharedMemory::from_bytes(font_data.data.data()),
        }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    fn into_font_data(self) -> FontData {
        FontData::new(self.data.take().unwrap_or_default().into(), self.id.index)
    }
}

#[derive(Debug, Clone)]
pub struct PreparedScene {
    pub scene: RecordedScene,
    pub registered_fonts: Vec<RegisteredFont>,
}

#[derive(Debug, Default)]
pub struct FontTransportSender {
    sent_fonts: HashSet<FontIdentifier>,
}

impl FontTransportSender {
    pub fn prepare_scene(&mut self, font_namespace: u64, scene: Scene) -> PreparedScene {
        let mut font_ids = Vec::new();
        let mut scene_font_ids = HashMap::new();
        let mut registered_fonts = Vec::new();
        let commands = scene
            .commands
            .into_iter()
            .map(|command| {
                SerializableRenderCommand::from_render_command(command, |font_data| {
                    let font_id = FontIdentifier::from_font_data(font_namespace, font_data);
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
                        registered_fonts.push(RegisteredFont::from_font_data(font_id, font_data));
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
        }
    }
}

#[derive(Debug, Default)]
pub struct FontTransportReceiver {
    fonts: HashMap<FontIdentifier, FontData>,
}

impl FontTransportReceiver {
    pub fn register_fonts(&mut self, registered_fonts: Vec<RegisteredFont>) {
        for font in registered_fonts {
            match self.fonts.entry(font.id) {
                Entry::Occupied(_) => {}
                Entry::Vacant(entry) => {
                    entry.insert(font.into_font_data());
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
                    style: command.style,
                    brush: command.brush,
                    brush_alpha: command.brush_alpha,
                    transform: command.transform,
                    glyph_transform: command.glyph_transform,
                    glyphs: command.glyphs,
                }))
            }
            RenderCommand::BoxShadow(command) => Self(RenderCommand::BoxShadow(command)),
            RenderCommand::Placeholder(command) => Self(RenderCommand::Placeholder(command)),
        }
    }

    fn into_render_command(self, mut font_data: impl FnMut(u32) -> FontData) -> RenderCommand {
        match self.0 {
            RenderCommand::PushLayer(command) => RenderCommand::PushLayer(command),
            RenderCommand::PushClipLayer(command) => RenderCommand::PushClipLayer(command),
            RenderCommand::PopLayer => RenderCommand::PopLayer,
            RenderCommand::Stroke(command) => RenderCommand::Stroke(command),
            RenderCommand::Fill(command) => RenderCommand::Fill(command),
            RenderCommand::GlyphRun(command) => {
                RenderCommand::GlyphRun(GlyphRunCommand {
                    font_data: font_data(command.font_data),
                    font_size: command.font_size,
                    hint: command.hint,
                    normalized_coords: command.normalized_coords,
                    style: command.style,
                    brush: command.brush,
                    brush_alpha: command.brush_alpha,
                    transform: command.transform,
                    glyph_transform: command.glyph_transform,
                    glyphs: command.glyphs,
                })
            }
            RenderCommand::BoxShadow(command) => RenderCommand::BoxShadow(command),
            RenderCommand::Placeholder(command) => RenderCommand::Placeholder(command),
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
    pub placeholder_commands: usize,
    pub solid_glyph_brush_runs: usize,
    pub gradient_glyph_brush_runs: usize,
    pub image_glyph_brush_runs: usize,
}

impl SceneSummary {
    pub fn describe(&self) -> String {
        format!(
            "commands={} glyph_runs={} glyphs={} font_refs={} fills={} strokes={} push_layers={} push_clip_layers={} pops={} box_shadows={} placeholders={} glyph_brushes(solid={}, gradient={}, image={})",
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
            self.placeholder_commands,
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
                    match &command.brush {
                        Brush::Solid(_) => summary.solid_glyph_brush_runs += 1,
                        Brush::Gradient(_) => summary.gradient_glyph_brush_runs += 1,
                        Brush::Image(_) => summary.image_glyph_brush_runs += 1,
                    }
                }
                RenderCommand::BoxShadow(_) => summary.box_shadow_commands += 1,
                RenderCommand::Placeholder(_) => summary.placeholder_commands += 1,
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

fn serialize_scene_to_shared_memory(scene: &RecordedScene) -> Result<IpcSharedMemory, String> {
    let byte_len = postcard::experimental::serialized_size(scene)
        .map_err(|error| format!("failed to measure paint scene: {error}"))?;
    let mut bytes = IpcSharedMemory::from_byte(0, byte_len);
    {
        let buffer = unsafe { bytes.deref_mut() };
        let written = postcard::to_slice(scene, buffer)
            .map_err(|error| format!("failed to serialize paint scene: {error}"))?;
        debug_assert_eq!(written.len(), byte_len);
    }
    Ok(bytes)
}

fn deserialize_scene_from_shared_memory(scene_bytes: &IpcSharedMemory) -> Result<RecordedScene, String> {
    postcard::from_bytes(scene_bytes)
        .map_err(|error| format!("failed to deserialize paint scene: {error}"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaintFrame {
    pub traversable_id: WebviewId,
    pub frame_id: FrameId,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub placeholder_frame_mappings: Vec<PlaceholderFrameMapping>,
    font_registrations: Vec<RegisteredFont>,
    scene_bytes: IpcSharedMemory,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PaintTransportSummary {
    pub scene_bytes: usize,
    pub registered_fonts: usize,
    pub registered_font_bytes: usize,
}

impl PaintFrame {
    pub fn new(
        traversable_id: WebviewId,
        frame_id: FrameId,
        viewport_width: u32,
        viewport_height: u32,
        placeholder_frame_mappings: Vec<PlaceholderFrameMapping>,
        scene: PreparedScene,
    ) -> Result<Self, String> {
        let PreparedScene {
            scene,
            registered_fonts,
        } = scene;
        Ok(Self {
            traversable_id,
            frame_id,
            viewport_width,
            viewport_height,
            placeholder_frame_mappings,
            font_registrations: registered_fonts,
            scene_bytes: serialize_scene_to_shared_memory(&scene)?,
        })
    }

    pub fn transport_summary(&self) -> PaintTransportSummary {
        PaintTransportSummary {
            scene_bytes: self.scene_bytes.len(),
            registered_fonts: self.font_registrations.len(),
            registered_font_bytes: self.font_registrations.iter().map(RegisteredFont::len).sum(),
        }
    }

    pub fn into_recorded_scene(
        self,
        font_receiver: &mut FontTransportReceiver,
    ) -> Result<RecordedScene, String> {
        let PaintFrame {
            font_registrations,
            scene_bytes,
            ..
        } = self;
        font_receiver.register_fonts(font_registrations);
        deserialize_scene_from_shared_memory(&scene_bytes)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    SetTraceSender(Option<TraceSender>),
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
    DestroyDocument { document_id: DocumentId },
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
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    DocumentFetchRequested(FetchRequest),
    WindowTimerRequested(WindowTimerRequest),
    WindowTimerCleared(WindowTimerClearRequest),
    CreateChildNavigable(CreateChildNavigableRequest),
    NavigationRequested(NavigateRequest),
    BeforeUnloadCompleted(BeforeUnloadResult),
    FinalizeNavigation(FinalizeNavigation),
    IframeTraversableRemoved(IframeTraversableRemoval),
    ScriptEvaluated(ScriptEvaluationResult),
    ElementClicked(ElementClickResult),
    ClipboardReadRequested(ClipboardReadRequest),
    ClipboardWriteRequested(ClipboardWriteRequest),
    CommandCompleted,
    PaintReady(PaintFrame),
    ShutdownCompleted,
}

#[cfg(test)]
mod tests {
    use super::{
        Command, DocumentFetchId, FetchResponse, FontTransportReceiver, FontTransportSender,
        FrameId, LoadedDocumentResponse, NavigableId, PaintFrame, PaintTransportSummary,
        PlaceholderFrameMapping, SceneSummary, WebviewId,
    };
    use anyrender::{
        Glyph, PaintScene, Scene,
        recording::RenderCommand,
    };
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
            [Glyph { id: glyph_id.into(), x, y }].into_iter(),
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
            Vec::<PlaceholderFrameMapping>::new(),
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
            Vec::<PlaceholderFrameMapping>::new(),
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
            Vec::<PlaceholderFrameMapping>::new(),
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
use anyrender::{
    Scene,
    recording::{GlyphRunCommand, RenderCommand},
};
use ipc_channel::ipc::{IpcReceiver, IpcSender};
use peniko::FontData;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ContentColorScheme {
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewportSnapshot {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub color_scheme: ContentColorScheme,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContentBootstrap {
    pub command_sender: IpcSender<ContentCommand>,
    pub event_receiver: IpcReceiver<ContentEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentFetchRequest {
    pub handler_id: u64,
    pub url: String,
    pub method: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableFontData {
    pub data: Vec<u8>,
    pub index: u32,
}

impl From<FontData> for SerializableFontData {
    fn from(font_data: FontData) -> Self {
        Self {
            data: font_data.data.data().to_vec(),
            index: font_data.index,
        }
    }
}

impl From<SerializableFontData> for FontData {
    fn from(font_data: SerializableFontData) -> Self {
        FontData::new(font_data.data.into(), font_data.index)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SerializableRenderCommand(pub RenderCommand<SerializableFontData>);

impl From<RenderCommand> for SerializableRenderCommand {
    fn from(command: RenderCommand) -> Self {
        match command {
            RenderCommand::PushLayer(command) => Self(RenderCommand::PushLayer(command)),
            RenderCommand::PushClipLayer(command) => Self(RenderCommand::PushClipLayer(command)),
            RenderCommand::PopLayer => Self(RenderCommand::PopLayer),
            RenderCommand::Stroke(command) => Self(RenderCommand::Stroke(command)),
            RenderCommand::Fill(command) => Self(RenderCommand::Fill(command)),
            RenderCommand::GlyphRun(command) => Self(RenderCommand::GlyphRun(GlyphRunCommand {
                font_data: command.font_data.into(),
                font_size: command.font_size,
                hint: command.hint,
                normalized_coords: command.normalized_coords,
                style: command.style,
                brush: command.brush,
                brush_alpha: command.brush_alpha,
                transform: command.transform,
                glyph_transform: command.glyph_transform,
                glyphs: command.glyphs,
            })),
            RenderCommand::BoxShadow(command) => Self(RenderCommand::BoxShadow(command)),
        }
    }
}

impl From<SerializableRenderCommand> for RenderCommand {
    fn from(command: SerializableRenderCommand) -> Self {
        match command.0 {
            RenderCommand::PushLayer(command) => RenderCommand::PushLayer(command),
            RenderCommand::PushClipLayer(command) => RenderCommand::PushClipLayer(command),
            RenderCommand::PopLayer => RenderCommand::PopLayer,
            RenderCommand::Stroke(command) => RenderCommand::Stroke(command),
            RenderCommand::Fill(command) => RenderCommand::Fill(command),
            RenderCommand::GlyphRun(command) => RenderCommand::GlyphRun(GlyphRunCommand {
                font_data: command.font_data.into(),
                font_size: command.font_size,
                hint: command.hint,
                normalized_coords: command.normalized_coords,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrollOffset {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedScene {
    pub tolerance: f64,
    pub commands: Vec<SerializableRenderCommand>,
}

impl From<Scene> for RecordedScene {
    fn from(scene: Scene) -> Self {
        Self {
            tolerance: scene.tolerance,
            commands: scene.commands.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<RecordedScene> for Scene {
    fn from(scene: RecordedScene) -> Self {
        Self {
            tolerance: scene.tolerance,
            commands: scene.commands.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaintFrame {
    pub document_id: u64,
    pub viewport_scroll: ScrollOffset,
    pub scene: RecordedScene,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContentCommand {
    SetViewport(ViewportSnapshot),
    CreateEmptyDocument { document_id: u64 },
    CreateLoadedDocument {
        document_id: u64,
        url: String,
        body: String,
    },
    DispatchEvent {
        document_id: u64,
        event: String,
    },
    UpdateTheRendering { document_id: u64 },
    CompleteDocumentFetch {
        handler_id: u64,
        resolved_url: String,
        body: Vec<u8>,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContentEvent {
    DocumentFetchRequested(ContentFetchRequest),
    PaintReady(PaintFrame),
}

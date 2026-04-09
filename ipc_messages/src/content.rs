use anyrender::{
    Scene,
    recording::{GlyphRunCommand, RenderCommand},
};
use ipc_channel::ipc::{IpcReceiver, IpcSender};
use peniko::{Brush, FontData};
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Serialize, Deserialize)]
pub struct Bootstrap {
    pub command_sender: IpcSender<Command>,
    pub event_receiver: IpcReceiver<Event>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchRequest {
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SceneSummary {
    pub commands: usize,
    pub glyph_runs: usize,
    pub glyphs: usize,
    pub font_bytes: usize,
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
            "commands={} glyph_runs={} glyphs={} font_bytes={} fills={} strokes={} push_layers={} push_clip_layers={} pops={} box_shadows={} glyph_brushes(solid={}, gradient={}, image={})",
            self.commands,
            self.glyph_runs,
            self.glyphs,
            self.font_bytes,
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
                    summary.font_bytes += command.font_data.data.len();
                    match &command.brush {
                        Brush::Solid(_) => summary.solid_glyph_brush_runs += 1,
                        Brush::Gradient(_) => summary.gradient_glyph_brush_runs += 1,
                        Brush::Image(_) => summary.image_glyph_brush_runs += 1,
                    }
                }
                RenderCommand::BoxShadow(_) => summary.box_shadow_commands += 1,
            }
        }

        summary
    }
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
pub enum CallbackData {
    ScriptSource(String),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    SetViewport(ViewportSnapshot),
    CreateEmptyDocument { document_id: u64 },
    CreateLoadedDocument {
        document_id: u64,
        url: String,
        body: String,
    },
    EvaluateScript {
        document_id: u64,
        source: String,
    },
    DispatchEvent {
        document_id: u64,
        event: String,
    },
    CallbackReady {
        document_id: u64,
        callback_id: u64,
        data: CallbackData,
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
pub enum Event {
    DocumentFetchRequested(FetchRequest),
    PaintReady(PaintFrame),
}

#[cfg(test)]
mod tests {
    use super::{RecordedScene, SceneSummary};
    use anyrender::{Glyph, PaintScene, Scene};
    use kurbo::Affine;
    use peniko::{Color, Fill, FontData};

    #[test]
    fn recorded_scene_round_trips_glyph_runs() {
        let font = FontData::new(vec![1_u8, 2, 3, 4].into(), 0);
        let mut scene = Scene::new();
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
                id: 7,
                x: 12.0,
                y: 18.0,
            }]
            .into_iter(),
        );

        let recorded = RecordedScene::from(scene.clone());
        let summary = recorded.summary();
        assert_eq!(
            summary,
            SceneSummary {
                commands: 1,
                glyph_runs: 1,
                glyphs: 1,
                font_bytes: 4,
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
        assert_eq!(Scene::from(recorded), scene);
    }
}
//! A portable terminal grid whose state and input are owned by the application.
//!
//! Unlike [`crate::TerminalView`], this widget does not spawn a PTY or interpret
//! terminal escape sequences. It renders an already interpreted cell grid and
//! emits semantic terminal input, which makes it suitable for remote sessions
//! and mobile clients.

use fission_core::authoring::{IrBuilder, LowerWidget, LoweringContext};
use fission_core::event::{EditingCommand, ImeEvent, InputEvent, KeyCode, KeyEvent, PointerEvent};
use fission_core::internal::{
    CustomEventResult, CustomHitResult, CustomRenderObject, InternalRenderNode,
};
use fission_core::ui::{SemanticsRegion, Widget};
use fission_core::{Action, ActionEnvelope, ActionId, FlexDirection};
use fission_ir::op::{AlignItems, Color, Fill, LayoutOp, PaintOp, TextRun, TextStyle};
use fission_ir::{Op, WidgetId};
use fission_layout::{LayoutPoint, LayoutRect};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const DEFAULT_FONT_SIZE: f32 = 13.0;
const DEFAULT_LINE_HEIGHT: f32 = 18.0;
const DEFAULT_PADDING_X: f32 = 10.0;
const DEFAULT_PADDING_Y: f32 = 8.0;

/// A contiguous terminal row fragment sharing one visual style.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TerminalCellRun {
    pub text: String,
    pub foreground: Color,
    pub background: Option<Color>,
    pub underline: bool,
    pub bold: bool,
    pub dim: bool,
}

impl TerminalCellRun {
    pub fn plain(text: impl Into<String>, foreground: Color) -> Self {
        Self {
            text: text.into(),
            foreground,
            background: None,
            underline: false,
            bold: false,
            dim: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalCursorShape {
    Block,
    Underline,
    #[default]
    Bar,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalCursor {
    pub column: usize,
    pub row: usize,
    pub visible: bool,
    pub shape: TerminalCursorShape,
}

/// Complete visible state for one terminal viewport.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TerminalSurfaceSnapshot {
    pub rows: Vec<Vec<TerminalCellRun>>,
    pub columns: usize,
    pub cursor: TerminalCursor,
    pub foreground: Color,
    pub background: Color,
    pub cursor_color: Color,
}

impl Default for TerminalSurfaceSnapshot {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            columns: 80,
            cursor: TerminalCursor::default(),
            foreground: Color {
                r: 223,
                g: 230,
                b: 239,
                a: 255,
            },
            background: Color {
                r: 12,
                g: 16,
                b: 24,
                a: 255,
            },
            cursor_color: Color {
                r: 105,
                g: 240,
                b: 174,
                a: 255,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalKey {
    Enter,
    Escape,
    Backspace,
    Delete,
    Tab,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Character(char),
}

/// Semantic input emitted by [`TerminalSurface`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalInput {
    Text(String),
    Paste(String),
    Key { key: TerminalKey, modifiers: u8 },
    Scroll { lines: i32 },
}

impl Action for TerminalInput {
    fn static_id() -> ActionId {
        ActionId::from_name("fission_widgets::TerminalInput")
    }
}

/// A directly focusable terminal viewport backed by application-owned state.
#[derive(Clone, Debug)]
pub struct TerminalSurface {
    pub snapshot: TerminalSurfaceSnapshot,
    pub on_input: Option<ActionEnvelope>,
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub font_size: f32,
    pub line_height: f32,
    pub padding_x: f32,
    pub padding_y: f32,
    pub stable_id: u64,
}

impl TerminalSurface {
    pub fn new(snapshot: TerminalSurfaceSnapshot, width: f32, height: f32) -> Self {
        Self {
            snapshot,
            on_input: None,
            viewport_width: width,
            viewport_height: height,
            font_size: DEFAULT_FONT_SIZE,
            line_height: DEFAULT_LINE_HEIGHT,
            padding_x: DEFAULT_PADDING_X,
            padding_y: DEFAULT_PADDING_Y,
            stable_id: 0,
        }
    }

    pub fn on_input(mut self, action: ActionEnvelope) -> Self {
        self.on_input = Some(action);
        self
    }

    pub fn stable_id(mut self, stable_id: u64) -> Self {
        self.stable_id = stable_id;
        self
    }

    pub fn font_size(mut self, font_size: f32) -> Self {
        self.font_size = font_size;
        self
    }

    pub fn line_height(mut self, line_height: f32) -> Self {
        self.line_height = line_height;
        self
    }

    pub fn padding(mut self, x: f32, y: f32) -> Self {
        self.padding_x = x;
        self.padding_y = y;
        self
    }
}

impl From<TerminalSurface> for Widget {
    fn from(surface: TerminalSurface) -> Self {
        let node = Arc::new(TerminalSurfaceRenderNode {
            snapshot: surface.snapshot,
            on_input: surface.on_input,
            viewport_width: surface.viewport_width,
            viewport_height: surface.viewport_height,
            font_size: surface.font_size,
            line_height: surface.line_height,
            char_width: surface.font_size * 0.6,
            padding_x: surface.padding_x,
            padding_y: surface.padding_y,
            stable_id: surface.stable_id,
        });
        let lowerer: Arc<dyn LowerWidget> = node.clone();
        let render_object: Arc<dyn CustomRenderObject> = node;
        SemanticsRegion::new(fission_core::internal::custom_render_widget(
            InternalRenderNode {
                debug_tag: "TerminalSurface".into(),
                lowerer: Some(lowerer),
                render_object: Some(render_object),
            },
        ))
        .role(fission_ir::Role::TextInput)
        .label("Terminal")
        .focusable(true)
        .into()
    }
}

#[derive(Debug)]
struct TerminalSurfaceRenderNode {
    snapshot: TerminalSurfaceSnapshot,
    on_input: Option<ActionEnvelope>,
    viewport_width: f32,
    viewport_height: f32,
    font_size: f32,
    line_height: f32,
    char_width: f32,
    padding_x: f32,
    padding_y: f32,
    stable_id: u64,
}

impl TerminalSurfaceRenderNode {
    fn cursor_rect(&self, node_rect: LayoutRect) -> Option<LayoutRect> {
        let cursor = self.snapshot.cursor;
        if !cursor.visible || cursor.row >= self.snapshot.rows.len() {
            return None;
        }
        let x = node_rect.origin.x + self.padding_x + cursor.column as f32 * self.char_width;
        let y = node_rect.origin.y + self.padding_y + cursor.row as f32 * self.line_height;
        let (width, height, y_offset) = match cursor.shape {
            TerminalCursorShape::Underline => {
                (self.char_width.max(4.0), 2.0, self.line_height - 2.0)
            }
            TerminalCursorShape::Block => (self.char_width.max(6.0), self.line_height, 0.0),
            TerminalCursorShape::Bar => (2.0, self.line_height, 0.0),
        };
        Some(LayoutRect::new(x, y + y_offset, width, height))
    }

    fn emit(&self, node_id: WidgetId, input: TerminalInput) -> CustomEventResult {
        match &self.on_input {
            Some(action) => {
                CustomEventResult::consumed_with(vec![(node_id, action.with_action(&input))])
            }
            None => CustomEventResult::consumed(),
        }
    }

    fn row_runs(&self, runs: &[TerminalCellRun]) -> Vec<TextRun> {
        if runs.is_empty() {
            return vec![self.text_run(
                " ".repeat(self.snapshot.columns.max(1)),
                self.snapshot.foreground,
                None,
                false,
                false,
                false,
            )];
        }
        runs.iter()
            .map(|run| {
                self.text_run(
                    run.text.clone(),
                    run.foreground,
                    run.background,
                    run.underline,
                    run.bold,
                    run.dim,
                )
            })
            .collect()
    }

    fn text_run(
        &self,
        text: String,
        color: Color,
        background_color: Option<Color>,
        underline: bool,
        bold: bool,
        dim: bool,
    ) -> TextRun {
        let color = if dim { dim_color(color, 0.72) } else { color };
        TextRun {
            text,
            style: TextStyle {
                font_size: self.font_size,
                color,
                underline,
                font_family: Some("monospace".into()),
                locale: None,
                font_weight: if bold { 700 } else { 400 },
                font_style: fission_ir::op::FontStyle::Normal,
                line_height: Some(self.line_height),
                letter_spacing: 0.0,
                background_color,
                typography: Default::default(),
            },
        }
    }
}

impl LowerWidget for TerminalSurfaceRenderNode {
    fn lower_dyn(&self, cx: &mut LoweringContext) -> WidgetId {
        let row_count = self.snapshot.rows.len().max(1);
        let width = self
            .viewport_width
            .max(self.char_width * self.snapshot.columns.max(1) as f32 + self.padding_x * 2.0);
        let height = self
            .viewport_height
            .max(self.line_height * row_count as f32 + self.padding_y * 2.0);

        let background = IrBuilder::new(
            cx.next_node_id(),
            Op::Paint(PaintOp::DrawRect {
                fill: Some(Fill::Solid(self.snapshot.background)),
                stroke: None,
                corner_radius: 0.0,
                shadow: None,
                corner_radii: None,
                border_sides: None,
            }),
        )
        .build(cx);

        let rows = if self.snapshot.rows.is_empty() {
            vec![Vec::new()]
        } else {
            self.snapshot.rows.clone()
        };
        let mut row_ids = Vec::with_capacity(rows.len());
        for row in &rows {
            let paint = IrBuilder::new(
                cx.next_node_id(),
                Op::Paint(PaintOp::DrawRichText {
                    runs: self.row_runs(row),
                    wrap: false,
                    caret_index: None,
                    caret_color: None,
                    caret_width: None,
                    caret_height: None,
                    caret_radius: None,
                    paragraph_style: None,
                }),
            )
            .build(cx);
            let mut row_box = IrBuilder::new(
                cx.next_node_id(),
                Op::Layout(LayoutOp::Box {
                    width: Some(width - self.padding_x * 2.0),
                    height: Some(self.line_height),
                    min_width: None,
                    max_width: None,
                    min_height: None,
                    max_height: None,
                    padding: [0.0; 4],
                    flex_grow: 0.0,
                    flex_shrink: 0.0,
                    aspect_ratio: None,
                }),
            );
            row_box.add_child(paint);
            row_ids.push(row_box.build(cx));
        }

        let mut column = IrBuilder::new(
            cx.next_node_id(),
            Op::Layout(LayoutOp::Flex {
                direction: FlexDirection::Column,
                wrap: fission_ir::op::FlexWrap::NoWrap,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                padding: [
                    self.padding_y,
                    self.padding_x,
                    self.padding_y,
                    self.padding_x,
                ],
                gap: None,
                line_gap: None,
                align_items: AlignItems::Stretch,
                justify_content: fission_ir::op::JustifyContent::Start,
            }),
        );
        column.add_children(row_ids);

        let mut layers = IrBuilder::new(cx.next_node_id(), Op::Layout(LayoutOp::ZStack));
        layers.add_child(background);
        layers.add_child(column.build(cx));
        if let Some(rect) = self.cursor_rect(LayoutRect::new(0.0, 0.0, width, height)) {
            let cursor = IrBuilder::new(
                cx.next_node_id(),
                Op::Paint(PaintOp::DrawRect {
                    fill: Some(Fill::Solid(self.snapshot.cursor_color)),
                    stroke: None,
                    corner_radius: 0.0,
                    shadow: None,
                    corner_radii: None,
                    border_sides: None,
                }),
            )
            .build(cx);
            let mut positioned = IrBuilder::new(
                cx.next_node_id(),
                Op::Layout(LayoutOp::Positioned {
                    left: Some(rect.origin.x),
                    top: Some(rect.origin.y),
                    right: None,
                    bottom: None,
                    width: Some(rect.size.width),
                    height: Some(rect.size.height),
                }),
            );
            positioned.add_child(cursor);
            layers.add_child(positioned.build(cx));
        }

        let mut outer = IrBuilder::new(
            cx.next_node_id(),
            Op::Layout(LayoutOp::Box {
                width: Some(width),
                height: Some(height),
                min_width: Some(width),
                max_width: None,
                min_height: Some(height),
                max_height: None,
                padding: [0.0; 4],
                flex_grow: 1.0,
                flex_shrink: 1.0,
                aspect_ratio: None,
            }),
        );
        outer.add_child(layers.build(cx));
        outer.build(cx)
    }

    fn widget_id(&self) -> Option<WidgetId> {
        Some(WidgetId::derived(
            WidgetId::explicit("fission.terminal.surface").as_u128(),
            &[self.stable_id as u32, (self.stable_id >> 32) as u32],
        ))
    }

    fn stable_key(&self) -> u64 {
        self.stable_id
    }
}

impl CustomRenderObject for TerminalSurfaceRenderNode {
    fn is_runtime_dynamic(&self) -> bool {
        true
    }

    fn accepts_text_input(&self) -> bool {
        true
    }

    fn hit_test(&self, _local_point: LayoutPoint, _node_rect: LayoutRect) -> CustomHitResult {
        CustomHitResult::inside(None)
    }

    fn handle_event(
        &self,
        node_id: WidgetId,
        event: &InputEvent,
        _node_rect: LayoutRect,
    ) -> CustomEventResult {
        match event {
            InputEvent::Keyboard(KeyEvent::Down {
                key_code,
                modifiers,
            }) => self.emit(
                node_id,
                TerminalInput::Key {
                    key: map_key_code(key_code),
                    modifiers: *modifiers,
                },
            ),
            InputEvent::Keyboard(KeyEvent::DownWithText {
                key_code,
                modifiers,
                text,
            }) => {
                if text.is_empty() || modifiers & (2 | 4 | 8) != 0 {
                    self.emit(
                        node_id,
                        TerminalInput::Key {
                            key: map_key_code(key_code),
                            modifiers: *modifiers,
                        },
                    )
                } else {
                    self.emit(node_id, TerminalInput::Text(text.clone()))
                }
            }
            InputEvent::Ime(ImeEvent::Commit { text }) => {
                self.emit(node_id, TerminalInput::Text(text.clone()))
            }
            InputEvent::Ime(ImeEvent::Preedit { .. }) => CustomEventResult::consumed(),
            InputEvent::Editing(EditingCommand::Paste(text)) => {
                self.emit(node_id, TerminalInput::Paste(text.clone()))
            }
            InputEvent::Pointer(PointerEvent::Scroll { delta, .. }) => {
                let lines = if delta.y.abs() < 1.0 {
                    delta.y.signum() as i32
                } else {
                    (delta.y / self.line_height).round() as i32
                };
                self.emit(node_id, TerminalInput::Scroll { lines })
            }
            InputEvent::Pointer(PointerEvent::Down { .. }) => CustomEventResult::consumed(),
            _ => CustomEventResult::ignored(),
        }
    }

    fn ime_cursor_area(&self, node_rect: LayoutRect) -> Option<LayoutRect> {
        self.cursor_rect(node_rect)
    }
}

fn map_key_code(key: &KeyCode) -> TerminalKey {
    match key {
        KeyCode::Space => TerminalKey::Character(' '),
        KeyCode::Enter => TerminalKey::Enter,
        KeyCode::Escape => TerminalKey::Escape,
        KeyCode::Backspace => TerminalKey::Backspace,
        KeyCode::Delete => TerminalKey::Delete,
        KeyCode::Tab => TerminalKey::Tab,
        KeyCode::Left => TerminalKey::Left,
        KeyCode::Right => TerminalKey::Right,
        KeyCode::Up => TerminalKey::Up,
        KeyCode::Down => TerminalKey::Down,
        KeyCode::Home => TerminalKey::Home,
        KeyCode::End => TerminalKey::End,
        KeyCode::PageUp => TerminalKey::PageUp,
        KeyCode::PageDown => TerminalKey::PageDown,
        KeyCode::Char(ch) => TerminalKey::Character(*ch),
    }
}

fn dim_color(color: Color, factor: f32) -> Color {
    Color {
        r: ((color.r as f32) * factor).round().clamp(0.0, 255.0) as u8,
        g: ((color.g as f32) * factor).round().clamp(0.0, 255.0) as u8,
        b: ((color.b as f32) * factor).round().clamp(0.0, 255.0) as u8,
        a: color.a,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_node(snapshot: TerminalSurfaceSnapshot) -> TerminalSurfaceRenderNode {
        TerminalSurfaceRenderNode {
            snapshot,
            on_input: None,
            viewport_width: 800.0,
            viewport_height: 480.0,
            font_size: 14.0,
            line_height: 20.0,
            char_width: 8.4,
            padding_x: 10.0,
            padding_y: 8.0,
            stable_id: 1,
        }
    }

    #[test]
    fn maps_navigation_and_character_keys() {
        assert_eq!(map_key_code(&KeyCode::Left), TerminalKey::Left);
        assert_eq!(
            map_key_code(&KeyCode::Char('x')),
            TerminalKey::Character('x')
        );
    }

    #[test]
    fn dimming_preserves_alpha() {
        assert_eq!(
            dim_color(
                Color {
                    r: 100,
                    g: 50,
                    b: 10,
                    a: 200
                },
                0.5
            ),
            Color {
                r: 50,
                g: 25,
                b: 5,
                a: 200
            }
        );
    }

    #[test]
    fn styled_cell_runs_lower_as_monospace_rich_text() {
        let foreground = Color {
            r: 12,
            g: 240,
            b: 90,
            a: 255,
        };
        let background = Color {
            r: 20,
            g: 30,
            b: 40,
            a: 255,
        };
        let node = render_node(TerminalSurfaceSnapshot::default());
        let runs = node.row_runs(&[TerminalCellRun {
            text: "CPU".into(),
            foreground,
            background: Some(background),
            underline: true,
            bold: true,
            dim: false,
        }]);

        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "CPU");
        assert_eq!(runs[0].style.font_family.as_deref(), Some("monospace"));
        assert_eq!(runs[0].style.color, foreground);
        assert_eq!(runs[0].style.background_color, Some(background));
        assert_eq!(runs[0].style.font_weight, 700);
        assert!(runs[0].style.underline);
    }
}

//! Builds the quill tree from a [`ViewModel`].
//!
//! This is the whole UI for now: a left rail, and one of three pages. It is
//! deliberately small — the migration plan's "minimal closed loop" — and it
//! uses only public `draw_components` APIs. Callbacks push [`Action`]s into an
//! [`Actions`] queue; the app drains them after routing input.
//!
//! ## Layout shape (matters)
//!
//! quill's layout root places its direct children by **anchors**, and flex
//! starts one level down (see `examples/file_browser/src/ui.rs`). So the tree
//! is the canonical three layers:
//!
//! ```text
//! Flex::column()                 <- root, fills the viewport
//!   └─ Flex::row()               <- the two panes side by side
//!        ├─ sidebar             <- fixed basis, shrink(0)
//!        └─ content             <- grow(1.0), clip(true)
//! ```
//!
//! A single `Row` mounted at the root, with panes left to size to content, is
//! what made everything pile up at the origin.

use std::cell::RefCell;
use std::rc::Rc;

use draw_components::{
    Badge, Button, Card, Column, Component, Divider, EmptyState, Flex, Panel, Row, Text,
};
use draw_core::{Color, Edges};
use draw_scene::{SceneChild, SceneTree};
use draw_theme::{space, Theme, Tone};
use draw_ui::{MouseFilter, SizeBasis, SurfaceStyle};

use crate::frame::FrameImage;
use crate::model::{Action, Section, ViewModel};

/// The rail's fixed width in logical pixels.
const SIDEBAR_WIDTH: f32 = 224.0;

/// Where view callbacks deposit what the user did. The app drains it once per
/// frame (see the quill UI guide's "state lives in cells" rule).
#[derive(Clone, Default)]
pub struct Actions {
    queue: Rc<RefCell<Vec<Action>>>,
}

impl Actions {
    /// Record an action.
    pub fn push(&self, action: Action) {
        self.queue.borrow_mut().push(action);
    }

    /// Take everything recorded since the last drain.
    pub fn drain(&self) -> Vec<Action> {
        std::mem::take(&mut *self.queue.borrow_mut())
    }
}

/// Build the whole tree for one frame.
pub fn build(theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) -> SceneTree {
    Flex::column()
        .mouse_filter(MouseFilter::Ignore)
        .child(
            Flex::row()
                .gap(0.0)
                .padding(Edges::ZERO)
                .mouse_filter(MouseFilter::Ignore)
                .child(sidebar(theme, model, actions))
                .child(content(theme, model, actions)),
        )
        .into_tree()
}

fn sidebar(theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) -> Column {
    let mut column = Column::new()
        .gap(space::XS)
        .padding(Edges::new(space::MD, space::LG, space::MD, space::MD))
        .basis(SizeBasis::Px(SIDEBAR_WIDTH))
        .shrink(0.0)
        .surface(SurfaceStyle::new(theme.palette().surface))
        .mouse_filter(MouseFilter::Ignore);
    column = column.child(
        Text::subheading("Classic Game Box", theme)
            .max_lines(1)
            .ellipsis(true),
    );
    column = column.child(Text::caption("Rust + libretro", theme).tone(Tone::Subtle));
    column = column.child(Divider::horizontal(theme));
    for section in Section::ALL {
        let actions = actions.clone();
        let button = if section == model.section {
            Button::primary(section.label(), theme)
        } else {
            Button::ghost(section.label(), theme)
        };
        column = column.child(button.on_click(move || actions.push(Action::Show(section))));
    }
    column
}

/// The content column, already padded and set to take the rest of the row.
fn content(theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) -> Panel {
    let page = match model.section {
        Section::Library => library_page(theme, model, actions),
        Section::Play => play_page(theme, model, actions),
        Section::Settings => settings_page(theme, model),
    };
    // The status line is the app's only channel for "saved", "no core",
    // "read failed" — keep it visible under every page, not just the play one.
    let mut column = Column::new()
        .gap(space::XS)
        .mouse_filter(MouseFilter::Ignore)
        .child(page.padding(Edges::all(space::LG)).grow(1.0));
    if !model.status.is_empty() {
        column = column.child(
            Column::new()
                .padding(Edges::new(space::LG, space::XS, space::LG, space::SM))
                .mouse_filter(MouseFilter::Ignore)
                .child(Text::caption(model.status.as_str(), theme).tone(Tone::Muted)),
        );
    }

    Panel::new()
        .color(Color::TRANSPARENT)
        .flat()
        .grow(1.0)
        .clip(true)
        .mouse_filter(MouseFilter::Ignore)
        .child(column)
}

fn library_page(theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) -> Column {
    let mut column = Column::new()
        .gap(space::MD)
        .mouse_filter(MouseFilter::Ignore);
    column = column.child(Text::title("游戏库", theme));

    if model.games.is_empty() {
        column = column.child(
            EmptyState::new("还没有游戏", theme).description("把 ROM 放进库目录，或点“添加 ROM”。"),
        );
    } else {
        let mut list = Column::new().gap(space::XS);
        for (index, game) in model.games.iter().enumerate() {
            let actions = actions.clone();
            let label = format!("{}   ·   {}", game.title, game.system.short());
            list = list.child(
                Button::ghost(label, theme)
                    .min_size(0.0, 30.0)
                    .on_click(move || actions.push(Action::Play(index))),
            );
        }
        column = column.child(
            Card::new(theme)
                .gap(space::XS)
                .padding(Edges::all(space::SM))
                .child(list),
        );
    }

    let actions = actions.clone();
    column = column.child(
        Button::secondary("添加 ROM…", theme).on_click(move || actions.push(Action::OpenRom)),
    );
    column
}

fn play_page(theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) -> Column {
    let mut column = Column::new()
        .gap(space::MD)
        .mouse_filter(MouseFilter::Ignore);
    column = column.child(Text::title("游玩", theme));

    // A game launched from `--rom` may not be in the library list, so the
    // title falls back to the core name rather than the selected row.
    let title = model
        .selected
        .and_then(|index| model.games.get(index))
        .map(|game| game.title.clone())
        .or_else(|| (!model.core_name.is_empty()).then(|| model.core_name.clone()))
        .unwrap_or_else(|| "没有选中游戏".to_string());
    column = column.child(Text::subheading(title, theme).max_lines(1).ellipsis(true));

    // The framebuffer itself. It grows into the remaining space and centres an
    // integer-scaled, letterboxed picture inside whatever rectangle it gets.
    match &model.frame {
        Some(frame) => {
            column =
                column.child(FrameImage::new(frame.texture, frame.width, frame.height).grow(1.0));
        }
        None => {
            column = column.child(
                Card::new(theme)
                    .grow(1.0)
                    .padding(Edges::all(space::LG))
                    .child(Text::small("没有画面：还没有载入游戏。", theme).tone(Tone::Muted)),
            );
        }
    }

    let pause = if model.paused { "继续" } else { "暂停" };
    let mut controls = Row::new().gap(space::SM);
    let toggle = actions.clone();
    controls = controls
        .child(Button::primary(pause, theme).on_click(move || toggle.push(Action::TogglePause)));
    let reset = actions.clone();
    controls = controls
        .child(Button::secondary("复位", theme).on_click(move || reset.push(Action::Reset)));
    let save = actions.clone();
    controls = controls.child(
        Button::secondary("快速存档", theme).on_click(move || save.push(Action::SaveState(0))),
    );
    let load = actions.clone();
    controls = controls.child(
        Button::secondary("快速读档", theme).on_click(move || load.push(Action::LoadState(0))),
    );
    column = column.child(controls);
    column = column.child(
        Text::caption("F5 存档 / F6 读档；F1–F3 存槽，Shift+F1–F3 读槽", theme).tone(Tone::Subtle),
    );

    if !model.core_name.is_empty() {
        column = column
            .child(Text::small(format!("核心：{}", model.core_name), theme).tone(Tone::Muted));
    }
    column
}

fn settings_page(theme: &'static dyn Theme, model: &ViewModel) -> Column {
    Column::new()
        .gap(space::MD)
        .mouse_filter(MouseFilter::Ignore)
        .child(Text::title("设置", theme))
        .child(
            Card::new(theme)
                .gap(space::SM)
                .padding(Edges::all(space::SM))
                .child(Text::subheading("模拟器核心", theme))
                .child(Text::small(format!("当前核心：{}", model.core_name), theme))
                .child(
                    Text::small("每个机种的核心选择器接回后放在这里。", theme).tone(Tone::Muted),
                ),
        )
        .child(
            Card::new(theme)
                .gap(space::SM)
                .padding(Edges::all(space::SM))
                .child(Text::subheading("按键与手柄", theme))
                .child(
                    Text::small("键盘绑定与 gilrs 手柄映射在 cgb-input。", theme).tone(Tone::Muted),
                )
                .child(Badge::pill("默认布局", theme)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FrameHandle;
    use draw_core::Size;
    use draw_render::{DrawCommand, PaintContext, TextureId};
    use draw_theme::{default_theme, Mode};

    /// The play page must emit exactly the image command the wgpu backend turns
    /// into a texture blit; the tree/layout path is otherwise untested.
    #[test]
    fn the_play_page_emits_a_draw_image() {
        let theme = default_theme(Mode::Dark);
        let actions = Actions::default();
        let model = ViewModel {
            section: Section::Play,
            playing: true,
            frame: Some(FrameHandle {
                texture: TextureId::new(1),
                width: 256,
                height: 240,
            }),
            ..ViewModel::default()
        };

        let mut tree = build(theme, &model, &actions);
        draw_ui::layout(
            &mut tree,
            draw_core::ViewportSize::new(Size::new(1100.0, 760.0)),
        );
        tree.update();

        let mut ctx = PaintContext::new();
        draw_ui::paint(&tree, &mut ctx);
        let list = ctx.into_draw_list();

        let destination = list.commands().iter().find_map(|command| match command {
            DrawCommand::DrawImage { destination, .. } => Some(*destination),
            _ => None,
        });
        let destination = destination.expect("the play page draws the framebuffer");
        // One scale for both axes: aspect ratio preserved (the fit may be
        // fractional, so the scale is not necessarily a whole number).
        let scale_x = destination.size.width / 256.0;
        let scale_y = destination.size.height / 240.0;
        assert!(scale_x > 0.0);
        assert!((scale_x - scale_y).abs() < 0.01, "{scale_x} vs {scale_y}");
    }

    /// Save/load results arrive as `ViewModel::status`; the shared shell must
    /// paint it, not swallow it.
    #[test]
    fn the_status_line_is_painted() {
        let theme = default_theme(Mode::Dark);
        let actions = Actions::default();
        let model = ViewModel {
            status: "已存档（槽位 0）".to_string(),
            ..ViewModel::default()
        };

        let mut tree = build(theme, &model, &actions);
        draw_ui::layout(
            &mut tree,
            draw_core::ViewportSize::new(Size::new(1100.0, 760.0)),
        );
        tree.update();

        let mut ctx = PaintContext::new();
        draw_ui::paint(&tree, &mut ctx);
        let list = ctx.into_draw_list();
        assert!(list.commands().iter().any(|command| matches!(command,
            DrawCommand::DrawText { text, .. } if text.contains("已存档"))));
    }
}

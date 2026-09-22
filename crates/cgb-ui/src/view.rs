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
    Badge, Button, Card, Column, Component, Divider, EmptyState, Flex, Grid, Panel, Row,
    ScrollView, ScrollViewState, Text,
};
use draw_core::{Color, Edges};
use draw_scene::{SceneChild, SceneTree};
use draw_theme::{radius, space, Theme, Tone};
use draw_ui::{Align, Justify, MouseFilter, SizeBasis, SurfaceStyle, Track};

use crate::frame::FrameImage;
use crate::model::{Action, GameRow, Section, ViewModel};

/// The rail's fixed width in logical pixels.
const SIDEBAR_WIDTH: f32 = 224.0;

/// Library grid columns. A fixed count; the cells split the available width.
const LIBRARY_COLUMNS: usize = 4;

/// Height of a card's cover placeholder in logical pixels.
const PLACEHOLDER_HEIGHT: f32 = 132.0;

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

/// Build the whole tree for one frame, plus the persistent view state the app
/// must drive across frames (the library grid's scroll offset).
pub fn build(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &Actions,
) -> (SceneTree, Option<ScrollViewState>) {
    let mut library_scroll = None;
    let tree = Flex::column()
        .mouse_filter(MouseFilter::Ignore)
        .child(
            Flex::row()
                .gap(0.0)
                .padding(Edges::ZERO)
                .mouse_filter(MouseFilter::Ignore)
                .child(sidebar(theme, model, actions))
                .child(content(theme, model, actions, &mut library_scroll)),
        )
        .into_tree();
    (tree, library_scroll)
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
fn content(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &Actions,
    library_scroll: &mut Option<ScrollViewState>,
) -> Panel {
    let page = match model.section {
        Section::Library => library_page(theme, model, actions, library_scroll),
        Section::Play => play_page(theme, model, actions),
        Section::Settings => settings_page(theme, model, actions),
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

fn library_page(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &Actions,
    library_scroll: &mut Option<ScrollViewState>,
) -> Column {
    let mut column = Column::new()
        .gap(space::MD)
        .mouse_filter(MouseFilter::Ignore);
    column = column.child(Text::title("游戏库", theme));

    if model.games.is_empty() {
        column = column.child(
            EmptyState::new("还没有游戏", theme).description("把 ROM 放进库目录，或点“添加 ROM”。"),
        );
    } else {
        // Only the grid scrolls; the title and the add button stay put.
        let view = ScrollView::new(theme)
            .grow(1.0)
            .child(library_grid(theme, model, actions));
        *library_scroll = Some(view.state());
        column = column.child(view);
    }

    let add_files = actions.clone();
    let add_dir = actions.clone();
    column = column.child(
        Row::new()
            .gap(space::SM)
            .child(
                Button::secondary("添加游戏文件…", theme)
                    .on_click(move || add_files.push(Action::AddGames)),
            )
            .child(
                Button::ghost("添加游戏目录…", theme)
                    .on_click(move || add_dir.push(Action::OpenRom)),
            ),
    );
    column
}

/// The library as a fixed-column grid of cover cards. The whole grid is the
/// content of the library page's [`ScrollView`].
fn library_grid(theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) -> Grid {
    let mut grid = Grid::new(vec![Track::Fr(1.0); LIBRARY_COLUMNS])
        .gap(space::MD)
        .padding(Edges::ZERO);
    for (index, game) in model.games.iter().enumerate() {
        grid = grid.child(game_card(theme, game, index, actions));
    }
    grid
}

/// One library cell: a cover placeholder, the title, and a console badge.
///
/// The cover is a placeholder until real captures exist — quill has no image
/// widget, and loading covers is a later step (see `frame.rs`). Clicking
/// anywhere on the card starts the game.
fn game_card(theme: &'static dyn Theme, game: &GameRow, index: usize, actions: &Actions) -> Card {
    let actions = actions.clone();
    Card::new(theme)
        .gap(space::XS)
        .padding(Edges::all(space::SM))
        .on_click(move || actions.push(Action::Play(index)))
        .child(
            Flex::row()
                .align(Align::Center)
                .justify(Justify::Center)
                .min_size(0.0, PLACEHOLDER_HEIGHT)
                .surface(
                    SurfaceStyle::new(theme.palette().surface)
                        .border(theme.palette().border)
                        .radius(radius::MD),
                )
                .child(Text::caption("截图", theme).tone(Tone::Muted)),
        )
        .child(
            Text::small(game.title.as_str(), theme)
                .max_lines(1)
                .ellipsis(true),
        )
        .child(Row::new().child(Badge::new(game.system.short(), theme)))
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

fn settings_page(theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) -> Column {
    let mut column = Column::new()
        .gap(space::MD)
        .mouse_filter(MouseFilter::Ignore)
        .child(Text::title("设置", theme));

    // Library folders the scanner walks.
    let mut dirs = Column::new().gap(space::XS);
    if model.library_dirs.is_empty() {
        dirs = dirs.child(Text::small("还没有扫描目录。", theme).tone(Tone::Muted));
    } else {
        for (index, dir) in model.library_dirs.iter().enumerate() {
            let actions = actions.clone();
            dirs = dirs.child(
                Row::new()
                    .gap(space::SM)
                    .child(
                        Text::small(dir.as_str(), theme)
                            .grow(1.0)
                            .max_lines(1)
                            .ellipsis(true),
                    )
                    .child(
                        Button::ghost("移除", theme)
                            .on_click(move || actions.push(Action::RemoveLibraryDir(index))),
                    ),
            );
        }
    }
    let add = actions.clone();
    column = column.child(
        Card::new(theme)
            .gap(space::SM)
            .padding(Edges::all(space::SM))
            .child(Text::subheading("游戏目录", theme))
            .child(dirs)
            .child(
                Button::secondary("添加游戏目录…", theme)
                    .on_click(move || add.push(Action::OpenRom)),
            ),
    );

    // One core pick per console. The selected one is the core that would run
    // the console now (the saved pick, or the manifest default).
    let mut cores = Column::new().gap(space::SM);
    let mut any_core = false;
    for system in cgb_systems::SYSTEMS {
        let mut group = Column::new().gap(space::XS);
        let mut any = false;
        for (index, core) in model.cores.iter().enumerate() {
            if core.system != *system {
                continue;
            }
            any = true;
            any_core = true;
            let actions = actions.clone();
            let button = if core.selected {
                Button::primary(format!("{}（当前）", core.name), theme)
            } else {
                Button::ghost(core.name.clone(), theme)
            };
            group = group.child(button.on_click(move || actions.push(Action::SelectCore(index))));
        }
        if any {
            cores = cores
                .child(Text::small(system.name(), theme).tone(Tone::Muted))
                .child(group);
        }
    }
    if !any_core {
        cores =
            cores.child(Text::small("没有可用核心（见 cores.json）。", theme).tone(Tone::Muted));
    }
    column = column.child(
        Card::new(theme)
            .gap(space::SM)
            .padding(Edges::all(space::SM))
            .child(Text::subheading("模拟器核心", theme))
            .child(cores),
    );

    // Keyboard bindings, read-only for now.
    let mut bindings = Column::new().gap(space::XS);
    if model.bindings.is_empty() {
        bindings = bindings.child(Text::small("没有绑定。", theme).tone(Tone::Muted));
    } else {
        for row in &model.bindings {
            bindings = bindings.child(Text::small(
                format!("{}   —   {}", row.button, row.keys),
                theme,
            ));
        }
    }
    column = column.child(
        Card::new(theme)
            .gap(space::SM)
            .padding(Edges::all(space::SM))
            .child(Text::subheading("按键", theme))
            .child(bindings)
            .child(Text::caption("手柄走 gilrs 自动映射；重绑定后置。", theme).tone(Tone::Subtle)),
    );

    column
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BindingRow, CoreRow, FrameHandle, GameRow};
    use cgb_systems::SystemId;
    use draw_core::Size;
    use draw_render::{DrawCommand, PaintContext, TextureId};
    use draw_theme::{default_theme, Mode};

    fn paint(model: &ViewModel) -> draw_render::DrawList {
        let theme = default_theme(Mode::Dark);
        let actions = Actions::default();
        let (mut tree, mut scroll) = build(theme, model, &actions);
        let viewport = draw_core::ViewportSize::new(Size::new(1100.0, 760.0));
        draw_ui::layout(&mut tree, viewport);
        tree.update();
        if let Some(scroll) = scroll.as_mut() {
            if scroll.sync(&mut tree) {
                draw_ui::layout(&mut tree, viewport);
                tree.update();
            }
        }
        let mut ctx = PaintContext::new();
        draw_ui::paint(&tree, &mut ctx);
        ctx.into_draw_list()
    }

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

        let (mut tree, _) = build(theme, &model, &actions);
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
        let model = ViewModel {
            status: "已存档（槽位 0）".to_string(),
            ..ViewModel::default()
        };
        let list = paint(&model);
        assert!(list.commands().iter().any(|command| matches!(command,
            DrawCommand::DrawText { text, .. } if text.contains("已存档"))));
    }

    /// The settings page must actually show the scanned folders and the core
    /// choices, not the old placeholder text.
    #[test]
    fn the_settings_page_lists_dirs_and_cores() {
        let model = ViewModel {
            section: Section::Settings,
            library_dirs: vec!["/roms/nes".to_string()],
            cores: vec![
                CoreRow {
                    key: "mesen".to_string(),
                    name: "Mesen".to_string(),
                    system: SystemId::Nes,
                    selected: true,
                },
                CoreRow {
                    key: "nestopia".to_string(),
                    name: "Nestopia".to_string(),
                    system: SystemId::Nes,
                    selected: false,
                },
            ],
            bindings: vec![BindingRow {
                button: "A".to_string(),
                keys: "X / K".to_string(),
            }],
            ..ViewModel::default()
        };
        let list = paint(&model);
        let has = |needle: &str| {
            list.commands().iter().any(|command| {
                matches!(command,
                    DrawCommand::DrawText { text, .. } if text.contains(needle))
            })
        };
        assert!(has("/roms/nes"), "the folder is listed");
        assert!(has("Nestopia"), "every core for the console is offered");
        assert!(has("Mesen"), "the selected core is shown");
        assert!(has("X / K"), "the binding is shown");
    }

    /// The library is a grid, not a list: the first `LIBRARY_COLUMNS` cells
    /// share a row (increasing x, same baseline) and the next one wraps to a
    /// new row below.
    #[test]
    fn the_library_page_lays_games_out_in_a_grid() {
        let games: Vec<GameRow> = (0..LIBRARY_COLUMNS + 1)
            .map(|index| GameRow {
                title: format!("Game {index}"),
                system: SystemId::Nes,
                path: format!("/roms/game{index}.nes"),
            })
            .collect();
        let model = ViewModel {
            games,
            ..ViewModel::default()
        };
        let list = paint(&model);
        let position = |needle: &str| {
            list.commands().iter().find_map(|command| match command {
                DrawCommand::DrawText { text, position, .. } if text == needle => Some(*position),
                _ => None,
            })
        };

        let first = position("Game 0").expect("the first card paints its title");
        let second = position("Game 1").expect("the second card paints its title");
        let wrapped = position(&format!("Game {LIBRARY_COLUMNS}"))
            .expect("the wrapped card paints its title");

        assert!(
            (first.y - second.y).abs() < 0.5,
            "the first columns share a row: {first:?} vs {second:?}"
        );
        assert!(second.x > first.x, "the next column is to the right");
        assert!(wrapped.y > first.y, "the next row is below the first");
    }
}

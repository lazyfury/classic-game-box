//! Builds the quill tree from a [`ViewModel`].
//!
//! Three columns, following the old Electron front end: a narrow icon rail on
//! the left picks what the middle column shows (the game library or settings);
//! the right column is the console and is always mounted. A slim header and a
//! status line top and tail the shell.
//!
//! ```text
//! Flex::column()                     <- layout root, one anchor-sized child
//!   └─ Flex::column()                <- the vertical stack (flex starts here)
//!        ├─ header
//!        ├─ Flex::row()              <- the three columns
//!        │    ├─ rail         (64px, shrink 0)   icon + label sections
//!        │    ├─ middle       (320px, shrink 0)  library grid / settings
//!        │    └─ play column  (grow 1)           the console, always
//!        └─ status bar
//! ```
//!
//! This still uses only public `draw_components` APIs. Callbacks push
//! [`Action`]s into an [`Actions`] queue; the app drains them after routing
//! input.
//!
//! ## Layout shape (matters)
//!
//! quill's layout root places its direct children by **anchors**, and flex
//! starts one level down (see `examples/file_browser/src/ui.rs`). So the row
//! of columns must live inside the root column, not at the root itself.

use std::cell::RefCell;
use std::rc::Rc;

use draw_components::{
    Button, Card, Column, Component, Divider, EmptyState, Flex, Glyph, Grid, Icon, Panel, Row,
    ScrollView, ScrollViewState, Text,
};
use draw_core::{Color, Edges};
use draw_scene::{SceneChild, SceneTree};
use draw_theme::radius::MD;
use draw_theme::{radius, space, Theme, Tone};
use draw_ui::{Align, Justify, MouseFilter, SizeBasis, SurfaceStyle, Track};

use crate::frame::FrameImage;
use crate::model::{Action, GameRow, Section, ViewModel};

/// The rail's fixed width in logical pixels.
const RAIL_WIDTH: f32 = 64.0;

/// The middle column's fixed width in logical pixels. The play column takes
/// whatever is left.
const MIDDLE_WIDTH: f32 = 320.0;

/// Library grid columns. Fixed count, so the cards stay a predictable size.
const LIBRARY_COLUMNS: usize = 2;

/// Height of a card's cover placeholder in logical pixels.
const PLACEHOLDER_HEIGHT: f32 = 112.0;


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
    let mut middle_scroll = None;
    // The layout root places its direct children by anchors, so the vertical
    // stack is one level down: the root's single child is a column, and *its*
    // children (header / columns / status) are the flex items.
    let tree = Flex::column()
        .mouse_filter(MouseFilter::Ignore)
        .child(
            Flex::column()
                .gap(0.0)
                .padding(Edges::ZERO)
                .mouse_filter(MouseFilter::Ignore)
                .child(header(theme, model))
                .child(
                    Flex::row()
                        .grow(1.0)
                        .gap(0.0)
                        .padding(Edges::ZERO)
                        .mouse_filter(MouseFilter::Ignore)
                        .child(rail(theme, model, actions))
                        .child(middle(theme, model, actions, &mut middle_scroll))
                        .child(play_column(theme, model, actions)),
                )
                .child(status_bar(theme, model)),
        )
        .into_tree();
    (tree, middle_scroll)
}

/// The slim top bar: the app name and what the middle column is showing.
fn header(theme: &'static dyn Theme, model: &ViewModel) -> Column {
    Column::new()
        .gap(0.0)
        .child(
            Row::new()
                .align(Align::Center)
                .gap(space::SM)
                .padding(Edges::new(space::LG, space::SM, space::LG, space::SM))
                .min_size(0.0, 40.0)
                .child(Text::subheading("Classic Game Box", theme).bold())
                .child(Text::caption(model.section.label(), theme).tone(Tone::Muted)),
        )
        .child(Divider::horizontal(theme))
}

/// The left rail: one icon-over-label button per section (the VS Code
/// activity bar, with the names always visible).
fn rail(theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) -> Column {
    let mut rail = Column::new()
        .gap(space::SM)
        .padding(Edges::new(space::XS, space::SM, space::XS, space::SM))
        .basis(SizeBasis::Px(RAIL_WIDTH))
        .shrink(0.0)
        .surface(SurfaceStyle::new(theme.palette().surface))
        .mouse_filter(MouseFilter::Ignore);
    for section in Section::ALL {
        rail = rail.child(rail_item(theme, section, model.section, actions));
    }
    rail
}

/// One rail entry. Selected is the accent fill; hover is the only other state.
fn rail_item(
    theme: &'static dyn Theme,
    section: Section,
    active: Section,
    actions: &Actions,
) -> Column {
    let actions = actions.clone();
    let selected = section == active;
    let glyph = match section {
        Section::Library => Glyph::Grid,
        Section::Settings => Glyph::Toggle,
    };
    let ink = if selected {
        theme.palette().on_accent
    } else {
        theme.palette().muted
    };
    Column::new()
        .gap(space::XXS)
        .padding(Edges::new(space::XXS, space::SM, space::XXS, space::SM))
        .align(Align::Center)
        .dynamic_background(move |state| {
            let fill = if selected {
                theme.palette().accent
            } else if state.hovered {
                theme.palette().surface_hover
            } else {
                Color::TRANSPARENT
            };
            SurfaceStyle::new(fill).radius(radius::MD)
        })
        .on_click(move || actions.push(Action::Show(section)))
        .child(Icon::new(glyph, theme).size(20.0).color(ink))
        .child(
            Text::caption(section.label(), theme)
                .color(ink)
                .max_lines(1),
        )
}

/// The middle column: a fixed-width panel holding the current page.
fn middle(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &Actions,
    middle_scroll: &mut Option<ScrollViewState>,
) -> Panel {
    let page = match model.section {
        Section::Library => library_page(theme, model, actions, middle_scroll),
        Section::Settings => settings_page(theme, model, actions, middle_scroll),
    };
    Panel::new()
        .color(theme.palette().surface_raised)
        .flat()
        .basis(SizeBasis::Px(MIDDLE_WIDTH))
        .shrink(0.0)
        .clip(true)
        .mouse_filter(MouseFilter::Ignore)
        .child(page.grow(1.0))
}

/// The console column, on the right and always mounted.
fn play_column(theme: &'static dyn Theme, model: &ViewModel, actions: &Actions) -> Column {
    let mut column = Column::new()
        .gap(space::SM)
        .padding(Edges::all(space::MD))
        .grow(1.0)
        .mouse_filter(MouseFilter::Ignore);

    // A game launched from `--rom` may not be in the library list, so the
    // title falls back to the core name rather than the selected row.
    let title = model
        .selected
        .and_then(|index| model.games.get(index))
        .map(|game| game.title.clone())
        .or_else(|| (!model.core_name.is_empty()).then(|| model.core_name.clone()))
        .unwrap_or_else(|| "没有选中游戏".to_string());
    column = column.child(Text::subheading(title, theme).max_lines(1).ellipsis(true));

    // The framebuffer itself: it grows into the remaining space and centres a
    // letterboxed picture inside whatever rectangle it gets.
    match &model.frame {
        Some(frame) => {
            column =
                column.child(FrameImage::new(frame.texture, frame.width, frame.height).grow(1.0));
        }
        None => {
            column = column.child(
                Flex::row()
                    .align(Align::Center)
                    .justify(Justify::Center)
                    .grow(1.0)
                    .surface(SurfaceStyle::new(theme.palette().surface).radius(radius::MD))
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

    if !model.core_name.is_empty() {
        column = column
            .child(Text::caption(format!("核心：{}", model.core_name), theme).tone(Tone::Muted));
    }
    column
}

fn library_page(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &Actions,
    library_scroll: &mut Option<ScrollViewState>,
) -> Column {
    let mut column = Column::new()
        .gap(space::SM)
        .padding(Edges::all(MD))
        .mouse_filter(MouseFilter::Ignore);
    column = column.child(Text::heading("游戏库", theme));

    if model.games.is_empty() {
        column = column.child(
            EmptyState::new("还没有游戏", theme)
                .description("把 ROM 拖进来，或点“添加游戏文件…”。"),
        );
    } else {
        // Only the grid scrolls; the title and the add buttons stay put.
        let view = ScrollView::new(theme)
            .scrollbar(false)
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
        .gap(space::SM)
        .padding(Edges::ZERO);
    for (index, game) in model.games.iter().enumerate() {
        grid = grid.child(game_card(theme, model, game, index, actions));
    }
    grid
}

/// One library cell: the cover, then the name and console under it. The whole
/// cell is the button; the playing game is marked like a Finder selection.
fn game_card(
    theme: &'static dyn Theme,
    model: &ViewModel,
    game: &GameRow,
    index: usize,
    actions: &Actions,
) -> Column {
    let actions = actions.clone();
    let playing = model.selected == Some(index);
    Column::new()
        .gap(space::XXS)
        .padding(Edges::all(space::XXS))
        .dynamic_background(move |state| {
            let fill = if playing {
                theme.palette().selection
            } else if state.hovered {
                theme.palette().surface_hover
            } else {
                Color::TRANSPARENT
            };
            SurfaceStyle::new(fill).radius(radius::MD)
        })
        .on_click(move || actions.push(Action::Play(index)))
        .child(cover(theme, game))
        .child(
            Row::new()
                .gap(space::XXS)
                .child(
                    Text::small(game.title.as_str(), theme)
                        .grow(1.0)
                        .max_lines(1)
                        .ellipsis(true),
                )
                .child(Text::caption(game.system.short(), theme).tone(Tone::Subtle)),
        )
}

/// A card's cover. There is no artwork yet, so the placeholder is a block of
/// surface colour with the game's name clipped into it — the old front end's
/// blank cover. A screenshot slot replaces this later (see `frame.rs`).
fn cover(theme: &'static dyn Theme, game: &GameRow) -> Flex {
    Flex::row()
        .align(Align::Center)
        .justify(Justify::Center)
        .padding(Edges::all(space::XS))
        .min_size(0.0, PLACEHOLDER_HEIGHT)
        .surface(SurfaceStyle::new(theme.palette().surface).radius(radius::SM))
        .child(
            Text::caption(game.title.as_str(), theme)
                .tone(Tone::Muted)
                .max_lines(3)
                .ellipsis(true),
        )
}

/// The bottom status line: the last app message, plus the save hotkeys.
fn status_bar(theme: &'static dyn Theme, model: &ViewModel) -> Column {
    let status = if model.status.is_empty() {
        "就绪"
    } else {
        model.status.as_str()
    };
    Column::new()
        .gap(0.0)
        .child(Divider::horizontal(theme))
        .child(
            Row::new()
                .align(Align::Center)
                .gap(space::SM)
                .padding(Edges::new(space::MD, space::XXS, space::MD, space::XXS))
                .min_size(0.0, 22.0)
                .child(
                    Text::caption(status, theme)
                        .tone(Tone::Muted)
                        .grow(1.0)
                        .max_lines(1)
                        .ellipsis(true),
                )
                .child(Text::caption("F5 存档 / F6 读档", theme).tone(Tone::Subtle)),
        )
}

fn settings_page(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &Actions,
    scroll: &mut Option<ScrollViewState>,
) -> Column {
    let mut column = Column::new()
        .gap(space::MD)
        .padding(Edges::all(space::MD))
        .mouse_filter(MouseFilter::Ignore)
        .child(Text::title("设置", theme));
    let mut body = Column::new()
        .gap(space::MD)
        .mouse_filter(MouseFilter::Ignore);

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
    body = body.child(
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
    body = body.child(
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
    body = body.child(
        Card::new(theme)
            .gap(space::SM)
            .padding(Edges::all(space::SM))
            .child(Text::subheading("按键", theme))
            .child(bindings)
            .child(Text::caption("手柄走 gilrs 自动映射；重绑定后置。", theme).tone(Tone::Subtle)),
    );

    // Only the settings bodies scroll; the title stays put.
    let view = ScrollView::new(theme).grow(1.0).scrollbar(false).child(body);
    *scroll = Some(view.state());
    column = column.child(view);

    column
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BindingRow, CoreRow, FrameHandle, GameRow};
    use cgb_systems::SystemId;
    use draw_core::{InputEvent, PointerButton, Size, Vec2};
    use draw_render::{DrawCommand, PaintContext, TextureId};
    use draw_theme::{default_theme, Mode};

    /// Build, lay out (running the scroll sync) and paint the shell. Returns the
    /// mounted tree so a test can also route input at it.
    fn laid_out(model: &ViewModel, actions: &Actions) -> (SceneTree, draw_render::DrawList) {
        let theme = default_theme(Mode::Dark);
        let (mut tree, mut scroll) = build(theme, model, actions);
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
        (tree, ctx.into_draw_list())
    }

    fn text_position(list: &draw_render::DrawList, needle: &str) -> Vec2 {
        list.commands()
            .iter()
            .find_map(|command| match command {
                DrawCommand::DrawText { text, position, .. } if text == needle => Some(*position),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no text {needle:?}"))
    }

    /// A card is a click target: a press and release inside it starts the game.
    #[test]
    fn clicking_a_card_plays_it() {
        let actions = Actions::default();
        let model = ViewModel {
            games: vec![GameRow {
                title: "Game 0".to_string(),
                system: SystemId::Nes,
                path: "/roms/game0.nes".to_string(),
            }],
            ..ViewModel::default()
        };
        let (mut tree, list) = laid_out(&model, &actions);
        let point = text_position(&list, "Game 0");

        let hit = draw_ui::hit_test(&tree, point).expect("something is under the card");
        assert!(
            draw_ui::is_interactive(&tree, hit),
            "the card is interactive"
        );
        draw_ui::handle_input(
            &mut tree,
            &InputEvent::PointerDown {
                position: point,
                button: PointerButton::Left,
            },
        );
        draw_ui::handle_input(
            &mut tree,
            &InputEvent::PointerUp {
                position: point,
                button: PointerButton::Left,
            },
        );
        assert_eq!(actions.drain(), vec![Action::Play(0)]);
    }

    /// The rail is the only way to change what the middle column shows, so it
    /// has to be clickable too (icon + label inside a clickable cell).
    #[test]
    fn clicking_the_rail_switches_section() {
        let actions = Actions::default();
        let model = ViewModel::default();
        let (mut tree, list) = laid_out(&model, &actions);
        let point = text_position(&list, "设置");
        draw_ui::handle_input(
            &mut tree,
            &InputEvent::PointerDown {
                position: point,
                button: PointerButton::Left,
            },
        );
        draw_ui::handle_input(
            &mut tree,
            &InputEvent::PointerUp {
                position: point,
                button: PointerButton::Left,
            },
        );
        assert!(actions.drain().contains(&Action::Show(Section::Settings)));
    }

    fn paint(model: &ViewModel) -> draw_render::DrawList {
        let actions = Actions::default();
        laid_out(model, &actions).1
    }

    /// The play column is always mounted, so a loaded frame emits the image
    /// command the wgpu backend turns into a texture blit — even while the
    /// middle column shows the library.
    #[test]
    fn the_play_column_emits_a_draw_image() {
        let theme = default_theme(Mode::Dark);
        let actions = Actions::default();
        let model = ViewModel {
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
        let destination = destination.expect("the play column draws the framebuffer");
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

    /// The shell is three columns: the library grid is in the middle column,
    /// and the console column is entirely to its right.
    #[test]
    fn the_shell_is_three_columns() {
        let model = ViewModel {
            games: vec![GameRow {
                title: "Game 0".to_string(),
                system: SystemId::Nes,
                path: "/roms/game0.nes".to_string(),
            }],
            playing: true,
            frame: Some(FrameHandle {
                texture: TextureId::new(1),
                width: 256,
                height: 240,
            }),
            ..ViewModel::default()
        };
        let list = paint(&model);
        let cover_x = list
            .commands()
            .iter()
            .find_map(|command| match command {
                DrawCommand::DrawText { text, position, .. } if text == "Game 0" => {
                    Some(position.x)
                }
                _ => None,
            })
            .expect("the card paints the name");
        let image_left = list
            .commands()
            .iter()
            .find_map(|command| match command {
                DrawCommand::DrawImage { destination, .. } => Some(destination.left()),
                _ => None,
            })
            .expect("the console paints the frame");

        let middle_end = RAIL_WIDTH + MIDDLE_WIDTH;
        assert!(
            cover_x < middle_end,
            "the grid sits in the middle column: {cover_x}"
        );
        assert!(
            image_left >= middle_end,
            "the console is right of the middle column: {image_left} < {middle_end}"
        );
    }

    /// The settings bodies scroll when they are taller than the middle column.
    #[test]
    fn the_settings_page_scrolls_when_it_overflows() {
        let theme = default_theme(Mode::Dark);
        let actions = Actions::default();
        let cores: Vec<CoreRow> = (0..40)
            .map(|index| CoreRow {
                key: format!("core{index}"),
                name: format!("Core {index}"),
                system: SystemId::Nes,
                selected: index == 0,
            })
            .collect();
        let model = ViewModel {
            section: Section::Settings,
            cores,
            ..ViewModel::default()
        };
        let (mut tree, mut scroll) = build(theme, &model, &actions);
        let viewport = draw_core::ViewportSize::new(Size::new(1100.0, 760.0));
        draw_ui::layout(&mut tree, viewport);
        tree.update();
        let scroll = scroll.as_mut().expect("the settings page has a ScrollView");
        scroll.sync(&mut tree);
        assert!(
            scroll.content_height() > scroll.viewport_height(),
            "the bodies overflow, so they scroll: {} vs {}",
            scroll.content_height(),
            scroll.viewport_height()
        );
    }
}

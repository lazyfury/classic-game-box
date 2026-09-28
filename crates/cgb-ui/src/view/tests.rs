//! The view's tests. They build the tree, lay it out, route input at it and
//! inspect the painted draw list, so the whole "ViewModel → tree → pixels" path
//! is checked without a window. Each feature module keeps its behaviour here;
//! the globs below gather them so a test can call any page's internals.

use super::*;

use super::components::*;
use super::library::*;

use crate::model::{
    BindingRow, CheatRow, Confirm, CoreRow, EditKind, EditState, FrameHandle, GameRow, SaveSlotRow,
    ScreenshotRow, ShaderKind, SortKey, SystemCount,
};
use crate::theme::ThemeChoice;
use cgb_systems::SystemId;
use igui::igui_components::{Component, Flex};
use igui::igui_core::{InputEvent, PointerButton, Size, Vec2};
use igui::igui_render::{DrawCommand, PaintContext, TextureId};
use igui::igui_theme::{default_theme, Mode};

/// Build, lay out (running the scroll sync) and paint the shell. Returns the
/// mounted tree so a test can also route input at it.
fn laid_out(model: &ViewModel, actions: &Actions) -> (SceneTree, igui::igui_render::DrawList) {
    let theme = default_theme(Mode::Dark);
    let width = Rc::new(Cell::new(model.middle_width));
    let (mut tree, mut scroll) = build(
        theme,
        model,
        actions,
        &width,
        &NodeRef::new(),
        &Cell::new((0, 0)),
        &NodeRef::new(),
    );
    let viewport = igui::igui_core::ViewportSize::new(Size::new(1100.0, 760.0));
    igui::igui_ui::layout(&mut tree, viewport);
    tree.update();
    if let Some(scroll) = scroll.as_mut() {
        if scroll.sync(&mut tree) {
            igui::igui_ui::layout(&mut tree, viewport);
            tree.update();
        }
    }
    let mut ctx = PaintContext::new();
    igui::igui_ui::paint(&tree, &mut ctx);
    (tree, ctx.into_draw_list())
}

fn text_position(list: &igui::igui_render::DrawList, needle: &str) -> Vec2 {
    list.commands()
        .iter()
        .find_map(|command| match command {
            DrawCommand::DrawText { text, position, .. } if text == needle => Some(*position),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no text {needle:?}"))
}

/// A minimal library row for the view tests.
fn game_row(name: &str, path: &str) -> GameRow {
    GameRow {
        id: 0,
        name: name.to_string(),
        file_name: path.rsplit('/').next().unwrap_or(path).to_string(),
        system: SystemId::Nes,
        path: path.to_string(),
        size: 0,
        pinned: false,
        play_count: 0,
        play_seconds: 0,
        last_played_at: 0,
        tags: Vec::new(),
        screenshots: 0,
        cover: None,
    }
}

/// A card is a click target: a press and release inside it starts the game.
#[test]
fn clicking_a_card_plays_it() {
    let actions = Actions::default();
    let model = ViewModel {
        games: vec![game_row("Game 0", "/roms/game0.nes")],
        ..ViewModel::default()
    };
    let (mut tree, list) = laid_out(&model, &actions);
    let point = text_position(&list, "Game 0");

    let hit = igui::igui_ui::hit_test(&tree, point).expect("something is under the card");
    assert!(
        igui::igui_ui::is_interactive(&tree, hit),
        "the card is interactive"
    );
    igui::igui_ui::handle_input(
        &mut tree,
        &InputEvent::PointerDown {
            position: point,
            button: PointerButton::Left,
        },
    );
    igui::igui_ui::handle_input(
        &mut tree,
        &InputEvent::PointerUp {
            position: point,
            button: PointerButton::Left,
        },
    );
    assert_eq!(actions.drain(), vec![Action::Play(0)]);
}

/// A right click on a card opens its context menu at the pointer.
#[test]
fn right_clicking_a_card_opens_its_context_menu() {
    let actions = Actions::default();
    let model = one_game();
    let (mut tree, list) = laid_out(&model, &actions);
    let point = text_position(&list, "Game 0");
    for event in [
        InputEvent::PointerDown {
            position: point,
            button: PointerButton::Right,
        },
        InputEvent::PointerUp {
            position: point,
            button: PointerButton::Right,
        },
    ] {
        igui::igui_ui::handle_input(&mut tree, &event);
    }
    match actions.drain().as_slice() {
        [Action::GameContextMenu { index, .. }] => assert_eq!(*index, 0),
        other => panic!("expected a context-menu action, got {other:?}"),
    }
}

/// The card controls register the tooltips the host shows on hover.
#[test]
fn card_controls_register_tooltips() {
    let actions = Actions::default();
    let mut game = one_game_row();
    game.screenshots = 2;
    let (_tree, _list) = isolated_cover(&game, &actions);
    let tips: Vec<String> = actions
        .take_tips()
        .into_iter()
        .map(|(_, text)| text)
        .collect();
    for expected in ["截图", "改名", "标签", "置顶", "删除"] {
        assert!(
            tips.iter().any(|tip| tip == expected),
            "missing tip {expected}: {tips:?}"
        );
    }
}

/// The settings core pick-up raises an action naming the console, so the
/// host can open the picker menu.
#[test]
fn the_core_picker_raises_an_action() {
    let actions = Actions::default();
    let model = ViewModel {
        section: Section::Settings,
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
        ..ViewModel::default()
    };
    let (mut tree, list) = laid_out(&model, &actions);
    let point = text_position(&list, "Mesen");
    click(&mut tree, point);
    match actions.drain().as_slice() {
        [Action::OpenCoreMenu { system, .. }] => assert_eq!(*system, SystemId::Nes),
        other => panic!("expected a core-picker action, got {other:?}"),
    }
}

/// Press and release at `point`, the way a mouse click arrives.
fn click(tree: &mut SceneTree, point: Vec2) {
    for event in [
        InputEvent::PointerDown {
            position: point,
            button: PointerButton::Left,
        },
        InputEvent::PointerUp {
            position: point,
            button: PointerButton::Left,
        },
    ] {
        igui::igui_ui::handle_input(tree, &event);
    }
}

fn one_game() -> ViewModel {
    ViewModel {
        games: vec![game_row("Game 0", "/roms/game0.nes")],
        ..ViewModel::default()
    }
}

/// Lay out a single card cover (no shell chrome around it), so its two
/// icon buttons can be located by the lines their SVGs draw.
fn isolated_cover(game: &GameRow, actions: &Actions) -> (SceneTree, igui::igui_render::DrawList) {
    let theme = default_theme(Mode::Dark);
    let mut tree = SceneTree::new();
    let root = tree.root();
    tree.add_child(
        root,
        Flex::column()
            .gap(0.0)
            .padding(Edges::ZERO)
            .mouse_filter(MouseFilter::Ignore)
            .child(cover(theme, game, 0, actions)),
    );
    igui::igui_ui::layout(
        &mut tree,
        igui::igui_core::ViewportSize::new(Size::new(320.0, 240.0)),
    );
    tree.update();
    let mut ctx = PaintContext::new();
    igui::igui_ui::paint(&tree, &mut ctx);
    (tree, ctx.into_draw_list())
}

/// The centres of the two SVG icons in a cover, split left/right by their
/// drawn lines: the pin is left, the delete is right.
/// Cluster the icon line endpoints into `n` groups by the widest x gaps
/// and return each group's centre, left to right. The card's controls are
/// pencil, tag, pin, delete in that order.
fn icon_centres(list: &igui::igui_render::DrawList, n: usize) -> Vec<Vec2> {
    let mut points: Vec<Vec2> = Vec::new();
    for command in list.commands() {
        if let DrawCommand::Line { from, to, .. } = command {
            points.push(*from);
            points.push(*to);
        }
    }
    assert!(!points.is_empty(), "the cover drew no icon lines");
    points.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap());
    let mut gaps: Vec<(usize, f32)> = (1..points.len())
        .map(|index| (index, points[index].x - points[index - 1].x))
        .collect();
    gaps.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let mut cuts: Vec<usize> = gaps
        .into_iter()
        .take(n - 1)
        .map(|(index, _)| index)
        .collect();
    cuts.sort_unstable();
    let mut groups: Vec<&[Vec2]> = Vec::new();
    let mut start = 0;
    for cut in cuts {
        groups.push(&points[start..cut]);
        start = cut;
    }
    groups.push(&points[start..]);
    groups
        .iter()
        .map(|group| {
            let count = group.len() as f32;
            Vec2::new(
                group.iter().map(|p| p.x).sum::<f32>() / count,
                group.iter().map(|p| p.y).sum::<f32>() / count,
            )
        })
        .collect()
}

fn one_game_row() -> GameRow {
    one_game().games.remove(0)
}

/// The pin icon is its own button and toggles the pin.
#[test]
fn clicking_pin_toggles_it() {
    let actions = Actions::default();
    let (mut tree, list) = isolated_cover(&one_game_row(), &actions);
    let centres = icon_centres(&list, 4);
    click(&mut tree, centres[2]);
    assert_eq!(actions.drain(), vec![Action::TogglePin(0)]);
}

/// The pencil and tag icons start a name / tag edit.
#[test]
fn clicking_rename_and_tag_start_edits() {
    let actions = Actions::default();
    let (mut tree, list) = isolated_cover(&one_game_row(), &actions);
    let centres = icon_centres(&list, 4);
    click(&mut tree, centres[0]);
    assert_eq!(actions.drain(), vec![Action::StartRename(0)]);
    click(&mut tree, centres[1]);
    assert_eq!(actions.drain(), vec![Action::StartTagEdit(0)]);
}

/// The delete icon is its own button: it deletes, it does not start the
/// game (the nearest callback wins over the card's play callback).
#[test]
fn clicking_delete_deletes_without_playing() {
    let actions = Actions::default();
    let (mut tree, list) = isolated_cover(&one_game_row(), &actions);
    let centres = icon_centres(&list, 4);
    click(&mut tree, centres[3]);
    assert_eq!(
        actions.drain(),
        vec![Action::RequestDelete(Confirm::DeleteGame(0))]
    );
}

/// The sort bar offers every key and the direction toggle.
#[test]
fn the_sort_bar_switches_the_key() {
    let actions = Actions::default();
    let (mut tree, list) = laid_out(&ViewModel::default(), &actions);
    let point = text_position(&list, "大小");
    click(&mut tree, point);
    assert_eq!(actions.drain(), vec![Action::Sort(SortKey::Size)]);
}

/// The stats bar shows the total and one chip per console, and a chip
/// filters the library to that console.
#[test]
fn the_stats_bar_counts_each_console_and_filters() {
    let actions = Actions::default();
    let model = ViewModel {
        total_games: 3,
        system_counts: vec![
            SystemCount {
                system: SystemId::Nes,
                count: 1,
            },
            SystemCount {
                system: SystemId::Gba,
                count: 2,
            },
        ],
        ..ViewModel::default()
    };
    let (mut tree, list) = laid_out(&model, &actions);
    let has = |needle: &str| {
        list.commands().iter().any(|command| {
            matches!(command,
                    DrawCommand::DrawText { text, .. } if text.contains(needle))
        })
    };
    assert!(has("共 3 个游戏"), "the total is shown");
    // The filter and sort chips share the toolbar shape, so they line up.
    let filter_chip = text_position(&list, "全部");
    let sort_chip = text_position(&list, "名称");
    assert!(
        (filter_chip.x - sort_chip.x).abs() < 1.0,
        "filter and sort chips align: {} vs {}",
        filter_chip.x,
        sort_chip.x
    );
    let point = text_position(&list, "GBA 2");
    click(&mut tree, point);
    assert_eq!(
        actions.drain(),
        vec![Action::FilterSystem(Some(SystemId::Gba))]
    );
}

/// Covers are a stable colour per path, so a game keeps its colour between
/// runs; different games get different colours.
#[test]
fn cover_colours_are_stable_and_varied() {
    assert_eq!(cover_color("/roms/a.nes"), cover_color("/roms/a.nes"));
    assert_ne!(cover_color("/roms/a.nes"), cover_color("/roms/b.nes"));
}

/// A card's tag line shows a few words then a count.
#[test]
fn tags_label_lists_a_few_words() {
    let mut game = game_row("Game 0", "/roms/game0.nes");
    game.tags = [
        "RPG".to_string(),
        "Action".to_string(),
        "Long".to_string(),
        "Extra".to_string(),
    ]
    .to_vec();
    assert_eq!(tags_label(&game), "#RPG #Action #Long +1");
    game.tags.truncate(2);
    assert_eq!(tags_label(&game), "#RPG #Action");
}

/// A card with a cover texture draws it (the only image when no game is
/// playing).
#[test]
fn a_card_with_a_cover_draws_its_texture() {
    let mut game = game_row("Game 0", "/roms/game0.nes");
    game.cover = Some(FrameHandle {
        texture: TextureId::new(0x1000),
        width: 256,
        height: 240,
    });
    let model = ViewModel {
        games: vec![game],
        ..ViewModel::default()
    };
    let list = paint(&model);
    assert!(list
        .commands()
        .iter()
        .any(|command| matches!(command, DrawCommand::DrawImage { .. })));
}

/// The meta line reads the file size and the play count/time.
#[test]
fn card_meta_reads_size_and_play_time() {
    assert_eq!(format_size(0), "0 B");
    assert_eq!(format_size(1536), "2 KB");
    assert_eq!(format_size(3 * 1024 * 1024), "3.0 MB");
    assert_eq!(format_duration(45), "45 秒");
    assert_eq!(format_duration(90), "1 分");
    assert_eq!(format_duration(3660), "1 时 1 分");

    let mut game = game_row("Game 0", "/roms/game0.nes");
    assert!(meta_label(&game).contains("未玩过"));
    game.play_count = 3;
    game.play_seconds = 120;
    game.size = 2048;
    let meta = meta_label(&game);
    assert!(meta.contains("2 KB"), "{meta}");
    assert!(meta.contains("玩过 3 次"), "{meta}");
    assert!(meta.contains("2 分"), "{meta}");
}

fn one_shot() -> ScreenshotRow {
    ScreenshotRow {
        id: 7,
        game_id: 0,
        game: "Game 0".to_string(),
        created_at: 0,
        is_cover: true,
        thumb: Some(FrameHandle {
            texture: TextureId::new(0x1_0000),
            width: 256,
            height: 240,
        }),
    }
}

/// The screenshots section lists the game, its shot count, the cover badge
/// and the thumbnail image.
#[test]
fn the_screenshots_page_lists_the_game_and_its_shots() {
    let model = ViewModel {
        section: Section::Screenshots,
        screenshot_game: Some(0),
        games: vec![game_row("Game 0", "/roms/game0.nes")],
        screenshots: vec![one_shot()],
        ..ViewModel::default()
    };
    let list = paint(&model);
    let has = |needle: &str| {
        list.commands().iter().any(|command| {
            matches!(command,
                    DrawCommand::DrawText { text, .. } if text.contains(needle))
        })
    };
    assert!(has("截图收藏"), "the page title");
    assert!(has("Game 0"), "the game name");
    assert!(has("1 张"), "the count");
    assert!(has("封面"), "the cover flag");
    assert!(
        list.commands()
            .iter()
            .any(|command| matches!(command, DrawCommand::DrawImage { .. })),
        "the thumbnail is drawn"
    );
}

/// The play column shows the previewed screenshot and its index.
#[test]
fn the_preview_column_shows_the_picture_and_its_index() {
    let model = ViewModel {
        preview: Some(7),
        games: vec![game_row("Game 0", "/roms/game0.nes")],
        screenshots: vec![one_shot()],
        ..ViewModel::default()
    };
    let list = paint(&model);
    let has = |needle: &str| {
        list.commands().iter().any(|command| {
            matches!(command,
                    DrawCommand::DrawText { text, .. } if text.contains(needle))
        })
    };
    assert!(has("1 / 1"), "the position");
    assert!(
        list.commands()
            .iter()
            .any(|command| matches!(command, DrawCommand::DrawImage { .. })),
        "the preview is drawn"
    );
}

/// The card's screenshot count is its own button that opens the section.
#[test]
fn the_card_screenshot_count_opens_the_section() {
    let actions = Actions::default();
    let mut game = game_row("Game 0", "/roms/game0.nes");
    game.screenshots = 2;
    let model = ViewModel {
        games: vec![game],
        ..ViewModel::default()
    };
    let (mut tree, list) = laid_out(&model, &actions);
    let point = text_position(&list, "2");
    click(&mut tree, point);
    assert_eq!(actions.drain(), vec![Action::ShowScreenshots(0)]);
}

/// The rail has a screenshots entry that switches the section.
#[test]
fn the_rail_offers_the_screenshots_section() {
    let actions = Actions::default();
    let (mut tree, list) = laid_out(&ViewModel::default(), &actions);
    let point = text_position(&list, "截图");
    click(&mut tree, point);
    assert!(actions
        .drain()
        .contains(&Action::Show(Section::Screenshots)));
}

/// Clicking a card's delete control asks the host to confirm the delete.
#[test]
fn clicking_delete_asks_to_confirm() {
    let actions = Actions::default();
    let (mut tree, list) = isolated_cover(&one_game_row(), &actions);
    let centres = icon_centres(&list, 4);
    click(&mut tree, centres[3]);
    assert_eq!(
        actions.drain(),
        vec![Action::RequestDelete(Confirm::DeleteGame(0))]
    );
}

/// An error status is painted in the error colour, a success in success.
#[test]
fn the_status_line_colours_by_severity() {
    let theme = default_theme(Mode::Dark);
    let colour = |model: &ViewModel| {
        let actions = Actions::default();
        let (_tree, list) = laid_out(model, &actions);
        list.commands().iter().find_map(|command| match command {
            DrawCommand::DrawText { text, paint, .. } if text == &model.status => Some(paint.color),
            _ => None,
        })
    };

    let mut model = ViewModel::default();
    model.set_status("读取 ROM 失败", StatusKind::Error);
    assert_eq!(colour(&model), Some(theme.palette().error));

    model.set_status("已存档", StatusKind::Success);
    assert_eq!(colour(&model), Some(theme.palette().success));
}

/// The edit bar shows a `TextInput` with the pending value, and its buttons
/// commit or cancel the edit.
#[test]
fn the_edit_bar_shows_the_field_and_commits() {
    let actions = Actions::default();
    actions.set_edit(Rc::new(RefCell::new(TextEdit::new("New Name"))));
    let model = ViewModel {
        editing: Some(EditState {
            game_id: 0,
            kind: EditKind::Name,
        }),
        ..ViewModel::default()
    };
    let (mut tree, list) = laid_out(&model, &actions);
    let has = |needle: &str| {
        list.commands().iter().any(|command| {
            matches!(command,
                    DrawCommand::DrawText { text, .. } if text == needle)
        })
    };
    assert!(has("改名"), "the bar is labelled");
    assert!(has("New Name"), "the field renders the pending value");

    click(&mut tree, text_position(&list, "保存"));
    assert_eq!(actions.drain(), vec![Action::CommitEdit]);
    click(&mut tree, text_position(&list, "取消"));
    assert_eq!(actions.drain(), vec![Action::CancelEdit]);
}

/// The field takes focus when it mounts (autofocus): it paints a caret and
/// routes committed text into the shared edit state, and the caret follows.
#[test]
fn the_edit_field_focuses_and_accepts_text() {
    fn caret(list: &igui::igui_render::DrawList) -> Option<igui::igui_core::Rect> {
        list.commands().iter().find_map(|command| match command {
            DrawCommand::FillRect { rect, .. } if (rect.size.width - 1.5).abs() < 0.01 => {
                Some(*rect)
            }
            _ => None,
        })
    }

    let actions = Actions::default();
    actions.set_edit(Rc::new(RefCell::new(TextEdit::new("ab"))));
    let model = ViewModel {
        editing: Some(EditState {
            game_id: 0,
            kind: EditKind::Name,
        }),
        ..ViewModel::default()
    };
    let (mut tree, list) = laid_out(&model, &actions);
    let before = caret(&list).expect("the focused field paints a caret");

    igui::igui_ui::handle_input(&mut tree, &InputEvent::TextInput { text: "c".into() });
    assert_eq!(
        actions.edit_text(),
        "abc",
        "committed text reaches the edit"
    );

    let mut ctx = PaintContext::new();
    igui::igui_ui::paint(&tree, &mut ctx);
    let after = caret(&ctx.into_draw_list()).expect("the caret is still painted");
    assert!(
        after.origin.x > before.origin.x,
        "the caret follows the inserted text"
    );
}

/// The search bar opens the field, and shows the current query.
#[test]
fn the_search_bar_starts_and_reflects_the_query() {
    let actions = Actions::default();
    let (mut tree, list) = laid_out(&ViewModel::default(), &actions);
    click(&mut tree, text_position(&list, "搜索名称 / #标签…"));
    assert_eq!(actions.drain(), vec![Action::StartSearch]);

    let model = ViewModel {
        search: "mario".to_string(),
        ..ViewModel::default()
    };
    let list = paint(&model);
    assert!(list.commands().iter().any(|command| matches!(command,
            DrawCommand::DrawText { text, .. } if text.contains("mario"))));

    // While the search edit is open, the row mounts the text field (with its
    // 完成 / 清除 buttons) instead of the plain button.
    let actions = Actions::default();
    actions.set_edit(Rc::new(RefCell::new(TextEdit::new("mario"))));
    let model = ViewModel {
        editing: Some(EditState {
            game_id: -1,
            kind: EditKind::Search,
        }),
        ..ViewModel::default()
    };
    let (_, list) = laid_out(&model, &actions);
    let has = |needle: &str| {
        list.commands()
            .iter()
            .any(|command| matches!(command, DrawCommand::DrawText { text, .. } if text == needle))
    };
    assert!(has("完成"), "the search field's confirm button is shown");
    assert!(has("清除"), "the search field's clear button is shown");
    assert!(has("mario"), "the field renders the current query");
}

/// The saves section lists the slots and its buttons emit the slot actions.
#[test]
fn the_saves_page_lists_slots_and_emits_actions() {
    let actions = Actions::default();
    let model = ViewModel {
        section: Section::Saves,
        playing: true,
        core_name: "Mesen".to_string(),
        saves_supported: true,
        saves: vec![
            SaveSlotRow {
                slot: 0,
                exists: true,
                modified_ms: 0,
                thumb: None,
            },
            SaveSlotRow {
                slot: 1,
                exists: false,
                modified_ms: 0,
                thumb: None,
            },
        ],
        ..ViewModel::default()
    };
    let (mut tree, list) = laid_out(&model, &actions);
    let has = |needle: &str| {
        list.commands().iter().any(|command| {
            matches!(command,
                    DrawCommand::DrawText { text, .. } if text.contains(needle))
        })
    };
    assert!(has("槽 1"));
    assert!(has("槽 2"));

    click(&mut tree, text_position(&list, "存"));
    assert_eq!(actions.drain(), vec![Action::SaveToSlot(0)]);
    click(&mut tree, text_position(&list, "读"));
    assert_eq!(actions.drain(), vec![Action::LoadFromSlot(0)]);
    click(&mut tree, text_position(&list, "删"));
    assert_eq!(actions.drain(), vec![Action::DeleteSlot(0)]);
}

/// The rail has a save section entry.
#[test]
fn the_rail_offers_the_saves_section() {
    let actions = Actions::default();
    let (mut tree, list) = laid_out(&ViewModel::default(), &actions);
    click(&mut tree, text_position(&list, "存档"));
    assert!(actions.drain().contains(&Action::Show(Section::Saves)));
}

/// The cheats page lists cheats, toggles one, and offers the import.
#[test]
fn the_cheats_page_lists_and_toggles() {
    let actions = Actions::default();
    let model = ViewModel {
        section: Section::Cheats,
        playing: true,
        core_name: "Mesen".to_string(),
        cheats: vec![
            CheatRow {
                desc: "Infinite Lives".to_string(),
                code: "AAAA".to_string(),
                enabled: true,
            },
            CheatRow {
                desc: "Max Coins".to_string(),
                code: "BBBB".to_string(),
                enabled: false,
            },
        ],
        ..ViewModel::default()
    };
    let (mut tree, list) = laid_out(&model, &actions);
    let has = |needle: &str| {
        list.commands().iter().any(|command| {
            matches!(command,
                    DrawCommand::DrawText { text, .. } if text.contains(needle))
        })
    };
    assert!(has("Infinite Lives"));
    assert!(has("AAAA"));

    click(&mut tree, text_position(&list, "开"));
    assert_eq!(actions.drain(), vec![Action::ToggleCheat(0)]);
    click(&mut tree, text_position(&list, "导入 .cht…"));
    assert_eq!(actions.drain(), vec![Action::ImportCheats]);
}

/// The rail has a cheats entry.
#[test]
fn the_rail_offers_the_cheats_section() {
    let actions = Actions::default();
    let (mut tree, list) = laid_out(&ViewModel::default(), &actions);
    click(&mut tree, text_position(&list, "金手指"));
    assert!(actions.drain().contains(&Action::Show(Section::Cheats)));
}

/// The rail is the only way to change what the middle column shows, so it
/// has to be clickable too (icon + label inside a clickable cell).
#[test]
fn clicking_the_rail_switches_section() {
    let actions = Actions::default();
    let model = ViewModel::default();
    let (mut tree, list) = laid_out(&model, &actions);
    let point = text_position(&list, "设置");
    igui::igui_ui::handle_input(
        &mut tree,
        &InputEvent::PointerDown {
            position: point,
            button: PointerButton::Left,
        },
    );
    igui::igui_ui::handle_input(
        &mut tree,
        &InputEvent::PointerUp {
            position: point,
            button: PointerButton::Left,
        },
    );
    assert!(actions.drain().contains(&Action::Show(Section::Settings)));
}

fn paint(model: &ViewModel) -> igui::igui_render::DrawList {
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

    let width = Rc::new(Cell::new(model.middle_width));
    let (mut tree, _) = build(
        theme,
        &model,
        &actions,
        &width,
        &NodeRef::new(),
        &Cell::new((0, 0)),
        &NodeRef::new(),
    );
    igui::igui_ui::layout(
        &mut tree,
        igui::igui_core::ViewportSize::new(Size::new(1100.0, 760.0)),
    );
    tree.update();

    let mut ctx = PaintContext::new();
    igui::igui_ui::paint(&tree, &mut ctx);
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

/// A paused game shows a large centered "暂停" label over the picture, in the
/// windowed play column and in fullscreen alike.
#[test]
fn a_paused_game_shows_a_centered_pause_label() {
    let actions = Actions::default();
    let model = ViewModel {
        playing: true,
        paused: true,
        frame: Some(FrameHandle {
            texture: TextureId::new(1),
            width: 256,
            height: 240,
        }),
        ..ViewModel::default()
    };
    let has_pause = |list: &igui::igui_render::DrawList| {
        list.commands().iter().any(|command| {
            matches!(command,
                DrawCommand::DrawText { text, .. } if text == "暂停")
        })
    };
    let (_, list) = laid_out(&model, &actions);
    assert!(has_pause(&list), "the paused play column shows the label");

    let mut fullscreen = model.clone();
    fullscreen.fullscreen = true;
    let (_, list) = laid_out(&fullscreen, &actions);
    assert!(has_pause(&list), "fullscreen shows the same label");
}

/// While a fullscreen transition settles, the UI is hidden behind a black
/// "loading" surface (and the shell is not mounted).
#[test]
fn a_hidden_ui_paints_a_black_surface() {
    let actions = Actions::default();
    let model = ViewModel {
        ui_hidden: true,
        ..ViewModel::default()
    };
    let (_, list) = laid_out(&model, &actions);
    let black = list.commands().iter().any(|command| match command {
        DrawCommand::FillRect { paint, .. } | DrawCommand::FillRoundedRect { paint, .. } => {
            paint.color == igui::igui_core::Color::BLACK
        }
        _ => false,
    });
    assert!(black, "the hidden UI paints a black surface");
    assert!(
        !list.commands().iter().any(|command| matches!(command,
                DrawCommand::DrawText { text, .. } if text == "游戏库")),
        "the shell is not mounted while hidden"
    );
}

/// Fullscreen reuses the right-column play view: the shell (header, rail,
/// library) is not mounted, but the title, live info, picture and controls are.
#[test]
fn fullscreen_play_hides_the_shell_but_keeps_the_picture() {
    let actions = Actions::default();
    let model = ViewModel {
        playing: true,
        fullscreen: true,
        selected: Some(0),
        games: vec![game_row("Game 0", "/roms/game0.nes")],
        frame: Some(FrameHandle {
            texture: TextureId::new(1),
            width: 256,
            height: 240,
        }),
        ..ViewModel::default()
    };
    let (mut tree, list) = laid_out(&model, &actions);
    assert!(
        list.commands()
            .iter()
            .any(|command| matches!(command, DrawCommand::DrawImage { .. })),
        "the framebuffer is painted"
    );
    // The library / rail is not mounted, but the play view (title + controls)
    // is, because fullscreen reuses it.
    let hidden = ["游戏库", "设置", "金手指"];
    for needle in hidden {
        assert!(
            !list.commands().iter().any(|command| matches!(command,
                    DrawCommand::DrawText { text, .. } if text == needle)),
            "{needle:?} should not be mounted in fullscreen"
        );
    }
    assert!(
        list.commands().iter().any(|command| matches!(command,
                DrawCommand::DrawText { text, .. } if text == "复位")),
        "the play controls are mounted in fullscreen"
    );
    click(&mut tree, text_position(&list, "退出全屏"));
    assert_eq!(actions.drain(), vec![Action::ToggleFullscreen]);
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
fn the_settings_shader_presets_emit_actions() {
    let actions = Actions::default();
    let model = ViewModel {
        section: Section::Settings,
        shader: ShaderKind::Off,
        ..ViewModel::default()
    };
    let (mut tree, list) = laid_out(&model, &actions);
    click(&mut tree, text_position(&list, "CRT"));
    assert_eq!(actions.drain(), vec![Action::SetShader(ShaderKind::Crt)]);
}

#[test]
fn the_settings_core_options_cycle() {
    let actions = Actions::default();
    let model = ViewModel {
        section: Section::Settings,
        core_options: vec![crate::model::CoreOptionRow {
            key: "region".to_string(),
            label: "Region".to_string(),
            values: vec![
                ("auto".to_string(), "Auto".to_string()),
                ("ntsc".to_string(), "NTSC".to_string()),
            ],
            value: "auto".to_string(),
        }],
        ..ViewModel::default()
    };
    let (mut tree, list) = laid_out(&model, &actions);
    let has = |needle: &str| {
        list.commands().iter().any(|command| {
            matches!(command,
                    DrawCommand::DrawText { text, .. } if text.contains(needle))
        })
    };
    assert!(has("Region"));
    click(&mut tree, text_position(&list, "›"));
    assert_eq!(actions.drain(), vec![Action::CycleCoreOption(0, 1)]);
}

#[test]
fn the_settings_page_shows_the_library_and_cores() {
    let model = ViewModel {
        section: Section::Settings,
        library_root: Some("/roms/nes".to_string()),
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
    assert!(has("/roms/nes"), "the library folder is shown");
    assert!(has("Mesen"), "the selected core is shown in the pull-down");
    assert!(has("X / K"), "the binding is shown");
}

/// The appearance section switches the theme family and the light / dark look.
#[test]
fn the_settings_page_switches_the_theme_and_appearance() {
    let actions = Actions::default();
    let model = ViewModel {
        section: Section::Settings,
        ..ViewModel::default()
    };
    let (mut tree, list) = laid_out(&model, &actions);
    click(&mut tree, text_position(&list, "浅色"));
    assert_eq!(actions.drain(), vec![Action::SetLight(true)]);
    click(&mut tree, text_position(&list, "默认"));
    assert_eq!(
        actions.drain(),
        vec![Action::SetThemeChoice(ThemeChoice::Default)]
    );
}

/// The grid mounts a window of rows, not the whole library.
/// The number of grid columns steps 2 / 3 / 4 as the middle column widens,
/// and never leaves that range.
#[test]
fn library_columns_steps_with_the_middle_width() {
    assert_eq!(library_columns(MIDDLE_MIN_WIDTH), MIN_LIBRARY_COLUMNS);
    assert_eq!(library_columns(MIDDLE_MAX_WIDTH), MAX_LIBRARY_COLUMNS);
    assert!(
        (MIN_LIBRARY_COLUMNS..=MAX_LIBRARY_COLUMNS).contains(&library_columns(400.0)),
        "a mid width picks a valid count"
    );
    assert!(library_columns(400.0) <= library_columns(600.0));
}

#[test]
fn visible_rows_windows_the_grid() {
    // 10 rows, 100px stride, 250px viewport: the visible rows plus slack.
    assert_eq!(visible_rows(0.0, 250.0, 10, 100.0), (0, 4));
    assert_eq!(visible_rows(500.0, 250.0, 10, 100.0), (5, 9));
    // Past the end clamps to the last row.
    assert_eq!(visible_rows(5000.0, 250.0, 10, 100.0), (9, 9));
    // Nothing to show.
    assert_eq!(visible_rows(0.0, 250.0, 0, 100.0), (0, 0));
}

/// The mounted window only changes when a row boundary is crossed, so a
/// scroll smaller than one row needs no rebuild.
#[test]
fn the_grid_window_only_changes_when_a_row_is_crossed() {
    let mut model = ViewModel {
        games: (0..40)
            .map(|index| game_row(&format!("Game {index}"), &format!("/roms/game{index}.nes")))
            .collect(),
        grid_viewport: 400.0,
        ..ViewModel::default()
    };
    let start = grid_window(&model);
    // A few pixels into the same row: same window.
    model.grid_offset = 20.0;
    assert_eq!(grid_window(&model), start);
    // Past a row boundary (`CARD_HEIGHT + space::SM = 192`): new window.
    model.grid_offset = 200.0;
    assert_ne!(grid_window(&model), start);
    assert_eq!(grid_window(&model), (1, 5));
}

/// The library is a grid, not a list: the first `grid_columns` cells share
/// a row (increasing x, same baseline) and the next one wraps to a new row
/// below.
#[test]
fn the_library_page_lays_games_out_in_a_grid() {
    let columns = ViewModel::default().grid_columns;
    let games: Vec<GameRow> = (0..columns + 1)
        .map(|index| game_row(&format!("Game {index}"), &format!("/roms/game{index}.nes")))
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
    let wrapped = position(&format!("Game {columns}")).expect("the wrapped card paints its title");

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
        games: vec![game_row("Game 0", "/roms/game0.nes")],
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
            DrawCommand::DrawText { text, position, .. } if text == "Game 0" => Some(position.x),
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

    let middle_end = RAIL_WIDTH + 320.0;
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
    let bindings: Vec<BindingRow> = (0..40)
        .map(|index| BindingRow {
            button: format!("按键 {index}"),
            keys: "X / K".to_string(),
        })
        .collect();
    let model = ViewModel {
        section: Section::Settings,
        bindings,
        ..ViewModel::default()
    };
    let width = Rc::new(Cell::new(model.middle_width));
    let (mut tree, mut scroll) = build(
        theme,
        &model,
        &actions,
        &width,
        &NodeRef::new(),
        &Cell::new((0, 0)),
        &NodeRef::new(),
    );
    let viewport = igui::igui_core::ViewportSize::new(Size::new(1100.0, 760.0));
    igui::igui_ui::layout(&mut tree, viewport);
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

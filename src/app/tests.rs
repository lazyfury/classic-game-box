use super::*;

fn db_game(name: &str, size: u64, pinned: bool) -> Game {
    Game {
        id: 0,
        path: format!("/{name}.nes"),
        file_name: format!("{name}.nes"),
        name: name.to_string(),
        system: cgb_systems::SystemId::Nes,
        size,
        mtime_ms: 0,
        added_at: 0,
        last_played_at: 0,
        play_count: 0,
        play_seconds: 0,
        pinned,
        cover: None,
        screenshots: 0,
        tags: Vec::new(),
    }
}

#[test]
fn a_hash_token_searches_tags() {
    let mut game = db_game("Super Mario", 1, false);
    game.tags = vec!["RPG".to_string(), "经典".to_string()];
    assert!(game_matches_search(&game, ""));
    assert!(game_matches_search(&game, "mario"));
    assert!(game_matches_search(&game, "#rpg"), "a #tag matches a tag");
    assert!(game_matches_search(&game, "#经典"), "CJK tags match too");
    assert!(game_matches_search(&game, "mario #RPG"), "AND-ed tokens");
    assert!(
        !game_matches_search(&game, "mario #action"),
        "a missing tag fails the match"
    );
    assert!(!game_matches_search(&game, "zelda"));
}

#[test]
fn order_games_keeps_pinned_first_then_sorts() {
    let names = |games: &[Game]| -> Vec<String> { games.iter().map(|g| g.name.clone()).collect() };
    let mut games = vec![
        db_game("b", 10, false),
        db_game("a", 30, false),
        db_game("c", 20, true),
    ];
    order_games(&mut games, SortKey::Name, false);
    assert_eq!(names(&games), ["c", "a", "b"]);

    order_games(&mut games, SortKey::Size, true);
    assert_eq!(names(&games), ["c", "a", "b"]);

    order_games(&mut games, SortKey::Size, false);
    assert_eq!(names(&games), ["c", "b", "a"]);
}

#[test]
fn system_counts_tally_each_console_in_systems_order() {
    let mut a = db_game("a", 1, false);
    let mut b = db_game("b", 1, false);
    let mut c = db_game("c", 1, false);
    a.system = SystemId::Gb;
    b.system = SystemId::Nes;
    c.system = SystemId::Nes;
    let counts = system_counts(&[a, b, c]);
    // NES comes before GB in `SYSTEMS`, and consoles with no games are left
    // out.
    assert_eq!(counts.len(), 2);
    assert_eq!(counts[0].system, SystemId::Nes);
    assert_eq!(counts[0].count, 2);
    assert_eq!(counts[1].system, SystemId::Gb);
    assert_eq!(counts[1].count, 1);
    assert!(system_counts(&[]).is_empty());
}

#[test]
fn save_state_hotkeys_match_the_old_layout() {
    assert_eq!(state_shortcut(Key::F5, false), Some(Action::SaveToSlot(0)));
    assert_eq!(
        state_shortcut(Key::F6, false),
        Some(Action::LoadFromSlot(0))
    );
    assert_eq!(state_shortcut(Key::F1, false), Some(Action::SaveToSlot(1)));
    assert_eq!(state_shortcut(Key::F1, true), Some(Action::LoadFromSlot(1)));
    assert_eq!(state_shortcut(Key::F4, false), None);
    assert_eq!(
        state_shortcut(Key::F11, false),
        Some(Action::ToggleFullscreen)
    );
}

#[test]
fn import_status_reports_copies_and_skips() {
    let mut report = ImportReport {
        copied: vec![PathBuf::from("/lib/mario.nes")],
        ..ImportReport::default()
    };
    assert!(import_status(&report).contains("已添加 1 个游戏"));

    report.unknown.push(PathBuf::from("/tmp/notes.txt"));
    let message = import_status(&report);
    assert!(message.contains("已添加 1 个游戏"), "{message}");
    assert!(message.contains("跳过 1 个"), "{message}");

    let already = ImportReport {
        already_inside: vec![PathBuf::from("/lib/mario.nes")],
        ..ImportReport::default()
    };
    assert!(import_status(&already).contains("已经在游戏库里"));

    let unknown = ImportReport {
        unknown: vec![PathBuf::from("/tmp/notes.txt")],
        ..ImportReport::default()
    };
    assert!(import_status(&unknown).contains("跳过不认识的 ROM"));
    assert!(import_status(&ImportReport::default()).contains("没有新增游戏"));
}

#[test]
fn inside_library_recognises_the_folder_and_its_children() {
    let root = std::env::temp_dir().join(format!("cgb-inside-{}", std::process::id()));
    let inside = root.join("nes");
    let outside = root.parent().unwrap().join("somewhere-else");
    assert!(inside_library(&root, &root));
    assert!(inside_library(&root, &inside));
    assert!(!inside_library(&root, &outside));
}

fn temp_paths(name: &str) -> Paths {
    let root = std::env::temp_dir().join(format!("cgb-app-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let paths = Paths::under(&root);
    paths.ensure().expect("create temp paths");
    paths
}

#[test]
fn the_manifest_is_read_from_the_packaged_cores_dir() {
    let paths = temp_paths("manifest");
    std::fs::write(
        paths.cores.join("cores.json"),
        r#"{ "cores": [ { "key": "x", "system": "nes", "dylib": "x.dylib" } ] }"#,
    )
    .expect("write manifest");

    let cores = load_core_manifest(&paths);
    assert!(cores.iter().any(|core| core.key == "x"));
    let _ = std::fs::remove_dir_all(&paths.root);
}

#[test]
fn a_downloaded_core_is_merged_into_the_manifest() {
    let paths = temp_paths("downloaded");
    std::fs::write(
            paths.cores.join("cores.json"),
            r#"{ "cores": [ { "key": "snes9x", "system": "snes", "dylib": "snes9x_libretro.dylib" } ] }"#,
        )
        .expect("write manifest");
    // The registry carries libretro's `systemid` (`super_nes`) and also
    // duplicates the shipped `snes9x` row.
    std::fs::write(
            paths.cores.join("downloaded.json"),
            r#"{ "cores": [
                { "key": "bsnes", "name": "bsnes", "system": "super_nes", "dylib": "bsnes_libretro.dylib" },
                { "key": "snes9x", "system": "snes", "dylib": "snes9x_libretro.dylib" }
            ] }"#,
        )
        .expect("write registry");

    let cores = load_core_manifest(&paths);
    // The new core is offered for its console…
    assert!(cores
        .iter()
        .any(|core| core.key == "bsnes" && core.system == SystemId::Snes));
    // …and a downloaded row duplicating a shipped (system, key) stays once.
    assert_eq!(
        cores
            .iter()
            .filter(|core| core.key == "snes9x" && core.system == SystemId::Snes)
            .count(),
        1
    );
    let _ = std::fs::remove_dir_all(&paths.root);
}

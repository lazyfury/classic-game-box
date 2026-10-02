use super::*;

fn db_game(name: &str, size: u64, pinned: bool) -> Game {
    Game {
        id: 0,
        path: format!("/{name}.nes"),
        file_name: format!("{name}.nes"),
        name: name.to_string(),
        system: cgb_libretro::SystemId::Nes,
        core: None,
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
    // F5/F6 act on the rolling quick stack; the numbered keys stay manual.
    assert_eq!(state_shortcut(Key::F5, false), Some(Action::QuickSave));
    assert_eq!(state_shortcut(Key::F6, false), Some(Action::LoadQuick(0)));
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
    // The loader only offers cores whose module exists, so give `x` one.
    std::fs::write(paths.cores.join("x.dylib"), b"").expect("write module");

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
    // Both rows resolve to a module, so the loader keeps them.
    std::fs::write(paths.cores.join("snes9x_libretro.dylib"), b"").expect("write snes9x module");
    std::fs::write(paths.cores.join("bsnes_libretro.dylib"), b"").expect("write bsnes module");

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

#[test]
fn a_core_whose_module_is_missing_is_not_offered() {
    let paths = temp_paths("missing-module");
    std::fs::write(
        paths.cores.join("cores.json"),
        r#"{ "cores": [
            { "key": "here", "system": "nes", "dylib": "here.dylib" },
            { "key": "gone", "system": "gba", "dylib": "gone.dylib" }
        ] }"#,
    )
    .expect("write manifest");
    std::fs::write(paths.cores.join("here.dylib"), b"").expect("write module");

    let cores = load_core_manifest(&paths);
    assert!(cores.iter().any(|core| core.key == "here"));
    assert!(!cores.iter().any(|core| core.key == "gone"));
    let _ = std::fs::remove_dir_all(&paths.root);
}

#[test]
fn a_blocked_core_is_never_offered() {
    let paths = temp_paths("blocked");
    // A blocked core registered before the blocklist existed is dropped.
    std::fs::write(
        paths.cores.join("downloaded.json"),
        r#"{ "cores": [ { "key": "squirreljme", "system": "j2me", "dylib": "squirreljme_libretro.dylib" } ] }"#,
    )
    .expect("write registry");
    std::fs::write(paths.cores.join("squirreljme_libretro.dylib"), b"").expect("write module");

    assert!(!load_core_manifest(&paths)
        .iter()
        .any(|core| core.key == "squirreljme"));
    let _ = std::fs::remove_dir_all(&paths.root);
}

fn spec(key: &str, system: SystemId) -> CoreSpec {
    CoreSpec {
        key: key.to_string(),
        name: key.to_string(),
        system,
        module: PathBuf::from(format!("{key}_libretro.dylib")),
        sample_rate: 0,
        frame_seconds: 1.0 / 60.0,
        option_defaults: Default::default(),
    }
}

fn catalog(entries: &[(&str, &str)]) -> Catalog {
    Catalog {
        generated: String::new(),
        source: String::new(),
        cores: entries
            .iter()
            .map(|(name, system)| crate::cores::CatalogEntry {
                name: name.to_string(),
                display_name: name.to_string(),
                system: system.to_string(),
                extensions: String::new(),
            })
            .collect(),
    }
}

#[test]
fn recommend_core_prefers_the_manifest_choice_then_the_catalog() {
    let catalog = catalog(&[("snes9x", "super_nes"), ("bsnes", "super_nes")]);
    let shipped = vec![
        spec("bsnes", SystemId::Snes),
        spec("snes9x", SystemId::Snes),
    ];
    // The manifest's first downloadable core wins.
    assert_eq!(
        recommend_core(&shipped, &catalog, SystemId::Snes),
        Some("bsnes".to_string())
    );
    // A console the manifest does not name falls back to the catalog.
    assert_eq!(
        recommend_core(&[], &catalog, SystemId::Snes),
        Some("snes9x".to_string())
    );
    // Nothing downloadable serves it.
    assert_eq!(recommend_core(&[], &catalog, SystemId::J2me), None);
}

#[test]
fn recommend_core_skips_blocked_cores() {
    // SquirrelJME is the only catalog J2ME core, but it is blocked, so the app
    // must offer no recommendation for J2ME rather than suggest it.
    let catalog = catalog(&[("squirreljme", "j2me")]);
    assert_eq!(recommend_core(&[], &catalog, SystemId::J2me), None);
    let shipped = vec![spec("squirreljme", SystemId::J2me)];
    assert_eq!(recommend_core(&shipped, &catalog, SystemId::J2me), None);
}

#[test]
fn missing_core_rows_lists_consoles_without_an_available_core() {
    let catalog = catalog(&[("snes9x", "super_nes"), ("mgba", "game_boy_advance")]);
    let shipped = vec![spec("snes9x", SystemId::Snes), spec("mgba", SystemId::Gba)];
    // Only NES has an available core; SNES and GBA are missing.
    let available = vec![spec("mesen", SystemId::Nes)];
    let mut snes = db_game("snes", 1, false);
    snes.system = SystemId::Snes;
    let mut gba = db_game("gba", 1, false);
    gba.system = SystemId::Gba;
    let games = vec![snes, gba, db_game("nes", 1, false)];

    assert_eq!(
        missing_core_rows(&games, &available, &shipped, &catalog),
        vec![
            MissingCoreRow {
                system: SystemId::Snes,
                core: "snes9x".to_string(),
            },
            MissingCoreRow {
                system: SystemId::Gba,
                core: "mgba".to_string(),
            },
        ]
    );
}

/// The "missing core" card used to appear only after a core download: a
/// refactor dropped the `rebuild_missing_cores()` call from the rescan path.
/// `rebuild_settings_view` runs at startup and on every rescan, so the card is
/// current as soon as games are loaded.
#[test]
fn rebuild_settings_view_refreshes_the_missing_core_card() {
    let root = std::env::temp_dir().join(format!("cgb-missing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut app = App::with_paths(Paths::under(&root), Args::default(), false);
    // A NES game with no available core (the release ships none) must show up
    // immediately, without waiting for a download.
    app.cores.clear();
    app.game_source = vec![db_game("mario", 1, false)];
    app.model.missing_cores.clear();
    app.rebuild_settings_view();
    assert_eq!(
        app.model.missing_cores.first().map(|row| row.system),
        Some(SystemId::Nes),
        "rebuild_settings_view refreshes the missing-core list"
    );
    let _ = std::fs::remove_dir_all(&root);
}

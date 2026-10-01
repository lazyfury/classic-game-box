//! A headless release gate: `--selfcheck`.
//!
//! It exercises the plumbing a windowed run cannot easily verify — paths, the
//! library schema and its round trips, settings, the core manifest and icon
//! rasterization — in a temp directory, prints one line per check and exits
//! non-zero if any hard check fails. It never opens a window and leaves
//! nothing behind.
//!
//! Core dylibs are third-party build products that may not be present in a
//! fresh checkout, so a missing module is a warning, not a failure; the
//! manifest failing to parse or being empty is a failure.

use std::path::{Path, PathBuf};

use crate::cores::load_cores;
use crate::library::{encode_png, DiskGame, Library};
use crate::paths::{Paths, Settings};
use crate::ui::{rasterize_icon, IconName};
use cgb_libretro::SystemId;

use crate::app::resource_dir;

/// Run every check; return the process exit code.
pub fn run() -> i32 {
    let root = std::env::temp_dir().join(format!("cgb-selfcheck-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let paths = Paths::under(&root);

    let mut failed = 0usize;
    let mut warn = 0usize;
    type Check = fn(&Paths) -> Result<(), String>;
    let checks: [(&str, Check); 5] = [
        ("paths", check_paths),
        ("library", check_library),
        ("settings", check_settings),
        ("icons", check_icons),
        ("render", check_render),
    ];
    for (name, check) in checks {
        match check(&paths) {
            Ok(()) => println!("ok    {name}"),
            Err(error) => {
                println!("FAIL  {name}: {error}");
                failed += 1;
            }
        }
    }

    // The core manifest: a hard check for the file, a warning per missing dylib.
    match check_cores(&paths) {
        Ok((declared, missing)) => {
            println!("ok    cores manifest ({declared} declared)");
            for module in missing {
                println!("warn  core dylib not built: {module}");
                warn += 1;
            }
        }
        Err(error) => {
            println!("FAIL  cores manifest: {error}");
            failed += 1;
        }
    }

    let _ = std::fs::remove_dir_all(&root);
    println!();
    if failed == 0 {
        println!("selfcheck: ok ({warn} warnings)");
        0
    } else {
        println!("selfcheck: {failed} failed, {warn} warnings");
        1
    }
}

fn check_paths(paths: &Paths) -> Result<(), String> {
    paths.ensure().map_err(|error| error.to_string())?;
    for dir in [&paths.root, &paths.system, &paths.saves, &paths.screenshots] {
        if !dir.is_dir() {
            return Err(format!("{} was not created", dir.display()));
        }
    }
    Ok(())
}

fn check_library(paths: &Paths) -> Result<(), String> {
    let library = Library::open(&paths.library_db).map_err(|error| error.to_string())?;
    let disk = |path: &str| DiskGame {
        path: path.to_string(),
        file_name: Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string()),
        system: SystemId::Nes,
        size: 1,
        mtime_ms: 0,
    };
    library
        .sync(&[disk("/a.nes"), disk("/b.nes")])
        .map_err(|error| error.to_string())?;
    if library.games().map_err(|e| e.to_string())?.len() != 2 {
        return Err("sync did not insert two games".to_string());
    }

    library
        .rename("/a.nes", "Alpha")
        .map_err(|e| e.to_string())?;
    library
        .set_pinned("/a.nes", true)
        .map_err(|e| e.to_string())?;
    library
        .note_played("/b.nes", 42)
        .map_err(|e| e.to_string())?;
    library
        .note_playtime("/b.nes", 30)
        .map_err(|e| e.to_string())?;
    library
        .set_tags("/b.nes", &["RPG".to_string()])
        .map_err(|e| e.to_string())?;

    let games = library.games().map_err(|e| e.to_string())?;
    let a = games.iter().find(|g| g.path == "/a.nes").expect("/a.nes");
    let b = games.iter().find(|g| g.path == "/b.nes").expect("/b.nes");
    if a.name != "Alpha" || !a.pinned {
        return Err("rename / pin did not stick".to_string());
    }
    if b.play_count != 1 || b.play_seconds != 30 || b.tags != ["RPG"] {
        return Err("play stats / tags did not stick".to_string());
    }

    // A screenshot round trip: first shot becomes the cover, then is deleted.
    let png = encode_png(
        2,
        2,
        &[0, 0, 0, 255, 1, 1, 1, 255, 2, 2, 2, 255, 3, 3, 3, 255],
    )
    .map_err(|error| error.to_string())?;
    let shot = library
        .save_screenshot("/b.nes", &png, 2, 2, false)
        .map_err(|e| e.to_string())?
        .ok_or("screenshot was not saved")?;
    let b = library
        .games()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|g| g.path == "/b.nes")
        .expect("/b.nes");
    if b.cover != Some(shot.id) {
        return Err("the first screenshot did not become the cover".to_string());
    }
    library
        .remove_screenshot(shot.id)
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn check_settings(paths: &Paths) -> Result<(), String> {
    let settings = Settings {
        library_root: Some("/roms".to_string()),
        ..Settings::default()
    };
    settings
        .save(&paths.settings_json)
        .map_err(|error| error.to_string())?;
    let reloaded = Settings::load(&paths.settings_json);
    if reloaded.library_folder() != Some("/roms") {
        return Err("settings did not round-trip".to_string());
    }
    Ok(())
}

fn check_icons(_paths: &Paths) -> Result<(), String> {
    for name in IconName::ALL {
        let (_, _, rgba) =
            rasterize_icon(name, 32).ok_or_else(|| format!("{name:?} did not rasterize"))?;
        if rgba.chunks(4).all(|pixel| pixel[3] == 0) {
            return Err(format!("{name:?} rasterized blank"));
        }
    }
    Ok(())
}

/// Build the library page, lay it out and paint it into a `DrawList`, then run
/// the structural inspector. This exercises the view → tree → paint path with
/// no window (the `igui` recording route), and asserts a semantic anchor so the
/// check is not just "it drew something".
fn check_render(_paths: &Paths) -> Result<(), String> {
    use crate::ui::{game_theme, Ui, ViewBridge, ViewModel};
    use igui::igui_core::{Size, ViewportSize};
    use igui::igui_profile::{inspect, FrameCounters, FrameStats, Severity, StageTimes};
    use igui::igui_render::{DrawCommand, PaintContext};
    use igui::igui_theme::Mode;

    let theme = game_theme(Mode::Dark);
    let actions = ViewBridge::default();
    let model = ViewModel {
        games: (0..40).map(sample_game).collect(),
        // The app feeds the measured viewport back after the first layout; the
        // virtualized grid uses it to size the mounted row window.
        grid_viewport: 760.0,
        ..ViewModel::default()
    };
    let mut ui = Ui::new(theme, &model, &actions);
    let viewport = ViewportSize::new(Size::new(1100.0, 760.0));
    ui.layout(viewport);
    let mut ctx = PaintContext::new();
    ui.paint(&mut ctx);
    let list = ctx.into_draw_list();
    if list.is_empty() {
        return Err("the library page painted nothing".to_string());
    }
    let stats = FrameStats {
        index: 0,
        frame_ms: 0.0,
        stages: StageTimes::new(0.0, 0.0, 0.0, 0.0),
        counters: FrameCounters::new(ui.tree().node_count(), 0, list.commands().len(), 1),
    };
    let report = inspect(&list, &stats);
    if report.count_of(Severity::Error) > 0 {
        let finding = report
            .findings()
            .iter()
            .find(|finding| finding.severity == Severity::Error)
            .expect("an error finding");
        return Err(format!("draw list inspector: {}", finding.summary()));
    }
    let labelled = list.commands().iter().any(
        |command| matches!(command, DrawCommand::DrawText { text, .. } if text.contains("游戏库")),
    );
    if !labelled {
        return Err("the library title was not painted".to_string());
    }
    Ok(())
}

/// A minimal library row for the render check.
fn sample_game(index: usize) -> crate::ui::GameRow {
    crate::ui::GameRow {
        id: index as i64,
        name: format!("Game {index}"),
        file_name: format!("game{index}.nes"),
        system: SystemId::Nes,
        path: format!("/roms/game{index}.nes"),
        size: (index as u64 + 1) * 4096,
        pinned: index % 17 == 0,
        play_count: (index % 9) as i64,
        play_seconds: (index % 3600) as i64,
        last_played_at: (index as i64) * 1000,
        tags: Vec::new(),
        screenshots: 0,
        cover: None,
    }
}

/// Parse the core manifest, returning how many cores it declares and which
/// declared modules are not built.
fn check_cores(paths: &Paths) -> Result<(usize, Vec<String>), String> {
    let manifest = core_manifest_path(paths).ok_or("no cores.json found")?;
    let cores = load_cores(&manifest);
    if cores.is_empty() {
        return Err(format!("{} declares no cores", manifest.display()));
    }
    let missing = cores
        .iter()
        .filter(|core| !resolve_module(paths, &core.module).is_file())
        .map(|core| core.module.display().to_string())
        .collect();
    Ok((cores.len(), missing))
}

fn core_manifest_path(paths: &Paths) -> Option<PathBuf> {
    let packaged = paths.cores.join("cores.json");
    if packaged.is_file() {
        return Some(packaged);
    }
    if let Some(resources) = resource_dir() {
        let bundled = resources.join("cores/cores.json");
        if bundled.is_file() {
            return Some(bundled);
        }
    }
    let dev = PathBuf::from("cores/cores.json");
    dev.is_file().then_some(dev)
}

fn resolve_module(paths: &Paths, module: &Path) -> PathBuf {
    if module.is_absolute() || module.is_file() {
        return module.to_path_buf();
    }
    let packaged = paths.cores.join(module);
    if packaged.is_file() {
        return packaged;
    }
    if let Some(resources) = resource_dir() {
        let bundled = resources.join("cores").join(module);
        if bundled.is_file() {
            return bundled;
        }
    }
    let dev = Path::new("cores/dist").join(module);
    if dev.is_file() {
        return dev;
    }
    packaged
}

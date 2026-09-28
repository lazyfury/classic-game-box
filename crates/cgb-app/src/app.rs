//! The `winit` + `wgpu` host: the frame loop and the wiring between the UI,
//! the libretro core, the audio device and the gamepad.
//!
//! quill is event-driven by default; an emulator is not. While a game is
//! running the loop schedules a redraw at the core's frame rate
//! (`ControlFlow::WaitUntil`); with no game, or when paused, it falls back to
//! `Wait` and does no work. See `docs/architecture/quill-native-migration.md`
//! §8.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use igui::igui_app::{
    App as IguiApp, AppBuilder, AppConfig, AppLogic, EventContext, EventResult, FrameContext,
    InitContext, PlatformEvent, PlatformObserver, Plugin,
};
use igui::igui_backend_wgpu::{FontConfig, FontMode, TextureEffect};
use igui::igui_core::{InputEvent, Key, Modifiers, Rect, ViewportSize};
use igui::igui_profile::{inspect, FrameCounters, FrameStats, Profiler, Severity, StageTimes};
use igui::igui_render::{DrawList, PaintContext, TextureId};
use igui::igui_theme::{Mode, Theme};
use igui::igui_ui::{focused_caret, TextEdit, TextMeasurer};
use igui_winit::{
    ClipboardPlugin, GpuConfig, ImePlugin, KeyboardPlugin, PointerPlugin, SharedBackend,
    SharedWindow, TextMeasurePlugin, TitlebarMode, WgpuPlugin, WindowConfig,
};
use winit::event::WindowEvent;
#[cfg(target_os = "macos")]
use winit::platform::macos::WindowExtMacOS;
#[cfg(not(target_os = "macos"))]
use winit::window::Fullscreen;
use winit::window::Window;

use cgb_input::{Gamepads, InputState, KeyboardBindings};
use cgb_library::{
    collect_games, decode_png, encode_png, import_roms, load_cores, seed_dir, Game, ImportReport,
    Library, Paths, Settings,
};
use cgb_systems::{choose_core, system_for_path, CoreSpec, JoypadButton, SystemId};
use cgb_ui::{
    library_columns, Action, Actions, BindingRow, Confirm, CoreOptionRow, CoreRow, EditKind,
    EditState, FrameHandle, GameRow, InputDescriptorRow, SafeArea, SaveSlotRow, ScreenshotRow,
    Section, ShaderKind, SortKey, StatusKind, SystemCount, ThemeChoice, Ui, ViewModel,
    MIDDLE_MAX_WIDTH, MIDDLE_MIN_WIDTH,
};

use crate::cli::{Args, CoreOverride};
use crate::session::Session;

/// The shortest gap between grid column-count recomputations while the middle
/// divider is dragged. A column change rebuilds the tree; throttling keeps a
/// drag from rebuilding on every pointer move.
const COLUMNS_CHECK_INTERVAL: Duration = Duration::from_millis(80);

/// How often `CGB_PERF` prints the aggregate frame summary (roughly two seconds
/// at 60 FPS).
const PERF_REPORT_FRAMES: u64 = 120;

/// Cover textures start above the game framebuffer's id, one per game.
const COVER_TEXTURE_BASE: u32 = 0x1000;

/// Screenshot thumbnails live in their own id space, above the covers.
const SCREENSHOT_TEXTURE_BASE: u32 = 0x1_0000;

/// Rasterized icon textures, above the screenshots.
const ICON_TEXTURE_BASE: u32 = 0x2_0000;

/// Save-state thumbnails, above the icons, one per slot.
const SAVE_TEXTURE_BASE: u32 = 0x3_0000;

/// The pixel size icons are rasterized at. They are drawn smaller, so the
/// bilinear downscale stays smooth.
const ICON_TEXTURE_PX: u32 = 32;

/// macOS title bar height, in logical points. The window uses a full-size
/// content view, so the UI runs under the title bar and the header must clear
/// this much.
#[cfg(target_os = "macos")]
const MACOS_TITLEBAR_HEIGHT: f32 = 0.0;

/// Room for the macOS traffic lights, which overlay the top-left of the
/// content.
#[cfg(target_os = "macos")]
const MACOS_TRAFFIC_LIGHTS: f32 = 72.0;

/// The chrome the UI must leave for the platform.
fn safe_area() -> SafeArea {
    #[cfg(target_os = "macos")]
    {
        SafeArea {
            top: MACOS_TITLEBAR_HEIGHT,
            left: MACOS_TRAFFIC_LIGHTS,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        SafeArea::ZERO
    }
}

/// Ask the window for fullscreen (or back).
///
/// macOS `Fullscreen::Borderless` puts the window on a separate Space and plays
/// the native animation, which is what sometimes janks/centres. The platform's
/// "simple fullscreen" covers the screen instantly on the same Space; pair it
/// with `set_borderless_game` to hide the menu bar and Dock.
#[cfg(target_os = "macos")]
fn apply_window_fullscreen(window: &Window, on: bool) {
    window.set_borderless_game(on);
    let _ = window.set_simple_fullscreen(on);
}

#[cfg(not(target_os = "macos"))]
fn apply_window_fullscreen(window: &Window, on: bool) {
    window.set_fullscreen(if on {
        Some(Fullscreen::Borderless(None))
    } else {
        None
    });
}

/// A registered cover texture and the screenshot row it came from.
struct CoverTexture {
    cover_id: i64,
    handle: FrameHandle,
}

/// A registered screenshot thumbnail and the file it came from.
struct ScreenshotTexture {
    file: String,
    handle: FrameHandle,
}

/// Arcade BIOS bundled in the checkout (`assets/roms/<system>/system`). The
/// core is pointed at the writable `<app data>/system` directory, so its
/// missing files are seeded from here on startup. Relative to the working
/// directory, like the dev `cores/cores.json` fallback.
const BUNDLED_ARCADE_SYSTEM: &str = "assets/roms/arcade/system";

/// Runs the app on the `igui_app` plugin runtime (winit + wgpu + input).
pub fn run(args: Args) {
    let app = App::new(args);
    let drops: Rc<RefCell<Vec<PathBuf>>> = app.pending_drops.clone();
    let title = "Classic Game Box".to_string();
    let size = (1100.0, 760.0);
    IguiApp::new(AppConfig {
        title: title.clone(),
        size,
        ..Default::default()
    })
    // The window: a transparent, title-less macOS title bar, with the platform
    // IME enabled so the text fields can compose CJK.
    .plugin(igui_winit::WinitPlugin::new(WindowConfig {
        title,
        size,
        titlebar: TitlebarMode::Transparent,
        ime: true,
    }))
    // The surface and backend, published as the `SharedBackend` service.
    .plugin(WgpuPlugin::new(GpuConfig {
        font: FontConfig {
            mode: FontMode::System,
            device_pixel_rasterization: true,
            ..Default::default()
        },
        ..Default::default()
    }))
    .plugin(PointerPlugin)
    .plugin(KeyboardPlugin)
    .plugin(ImePlugin)
    .plugin(TextMeasurePlugin)
    .plugin(ClipboardPlugin)
    .plugin(HostPlugin { drops })
    .logic(app)
    .build()
    .run();
}

/// Bridges host platform events the app cares about into shared flags: files
/// dropped on the window. (Window resizes are picked up from the presenter's
/// viewport, so a repeated resize does not force a redundant re-layout.)
struct HostPlugin {
    drops: Rc<RefCell<Vec<PathBuf>>>,
}

impl Plugin for HostPlugin {
    fn name(&self) -> &'static str {
        "cgb-host"
    }

    fn build(&self, app: &mut AppBuilder) {
        app.add_platform_observer(HostObserver {
            drops: self.drops.clone(),
        });
    }
}

struct HostObserver {
    drops: Rc<RefCell<Vec<PathBuf>>>,
}

impl PlatformObserver for HostObserver {
    fn on_platform(&mut self, event: PlatformEvent<'_>, _out: &mut Vec<InputEvent>) {
        if let Some(WindowEvent::DroppedFile(path)) = event.downcast_ref::<WindowEvent>() {
            self.drops.borrow_mut().push(path.clone());
        }
    }
}

/// A fullscreen toggle waiting for the OS window animation to settle.
///
/// Entering: switch to the lightweight play view first, then ask the window for
/// fullscreen. Leaving: ask the window to leave, keep the play view while it
/// animates, and only rebuild the heavy shell once the size stops changing.
#[derive(Clone, Copy)]
struct FullscreenTransition {
    /// The fullscreen state to apply once the window settles.
    target: bool,
    /// When the window last changed size (or the transition started).
    last_activity: Instant,
}

/// A fullscreen toggle waiting for the black screen to show before the OS
/// animation starts.
#[derive(Clone, Copy)]
struct PendingFullscreen {
    /// The fullscreen state to ask the window for.
    target: bool,
    /// The window request is issued at this time (the black screen shows until
    /// then).
    at: Instant,
}

/// The application state, driven as an [`AppLogic`] by the `igui_app` runtime.
struct App {
    /// The wgpu backend, published by `WgpuPlugin`; `None` until the first
    /// resume creates the window and the surface.
    backend: Option<SharedBackend>,
    /// The window, read from the `SharedWindow` service (fullscreen toggling).
    window: Option<Arc<Window>>,
    /// The backend's real font metrics, published by `TextMeasurePlugin`.
    measurer: Option<Rc<dyn TextMeasurer>>,
    /// Whether this frame must lay out and paint; otherwise the previous draw
    /// list is replayed (a running game only changes its texture).
    repaint: bool,
    /// The current frame's layout time, for `CGB_PERF`.
    last_layout: Duration,
    /// The current frame's paint time, for `CGB_PERF`.
    last_paint: Duration,

    theme: &'static dyn Theme,
    /// The theme family and light/dark appearance in use, so the settings page
    /// can switch them and the choice is remembered.
    theme_choice: ThemeChoice,
    light: bool,
    actions: Actions,
    model: ViewModel,
    ui: Ui,
    /// Set when the model changed and the tree must be rebuilt.
    dirty: bool,
    /// The last painted draw list. Re-submitted when nothing changed (a running
    /// game updates its texture in place), so the UI is not laid out and
    /// painted every frame.
    draw_list: Option<DrawList>,
    /// The `CGB_PERF` frame profiler; `None` when timing is off.
    profiler: Option<Profiler>,

    paths: Paths,
    settings: Settings,
    library: Option<Library>,
    /// Every scanned/added game, before the view sort and pin order. Kept so
    /// sorting and pinning can rebuild the view without touching disk again.
    game_source: Vec<Game>,
    /// Registered cover textures, keyed by game id. Reused across refreshes so
    /// a cover is decoded and uploaded once.
    cover_textures: HashMap<i64, CoverTexture>,
    /// Registered screenshot thumbnails, keyed by screenshot id.
    screenshot_textures: HashMap<i64, ScreenshotTexture>,
    /// Registered save-state thumbnails, keyed by slot, with the modified time
    /// they were uploaded for.
    save_textures: HashMap<u8, (i64, FrameHandle)>,
    /// The running game's cheats, and the `.cht` file they came from.
    cheats: Vec<cgb_library::Cheat>,
    cheat_path: Option<PathBuf>,
    /// The running core's options, cached for the settings page.
    core_options: Vec<cgb_libretro::CoreOption>,
    /// Screenshots ticked for a batch delete.
    selected_shots: Vec<i64>,
    /// Whether the screenshots page is in multi-select mode.
    screenshot_select: bool,
    /// Whether the current screenshot preview paused a running game, so it can
    /// be resumed when the preview closes.
    preview_paused: bool,
    /// The destructive action the open confirmation dialog is asking about.
    pending_confirm: Option<Confirm>,
    input: InputState,
    /// Keyboard bindings, one set per console (a NES and a GBA layout differ).
    bindings: HashMap<SystemId, KeyboardBindings>,
    /// The console whose binding set the keyboard feeds right now.
    active_system: SystemId,
    /// Whether the rewind key is held (the game steps back each frame).
    rewinding: bool,
    /// The game-picture post-process preset.
    shader: ShaderKind,
    gamepads: Option<Gamepads>,
    session: Option<Session>,
    /// `--rom` path to start once the window exists.
    pending_rom: Option<PathBuf>,
    /// `--core` override: a core key or a module path, applied to every game
    /// this run starts.
    core_override: Option<CoreOverride>,
    /// Every core declared in `cores.json`, in manifest order.
    cores: Vec<CoreSpec>,
    /// Keyboard modifiers, so save-state hotkeys can tell save from load.
    modifiers: Modifiers,
    /// Files dropped onto the window since the last frame. The winit runner
    /// delivers one `DroppedFile` event per file (through the host's observer),
    /// so they are buffered and added in one batch.
    pending_drops: Rc<RefCell<Vec<PathBuf>>>,
    /// The viewport the mounted tree was last laid out for. A resize only
    /// forces a re-layout when this actually changes (a fullscreen transition
    /// fires many repeats).
    last_viewport: Option<ViewportSize>,
    /// The settled fullscreen target the window was last asked for (as opposed
    /// to [`ViewModel::fullscreen`], which is what is mounted right now).
    fullscreen_target: bool,
    /// A fullscreen transition in flight: the play view stays mounted until the
    /// window stops resizing, then the target state is applied.
    transition: Option<FullscreenTransition>,
    /// A fullscreen request waiting for the hidden tree to be presented, then a
    /// short black-screen delay, before the OS animates.
    pending_fullscreen: Option<PendingFullscreen>,
    /// Whether the current frame painted, letting the pending request fire on
    /// the next frame.
    hidden_painted: bool,

    /// When the running game's play time was last flushed to the database.
    last_play_flush: Instant,
    /// Emulated frames per second: a short moving average of the core's frame
    /// deltas, shown in the play column.
    fps: f32,
    /// The core frame counter and wall time the current FPS window started at.
    fps_frames: u32,
    fps_time: Instant,
    /// When the grid's column count was last recomputed from the middle width.
    /// Throttles the rebuild a column change triggers while the divider is
    /// dragged.
    last_columns_check: Instant,
}

impl App {
    fn new(args: Args) -> Self {
        // A `--library-dir` overrides the remembered library; app data (and so
        // the settings that would remember it) stays in the platform folder.
        let forced_library = args.library_dir.is_some();
        let mut paths = Paths::platform();
        if let Some(dir) = args.library_dir.clone() {
            paths = Paths::new(paths.user_data.clone(), Some(dir));
        }
        let _ = paths.ensure();
        // Arcade cores need a BIOS. Seed the writable system dir the core
        // actually reads from the bundled assets; a player-supplied file wins.
        // A packaged app keeps its assets in the bundle's Resources.
        let bundled_arcade = resource_dir()
            .map(|resources| resources.join(BUNDLED_ARCADE_SYSTEM))
            .filter(|dir| dir.is_dir())
            .unwrap_or_else(|| PathBuf::from(BUNDLED_ARCADE_SYSTEM));
        let _ = seed_dir(&bundled_arcade, &paths.system);
        let mut settings = Settings::load(&paths.settings_json);
        // Remember where the library is so the next run finds it. An explicit
        // `--library-dir` sticks; so does an install that predates the single
        // library — its first (and only) folder is adopted. With no library
        // chosen the game data just stays in app data, unremembered, so the
        // first folder the player picks becomes the library.
        let root = paths.root.to_string_lossy().into_owned();
        let adopting_legacy_folder =
            settings.library_root.is_none() && !settings.legacy_library_dirs.is_empty();
        let mut settings_changed = false;
        if (forced_library || adopting_legacy_folder)
            && settings.library_root.as_deref() != Some(root.as_str())
        {
            settings.library_root = Some(root);
            settings_changed = true;
        }
        // The theme: a CLI pick (or `--light`) overrides the saved preference
        // and then sticks the same way a settings-page change does.
        if let Some(choice) = args.theme {
            settings.theme = Some(choice.key().to_string());
            settings_changed = true;
        }
        if args.light && !settings.light {
            settings.light = true;
            settings_changed = true;
        }
        if settings_changed {
            let _ = settings.save(&paths.settings_json);
        }
        let theme_choice = settings
            .theme
            .as_deref()
            .and_then(ThemeChoice::parse)
            .unwrap_or_default();
        let light = settings.light;
        let shader = ShaderKind::from_key(&settings.shader);
        let middle_width = if settings.middle_width > 0.0 {
            settings
                .middle_width
                .clamp(MIDDLE_MIN_WIDTH, MIDDLE_MAX_WIDTH)
        } else {
            320.0
        };
        let library = Library::open(&paths.library_db).ok();
        let cores = load_core_manifest(&paths);

        let actions = Actions::default();
        let mode = if light { Mode::Light } else { Mode::Dark };
        let theme = theme_choice.theme(mode);
        let model = ViewModel {
            core_name: "—".to_string(),
            middle_width,
            grid_columns: library_columns(middle_width),
            theme_choice,
            light,
            ..ViewModel::default()
        };
        let ui = Ui::new(theme, &model, &actions);

        let mut app = Self {
            backend: None,
            window: None,
            measurer: None,
            repaint: true,
            last_layout: Duration::ZERO,
            last_paint: Duration::ZERO,
            theme,
            theme_choice,
            light,
            actions,
            model,
            ui,
            dirty: true,
            draw_list: None,
            profiler: std::env::var_os("CGB_PERF").map(|_| Profiler::new()),
            paths,
            settings,
            library,
            game_source: Vec::new(),
            cover_textures: HashMap::new(),
            screenshot_textures: HashMap::new(),
            save_textures: HashMap::new(),
            cheats: Vec::new(),
            cheat_path: None,
            core_options: Vec::new(),
            selected_shots: Vec::new(),
            screenshot_select: false,
            preview_paused: false,
            pending_confirm: None,
            input: InputState::new(),
            bindings: cgb_systems::SYSTEMS
                .iter()
                .map(|system| (*system, KeyboardBindings::default_bindings_for(*system)))
                .collect(),
            active_system: SystemId::Nes,
            rewinding: false,
            shader,
            gamepads: match Gamepads::new() {
                Ok(gamepads) => Some(gamepads),
                Err(error) => {
                    eprintln!("cgb: 手柄不可用：{error}");
                    None
                }
            },
            session: None,
            pending_rom: args.rom,
            core_override: args.core,
            cores,
            modifiers: Modifiers::NONE,
            pending_drops: Rc::new(RefCell::new(Vec::new())),
            last_viewport: None,
            fullscreen_target: false,
            transition: None,
            pending_fullscreen: None,
            hidden_painted: false,
            last_play_flush: Instant::now(),
            fps: 0.0,
            fps_frames: 0,
            fps_time: Instant::now(),
            last_columns_check: Instant::now(),
        };
        app.refresh_library();
        app.rebuild_settings_view();
        app
    }

    /// Re-read the library folders and rebuild the game rows.
    ///
    /// The folder is the truth about what exists, so every refresh rescans it
    /// and reconciles the database: new files are inserted, vanished files are
    /// dropped, changed files have their facts refreshed. The rows then come
    /// from the database, which is the model — the name, pin, play statistics,
    /// screenshots and cover a scan cannot know live there.
    fn refresh_library(&mut self) {
        // The one game library (once chosen) plus its built-in ROM folder.
        // Before a library is chosen the game data lives in app data, so only
        // the built-in folder is scanned there — walking app data itself would
        // trip over the BIOS folder.
        let mut dirs: Vec<PathBuf> = Vec::new();
        if self.paths.has_library() {
            dirs.push(self.paths.root.clone());
        }
        if !dirs.contains(&self.paths.roms) {
            dirs.push(self.paths.roms.clone());
        }

        // Individually added files (dragged in or chosen in the dialog) merge
        // in, and are pruned from the settings once their file is gone.
        let (disk, kept) = collect_games(&dirs, &self.settings.added_roms);
        if kept != self.settings.added_roms {
            self.settings.added_roms = kept;
            let _ = self.settings.save(&self.paths.settings_json);
        }

        self.game_source = match &self.library {
            Some(library) => {
                let _ = library.sync(&disk);
                library.games().unwrap_or_default()
            }
            // No database: fall back to the scan, with no metadata to show.
            None => disk.iter().map(Game::from_disk).collect(),
        };
        self.refresh_cover_textures();
        self.refresh_screenshot_textures();
        self.rebuild_game_rows();
        self.rebuild_screenshot_rows();
    }

    /// Rebuild the view from the database without rescanning the ROM folders.
    ///
    /// Screenshot and cover changes only touch the database and the files under
    /// the screenshots directory, so a full folder scan is wasted work.
    fn reload_from_db(&mut self) {
        self.game_source = self
            .library
            .as_ref()
            .and_then(|library| library.games().ok())
            .unwrap_or_default();
        self.refresh_cover_textures();
        self.refresh_screenshot_textures();
        self.rebuild_game_rows();
        self.rebuild_screenshot_rows();
    }

    /// Decode and upload a texture for each game whose cover changed, reusing
    /// the previous upload when it did not. Covers whose game is gone are
    /// dropped from the cache (their texture stays on the GPU — the backend
    /// has no remove).
    fn refresh_cover_textures(&mut self) {
        let Some(backend) = self.backend.clone() else {
            return;
        };
        let mut backend = backend.borrow_mut();
        let Some(library) = &self.library else {
            return;
        };
        let live: HashSet<i64> = self.game_source.iter().map(|game| game.id).collect();
        self.cover_textures.retain(|id, _| live.contains(id));

        for game in &self.game_source {
            let Some(cover_id) = game.cover else {
                self.cover_textures.remove(&game.id);
                continue;
            };
            if self
                .cover_textures
                .get(&game.id)
                .is_some_and(|cover| cover.cover_id == cover_id)
            {
                continue;
            }
            let Ok(Some(path)) = library.screenshot_path(cover_id) else {
                continue;
            };
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Ok((width, height, rgba)) = decode_png(&bytes) else {
                continue;
            };
            let texture = TextureId::new(COVER_TEXTURE_BASE + game.id as u32);
            if backend
                .register_texture(texture, width, height, &rgba)
                .is_ok()
            {
                self.cover_textures.insert(
                    game.id,
                    CoverTexture {
                        cover_id,
                        handle: FrameHandle {
                            texture,
                            width,
                            height,
                        },
                    },
                );
            }
        }
    }

    /// Decode and upload a texture for every icon, so the UI draws each one as
    /// a single image instead of re-stroking its SVG every frame.
    fn install_icon_textures(&mut self) {
        let Some(backend) = self.backend.clone() else {
            return;
        };
        let mut backend = backend.borrow_mut();
        for (index, name) in cgb_ui::IconName::ALL.into_iter().enumerate() {
            let Some((width, height, rgba)) = cgb_ui::rasterize_icon(name, ICON_TEXTURE_PX) else {
                continue;
            };
            let texture = TextureId::new(ICON_TEXTURE_BASE + index as u32);
            if backend
                .register_texture(texture, width, height, &rgba)
                .is_ok()
            {
                cgb_ui::set_texture(
                    name,
                    FrameHandle {
                        texture,
                        width,
                        height,
                    },
                );
            }
        }
    }

    /// Decode and upload a texture for every screenshot, reusing uploads
    /// whose file is unchanged. The backend has no `remove_texture`, so
    /// deleted screenshots leave their texture behind.
    fn refresh_screenshot_textures(&mut self) {
        let Some(backend) = self.backend.clone() else {
            return;
        };
        let mut backend = backend.borrow_mut();
        let Some(library) = &self.library else {
            return;
        };
        let shots = library.screenshots().unwrap_or_default();
        let live: HashSet<i64> = shots.iter().map(|shot| shot.id).collect();
        self.screenshot_textures.retain(|id, _| live.contains(id));

        for shot in &shots {
            if self
                .screenshot_textures
                .get(&shot.id)
                .is_some_and(|texture| texture.file == shot.file)
            {
                continue;
            }
            let Ok(Some(path)) = library.screenshot_path(shot.id) else {
                continue;
            };
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Ok((width, height, rgba)) = decode_png(&bytes) else {
                continue;
            };
            let texture = TextureId::new(SCREENSHOT_TEXTURE_BASE + shot.id as u32);
            if backend
                .register_texture(texture, width, height, &rgba)
                .is_ok()
            {
                self.screenshot_textures.insert(
                    shot.id,
                    ScreenshotTexture {
                        file: shot.file.clone(),
                        handle: FrameHandle {
                            texture,
                            width,
                            height,
                        },
                    },
                );
            }
        }
    }

    /// Build the screenshots section's rows: the library's screenshots with
    /// their game name, cover flag and thumbnail handle.
    fn rebuild_screenshot_rows(&mut self) {
        let Some(library) = &self.library else {
            self.model.screenshots.clear();
            return;
        };
        let names: HashMap<i64, String> = self
            .game_source
            .iter()
            .map(|game| (game.id, game.name.clone()))
            .collect();
        let covers: HashMap<i64, Option<i64>> = self
            .game_source
            .iter()
            .map(|game| (game.id, game.cover))
            .collect();
        let shots = library.screenshots().unwrap_or_default();
        self.model.screenshots = shots
            .into_iter()
            .map(|shot| ScreenshotRow {
                id: shot.id,
                game_id: shot.game_id,
                game: names.get(&shot.game_id).cloned().unwrap_or_default(),
                created_at: shot.created_at,
                is_cover: covers.get(&shot.game_id).copied().flatten() == Some(shot.id),
                thumb: self
                    .screenshot_textures
                    .get(&shot.id)
                    .map(|texture| texture.handle),
            })
            .collect();
        // A preview whose screenshot is gone (deleted, or its game removed)
        // closes, resuming a game it paused.
        if let Some(id) = self.model.preview {
            if !self.model.screenshots.iter().any(|shot| shot.id == id) {
                self.model.preview = None;
                if self.preview_paused {
                    if let Some(session) = self.session.as_mut() {
                        session.toggle_pause();
                    }
                    self.preview_paused = false;
                }
            }
        }
        self.model.screenshot_select = self.screenshot_select;
        self.model.selected_screenshots = self.selected_shots.clone();
    }

    /// The id of the currently selected (usually playing) game.
    fn selected_game_id(&self) -> Option<i64> {
        self.model
            .selected
            .and_then(|index| self.model.games.get(index))
            .map(|game| game.id)
    }

    /// The screenshots the section currently shows: the game's, newest first.
    fn visible_screenshots(&self) -> Vec<i64> {
        let game_id = self
            .model
            .screenshot_game
            .or_else(|| self.selected_game_id());
        self.model
            .screenshots
            .iter()
            .filter(|shot| Some(shot.game_id) == game_id)
            .map(|shot| shot.id)
            .collect()
    }

    /// Show a screenshot large in the play column, pausing a running game.
    fn preview_screenshot(&mut self, id: i64) {
        self.model.preview = Some(id);
        if let Some(session) = self.session.as_mut() {
            if !session.paused() {
                session.toggle_pause();
                self.preview_paused = true;
                self.flush_playtime();
            }
        }
        self.dirty = true;
    }

    /// Close the preview, resuming a game this preview paused.
    fn close_preview(&mut self) {
        self.model.preview = None;
        if self.preview_paused {
            if let Some(session) = self.session.as_mut() {
                session.toggle_pause();
            }
            self.preview_paused = false;
        }
        self.dirty = true;
    }

    /// Step the preview to the next (`+1`) or previous (`-1`) screenshot.
    fn step_preview(&mut self, delta: i32) {
        let ids = self.visible_screenshots();
        if ids.is_empty() {
            return;
        }
        let current = self
            .model
            .preview
            .and_then(|id| ids.iter().position(|candidate| *candidate == id))
            .unwrap_or(0);
        let last = ids.len() as i32 - 1;
        let next = (current as i32 + delta).clamp(0, last) as usize;
        self.model.preview = Some(ids[next]);
        self.dirty = true;
    }

    /// Make a screenshot its game's cover.
    fn set_cover(&mut self, id: i64) {
        if let Some(library) = &self.library {
            let _ = library.set_cover(id);
        }
        self.reload_from_db();
        self.model
            .set_status("已设为封面".to_string(), StatusKind::Success);
    }

    /// Delete a screenshot (row and file).
    fn remove_screenshot(&mut self, id: i64) {
        if let Some(library) = &self.library {
            let _ = library.remove_screenshot(id);
        }
        if self.model.preview == Some(id) {
            self.close_preview();
        }
        self.reload_from_db();
        self.model
            .set_status("已删除截图".to_string(), StatusKind::Success);
    }

    /// Delete every ticked screenshot, then leave select mode.
    fn delete_selected_screenshots(&mut self) {
        let count = self.selected_shots.len();
        if count == 0 {
            return;
        }
        if let Some(library) = &self.library {
            for id in &self.selected_shots {
                let _ = library.remove_screenshot(*id);
            }
        }
        self.selected_shots.clear();
        self.screenshot_select = false;
        self.reload_from_db();
        self.model
            .set_status(format!("已删除 {count} 张截图"), StatusKind::Success);
    }

    /// Reveal a screenshot's file in the platform file browser.
    fn reveal_screenshot(&mut self, id: i64) {
        let Some(library) = &self.library else {
            return;
        };
        if let Ok(Some(path)) = library.screenshot_path(id) {
            reveal_path(&path);
        }
    }

    /// Open the screenshots directory in the platform file browser.
    fn open_screenshots_folder(&mut self) {
        if let Some(library) = &self.library {
            open_path(library.screenshots_dir());
        }
    }

    /// Write a save state to a slot, with a thumbnail.
    fn save_to_slot(&mut self, slot: u8) {
        self.flush_playtime();
        let (message, kind) = match self.session.as_ref() {
            Some(session) => match session.save_state(slot) {
                Ok(()) => (format!("已存档（槽位 {}）", slot + 1), StatusKind::Success),
                Err(error) => (error, StatusKind::Error),
            },
            None => ("没有正在运行的游戏".to_string(), StatusKind::Info),
        };
        self.model.set_status(message, kind);
        self.refresh_saves();
        self.dirty = true;
    }

    /// Load a save state from a slot.
    fn load_from_slot(&mut self, slot: u8) {
        let (message, kind) = match self.session.as_ref() {
            Some(session) => match session.load_state(slot) {
                Ok(()) => (format!("已读档（槽位 {}）", slot + 1), StatusKind::Success),
                Err(error) => (error, StatusKind::Error),
            },
            None => ("没有正在运行的游戏".to_string(), StatusKind::Info),
        };
        self.model.set_status(message, kind);
        self.dirty = true;
    }

    /// Delete a slot's state and thumbnail.
    fn delete_slot(&mut self, slot: u8) {
        if let Some(session) = self.session.as_ref() {
            session.delete_save(slot);
        }
        self.model.set_status(
            format!("已删除存档（槽位 {}）", slot + 1),
            StatusKind::Success,
        );
        self.refresh_saves();
        self.dirty = true;
    }

    /// Rebuild the saves list for the running game and its core, uploading a
    /// thumbnail for any slot that changed.
    fn refresh_saves(&mut self) {
        self.model.saves.clear();
        self.model.saves_supported = false;
        let Some(session) = self.session.as_ref() else {
            return;
        };
        self.model.saves_supported = session.save_supported();
        let slots = session.save_slots();
        for slot in &slots {
            let mut thumb = None;
            if slot.thumbnail {
                let stale = self
                    .save_textures
                    .get(&slot.slot)
                    .map(|(modified, _)| *modified)
                    != Some(slot.modified_ms);
                if stale {
                    if let Some(backend) = self.backend.clone() {
                        let mut backend = backend.borrow_mut();
                        if let Ok(bytes) = std::fs::read(session.save_thumbnail_path(slot.slot)) {
                            if let Ok((width, height, rgba)) = decode_png(&bytes) {
                                let texture = TextureId::new(SAVE_TEXTURE_BASE + slot.slot as u32);
                                if backend
                                    .register_texture(texture, width, height, &rgba)
                                    .is_ok()
                                {
                                    self.save_textures.insert(
                                        slot.slot,
                                        (
                                            slot.modified_ms,
                                            FrameHandle {
                                                texture,
                                                width,
                                                height,
                                            },
                                        ),
                                    );
                                }
                            }
                        }
                    }
                }
                thumb = self
                    .save_textures
                    .get(&slot.slot)
                    .map(|(_, handle)| *handle);
            }
            self.model.saves.push(SaveSlotRow {
                slot: slot.slot,
                exists: slot.exists,
                modified_ms: slot.modified_ms,
                thumb,
            });
        }
    }

    /// Project the running game's cheats into the view.
    fn populate_cheats(&mut self) {
        self.model.cheats = self
            .cheats
            .iter()
            .map(|cheat| cgb_ui::CheatRow {
                desc: cheat.desc.clone(),
                code: cheat.code.clone(),
                enabled: cheat.enabled,
            })
            .collect();
        self.dirty = true;
    }

    /// Import a RetroArch `.cht` for the running game, replacing its list.
    fn import_cheats(&mut self) {
        let Some(path) = self.cheat_path.clone() else {
            self.model
                .set_status("没有正在运行的游戏".to_string(), StatusKind::Info);
            self.dirty = true;
            return;
        };
        let Some(file) = rfd::FileDialog::new()
            .set_title("导入金手指 (.cht)")
            .add_filter("Cheat", &["cht"])
            .pick_file()
        else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(&file) else {
            self.model
                .set_status("读取金手指文件失败".to_string(), StatusKind::Error);
            self.dirty = true;
            return;
        };
        let cheats = cgb_library::parse_cht(&text);
        if cheats.is_empty() {
            self.model
                .set_status("文件里没有可用的金手指".to_string(), StatusKind::Error);
            self.dirty = true;
            return;
        }
        let count = cheats.len();
        self.cheats = cheats;
        let _ = cgb_library::save_cheats(&path, &self.cheats);
        if let Some(session) = self.session.as_ref() {
            session.apply_cheats(&self.cheats);
        }
        self.populate_cheats();
        self.model
            .set_status(format!("已导入 {count} 条金手指"), StatusKind::Success);
    }

    /// Enable or disable one cheat on the running core, and persist the list.
    fn toggle_cheat(&mut self, index: usize) {
        let Some(cheat) = self.cheats.get_mut(index) else {
            return;
        };
        cheat.enabled = !cheat.enabled;
        let enabled = cheat.enabled;
        let desc = cheat.desc.clone();
        if let Some(path) = self.cheat_path.clone() {
            let _ = cgb_library::save_cheats(&path, &self.cheats);
        }
        // Re-apply the whole list: the cores ignore the enabled flag, so a
        // disabled cheat has to be left out rather than sent as disabled.
        if let Some(session) = self.session.as_ref() {
            session.apply_cheats(&self.cheats);
        }
        self.populate_cheats();
        self.model.set_status(
            format!("{}：{desc}", if enabled { "已开启" } else { "已关闭" }),
            StatusKind::Success,
        );
    }

    /// Pick the picture post-process, persist it, and apply it.
    fn set_shader(&mut self, kind: ShaderKind) {
        self.shader = kind;
        self.settings.shader = kind.key().to_string();
        let _ = self.settings.save(&self.paths.settings_json);
        self.apply_shader();
        self.rebuild_settings_view();
        self.model
            .set_status(format!("画面效果：{}", kind.label()), StatusKind::Info);
    }

    /// Switch the UI theme / appearance, persist it and rebuild so the change
    /// shows immediately (a theme switch also rebuilds the overlays).
    fn set_theme(&mut self, choice: ThemeChoice, light: bool) {
        self.theme_choice = choice;
        self.light = light;
        self.settings.theme = Some(choice.key().to_string());
        self.settings.light = light;
        let _ = self.settings.save(&self.paths.settings_json);
        self.theme = choice.theme(if light { Mode::Light } else { Mode::Dark });
        if let Some(backend) = self.backend.clone() {
            backend
                .borrow_mut()
                .set_clear_color(self.theme.background());
        }
        self.rebuild_settings_view();
        self.dirty = true;
        let label = if light { "浅色" } else { "深色" };
        self.model.set_status(
            format!("外观：{} · {label}", choice.label()),
            StatusKind::Info,
        );
    }

    /// Apply the current preset to the running game's texture.
    fn apply_shader(&mut self) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let Some(backend) = self.backend.clone() else {
            return;
        };
        let mut backend = backend.borrow_mut();
        session.set_effect(&mut backend, texture_effect(self.shader));
    }

    /// Read the running core's options, apply any remembered values, and
    /// project them into the settings page.
    fn reload_core_options(&mut self) {
        let Some(session) = self.session.as_ref() else {
            self.core_options.clear();
            self.rebuild_settings_view();
            return;
        };
        let core_key = session.core_key().to_string();
        for option in session.core_options() {
            let key = format!("{core_key}:{}", option.key);
            if let Some(value) = self.settings.core_options.get(&key) {
                session.set_core_option(&option.key, value);
            }
        }
        self.core_options = session.core_options();
        self.rebuild_settings_view();
    }

    /// Cycle one core option to its previous/next value and persist it.
    fn cycle_core_option(&mut self, index: usize, delta: i32) {
        let Some(option) = self.core_options.get(index) else {
            return;
        };
        if option.values.is_empty() {
            return;
        }
        let current = option
            .values
            .iter()
            .position(|(value, _)| *value == option.value)
            .unwrap_or(0) as i32;
        let next = (current + delta).rem_euclid(option.values.len() as i32) as usize;
        let value = option.values[next].0.clone();
        let display = option.values[next].1.clone();
        let option_key = option.key.clone();
        let core_key = self
            .session
            .as_ref()
            .map(|session| session.core_key().to_string());
        if let Some(session) = self.session.as_ref() {
            session.set_core_option(&option_key, &value);
        }
        if let Some(core_key) = core_key {
            self.settings
                .core_options
                .insert(format!("{core_key}:{option_key}"), value.clone());
            let _ = self.settings.save(&self.paths.settings_json);
        }
        if let Some(option) = self.core_options.get_mut(index) {
            option.value = value;
        }
        self.rebuild_settings_view();
        self.model
            .set_status(format!("{option_key} = {display}"), StatusKind::Info);
    }

    /// Rebuild the library rows from [`App::game_source`], applying the saved
    /// sort order. Pinned games always come first; the sort key only orders
    /// within the pinned and unpinned groups. Cheap enough to run on every sort
    /// click, since it does not touch disk.
    fn rebuild_game_rows(&mut self) {
        let key = SortKey::from_key(&self.settings.library_sort);
        let desc = self.settings.library_sort_desc;
        // The selection is an index into the rows, so reordering moves it.
        // Remember the path and re-point the index after the sort.
        let selected = self
            .model
            .selected
            .and_then(|index| self.model.games.get(index))
            .map(|game| game.path.clone());
        // The tallies are over the whole library, so they do not move as the
        // search or the system filter narrows the grid. A filter for a console
        // the library no longer holds (after switching libraries) is dropped.
        self.model.total_games = self.game_source.len();
        self.model.system_counts = system_counts(&self.game_source);
        if let Some(filter) = self.model.system_filter {
            if !self
                .model
                .system_counts
                .iter()
                .any(|tally| tally.system == filter)
            {
                self.model.system_filter = None;
            }
        }
        let mut games = self.game_source.clone();
        if let Some(system) = self.model.system_filter {
            games.retain(|game| game.system == system);
        }
        if !self.model.search.is_empty() {
            games.retain(|game| game_matches_search(game, &self.model.search));
        }
        order_games(&mut games, key, desc);
        let covers = &self.cover_textures;
        self.model.games = games
            .into_iter()
            .map(|game| {
                let cover = covers.get(&game.id).map(|cover| cover.handle);
                game_row(game, cover)
            })
            .collect();
        self.model.selected =
            selected.and_then(|path| self.model.games.iter().position(|game| game.path == path));
        self.model.sort = key;
        self.model.sort_desc = desc;
        self.dirty = true;
    }

    /// Project the cores, folders and keyboard bindings into the settings page.
    fn rebuild_settings_view(&mut self) {
        self.model.cores = self
            .cores
            .iter()
            .map(|core| CoreRow {
                key: core.key.clone(),
                name: core.name.clone(),
                system: core.system,
                // The core that would run this console now: the saved pick, else
                // the manifest's first core for it.
                selected: choose_core(
                    &self.cores,
                    core.system,
                    self.settings.core_key(core.system),
                )
                .map(|chosen| chosen.key == core.key)
                .unwrap_or(false),
            })
            .collect();
        self.model.library_root = self.settings.library_root.clone();
        self.model.bindings = self
            .bindings
            .get(&self.active_system)
            .map(binding_rows)
            .unwrap_or_default();
        self.model.bindings_system = self.active_system.name().to_string();
        self.model.shader = self.shader;
        self.model.theme_choice = self.theme_choice;
        self.model.light = self.light;
        self.model.core_options = self
            .core_options
            .iter()
            .map(|option| CoreOptionRow {
                key: option.key.clone(),
                label: option.label.clone(),
                values: option.values.clone(),
                value: option.value.clone(),
            })
            .collect();
        // The running core's own input descriptors, when a game is loaded.
        self.model.core_inputs = self
            .session
            .as_ref()
            .map(|session| session.input_descriptors())
            .unwrap_or_default()
            .into_iter()
            .map(|descriptor| InputDescriptorRow {
                port: descriptor.port,
                device: descriptor.device,
                index: descriptor.index,
                id: descriptor.id,
                description: descriptor.description,
            })
            .collect();
        self.dirty = true;
    }

    /// Choose `dir` as the game library: remember it, point the library data
    /// (database, screenshots, saves, cheats) at it and rescan. There is only
    /// one library, so this replaces whatever was open.
    fn set_library_root(&mut self, dir: PathBuf) {
        let dir = dir.to_string_lossy().into_owned();
        self.settings.library_root = Some(dir.clone());
        let _ = self.settings.save(&self.paths.settings_json);
        self.paths = Paths::new(self.paths.user_data.clone(), Some(PathBuf::from(&dir)));
        let _ = self.paths.ensure();
        self.library = Library::open(&self.paths.library_db).ok();
        self.reload_library();
        self.model
            .set_status(format!("已切换游戏库：{dir}"), StatusKind::Info);
    }

    /// Ask for a folder and switch to it as the game library.
    fn switch_library(&mut self) {
        let Some(dir) = rfd::FileDialog::new().set_title("选择游戏库").pick_folder() else {
            return;
        };
        self.set_library_root(dir);
    }

    /// Ask for ROM files and add them to the library.
    fn add_games_dialog(&mut self) {
        let extensions: Vec<&str> = cgb_systems::SYSTEMS
            .iter()
            .flat_map(|system| system.extensions().iter().copied())
            .collect();
        let Some(paths) = rfd::FileDialog::new()
            .set_title("选择游戏")
            .add_filter("ROM", &extensions)
            .pick_files()
        else {
            return;
        };
        self.add_game_paths(paths);
    }

    /// Add ROM files (dropped in, or picked in the dialog) to the library.
    ///
    /// Every ROM — one picked, or all of them inside a dropped folder — is
    /// **copied** into the library folder, so the library stays one
    /// self-contained folder. This is the legacy front end's rule
    /// (`Library.add`): a game that was only pointed at would break the moment
    /// its file moved. With no library chosen yet, the first dropped folder
    /// simply becomes the library.
    fn add_game_paths(&mut self, paths: Vec<PathBuf>) {
        let mut files = Vec::new();
        let mut switched = false;
        for path in paths {
            if path.is_dir() {
                if !self.paths.has_library() {
                    // Nothing chosen yet: the folder becomes the library.
                    self.set_library_root(path);
                    switched = true;
                    continue;
                }
                if inside_library(&self.paths.root, &path) {
                    // Already part of the library; the rescan finds it.
                    continue;
                }
                files.extend(
                    cgb_library::scan_dir(&path)
                        .into_iter()
                        .map(|game| PathBuf::from(game.path)),
                );
            } else {
                files.push(path);
            }
        }
        let report = import_roms(&self.paths.roms, &files);
        self.reload_library();
        if !report.is_empty() || !switched {
            self.model
                .set_status(import_status(&report), StatusKind::Info);
        }
    }

    /// Add any files dropped since the last frame, in one batch.
    fn flush_drops(&mut self) {
        if self.pending_drops.borrow().is_empty() {
            return;
        }
        let paths = std::mem::take(&mut *self.pending_drops.borrow_mut());
        self.add_game_paths(paths);
    }

    /// Pin or unpin a game, then re-sort so it moves to (or leaves) the top.
    fn toggle_pin(&mut self, index: usize) {
        let Some(game) = self.model.games.get(index) else {
            return;
        };
        let path = game.path.clone();
        let pinned = !game.pinned;
        if let Some(library) = &self.library {
            let _ = library.set_pinned(&path, pinned);
        }
        if let Some(source) = self.game_source.iter_mut().find(|game| game.path == path) {
            source.pinned = pinned;
        }
        self.rebuild_game_rows();
        let verb = if pinned {
            "已置顶"
        } else {
            "已取消置顶"
        };
        let message = format!("{verb}：{}", self.name_of(&path));
        self.model.set_status(message, StatusKind::Info);
    }

    /// Delete a game outright: remove the ROM file, forget it in the settings
    /// and the database. If it was the running game, stop the machine first.
    fn delete_game(&mut self, game_id: i64) {
        let Some(game) = self.game_source.iter().find(|game| game.id == game_id) else {
            return;
        };
        let path = game.path.clone();
        let name = game.name.clone();
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                self.model
                    .set_status(format!("删除失败：{error}"), StatusKind::Error);
                self.dirty = true;
                return;
            }
        }
        if self.selected_game_id() == Some(game_id) {
            self.flush_playtime();
            self.session = None;
            self.cheats.clear();
            self.cheat_path = None;
            self.core_options.clear();
            self.model.selected = None;
            self.model.playing = false;
            self.model.paused = false;
            self.model.frame = None;
            // The immersive view has nothing left to show.
            self.set_fullscreen(false);
        }
        self.settings.added_roms.retain(|rom| rom != &path);
        let _ = self.settings.save(&self.paths.settings_json);
        if let Some(library) = &self.library {
            let _ = library.remove(&path);
        }
        self.reload_library();
        self.model
            .set_status(format!("已删除：{name}"), StatusKind::Success);
    }

    /// The display name of a library path, for status messages; falls back to
    /// the path itself when the game is no longer listed.
    fn name_of(&self, path: &str) -> String {
        self.model
            .games
            .iter()
            .find(|game| game.path == path)
            .map(|game| game.name.clone())
            .unwrap_or_else(|| path.to_string())
    }

    /// Pick a sort key. Choosing the active key again flips the direction;
    /// choosing a new key starts it in that key's natural direction.
    fn set_sort(&mut self, key: SortKey) {
        if self.model.sort == key {
            self.settings.library_sort_desc = !self.settings.library_sort_desc;
        } else {
            self.settings.library_sort = key.key().to_string();
            self.settings.library_sort_desc = key.default_desc();
        }
        let _ = self.settings.save(&self.paths.settings_json);
        self.rebuild_game_rows();
    }

    /// Remember a core pick for its console.
    fn select_core(&mut self, index: usize) {
        let Some(row) = self.model.cores.get(index) else {
            return;
        };
        let (system, key) = (row.system, row.key.clone());
        self.settings.set_core_key(system, Some(&key));
        let _ = self.settings.save(&self.paths.settings_json);
        self.model.set_status(
            format!("{} 的核心已切换为 {key}", system.short()),
            StatusKind::Success,
        );
        self.rebuild_settings_view();
    }

    /// Re-scan and reconcile the library after the folders changed.
    fn reload_library(&mut self) {
        self.refresh_library();
        self.rebuild_settings_view();
    }

    /// Write the running game's accumulated play time to the library. Safe to
    /// call often: whole seconds are drained and the fraction is kept.
    fn flush_playtime(&mut self) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let path = session.rom_path().to_path_buf();
        let seconds = session.take_played_seconds();
        if seconds <= 0 {
            return;
        }
        let path_str = path.to_string_lossy().into_owned();
        if let Some(library) = &self.library {
            let _ = library.note_playtime(&path_str, seconds);
        }
        if let Some(source) = self
            .game_source
            .iter_mut()
            .find(|game| game.path == path_str)
        {
            source.play_seconds += seconds;
        }
    }

    /// Record a game start: bump the play count, stamp the time, and refresh
    /// the rows so the "recent"/"playtime" orders move the game.
    fn note_started(&mut self, rom_path: &Path) {
        let path = rom_path.to_string_lossy().into_owned();
        let now = now_millis();
        if let Some(library) = &self.library {
            let _ = library.note_played(&path, now);
        }
        if let Some(source) = self.game_source.iter_mut().find(|game| game.path == path) {
            source.play_count += 1;
            source.last_played_at = now;
        }
        self.rebuild_game_rows();
        self.model.selected = self
            .model
            .games
            .iter()
            .position(|game| Path::new(&game.path) == rom_path);
        // The screenshots section follows the game just started.
        self.model.screenshot_game = self.selected_game_id();
    }

    /// Take a screenshot of the running game: encode its last frame, store it
    /// under the game in the library, and refresh. `as_cover` also makes the
    /// new picture the game's cover.
    fn capture_screenshot(&mut self, as_cover: bool) {
        let Some(session) = self.session.as_ref() else {
            self.model
                .set_status("没有正在运行的游戏".to_string(), StatusKind::Info);
            self.dirty = true;
            return;
        };
        let rom_path = session.rom_path().to_string_lossy().into_owned();
        let Some((width, height, pixels)) = session.last_pixels() else {
            self.model
                .set_status("还没有画面可以截图".to_string(), StatusKind::Info);
            self.dirty = true;
            return;
        };
        let png = match encode_png(width, height, pixels) {
            Ok(png) => png,
            Err(error) => {
                self.model
                    .set_status(format!("截图失败：{error}"), StatusKind::Error);
                self.dirty = true;
                return;
            }
        };
        let saved = match &self.library {
            Some(library) => library.save_screenshot(
                &rom_path,
                &png,
                i64::from(width),
                i64::from(height),
                as_cover,
            ),
            None => {
                self.model
                    .set_status("游戏库不可用".to_string(), StatusKind::Error);
                self.dirty = true;
                return;
            }
        };
        match saved {
            Ok(Some(_)) => {
                let message = if as_cover {
                    "已截图并设为封面"
                } else {
                    "已截图"
                };
                self.model.set_status(message, StatusKind::Success);
                self.reload_from_db();
            }
            Ok(None) => {
                self.model
                    .set_status("该游戏不在游戏库中".to_string(), StatusKind::Info);
                self.dirty = true;
            }
            Err(error) => {
                self.model
                    .set_status(format!("截图失败：{error}"), StatusKind::Error);
                self.dirty = true;
            }
        }
    }

    /// App-level keyboard commands, handled before the UI or the emulator sees
    /// the key. While a text field is open it owns the keyboard, except that
    /// Enter commits and Escape cancels.
    fn handle_hotkey(&mut self, event: &InputEvent) {
        let (key, pressed) = match event {
            InputEvent::KeyDown { key } => (*key, true),
            InputEvent::KeyUp { key } => (*key, false),
            _ => return,
        };
        if self.model.editing.is_some() {
            if pressed {
                match key {
                    Key::Enter => self.commit_edit(),
                    Key::Escape => self.cancel_edit(),
                    _ => {}
                }
            }
            return;
        }
        match key {
            // Backspace is the rewind key: hold it to step the game back.
            Key::Backspace => self.rewinding = pressed,
            // F12 is the screenshot key; Shift+F12 also sets the cover.
            Key::F12 if pressed => self.capture_screenshot(self.modifiers.shift),
            // Save-state / fullscreen hotkeys fire once, on press.
            _ if pressed => {
                if let Some(action) = state_shortcut(key, self.modifiers.shift) {
                    self.actions.push(action);
                    self.handle_actions();
                }
            }
            _ => {}
        }
    }

    /// Route one input event: the UI first, then the emulator bindings.
    fn feed(&mut self, event: &InputEvent) {
        let scroll_before = self.ui.scroll_offset();
        let captured = self
            .session
            .as_ref()
            .is_some_and(|session| !session.paused());
        let keyboard = matches!(event, InputEvent::KeyDown { .. } | InputEvent::KeyUp { .. });
        let editing = self.model.editing.is_some();
        // An open overlay owns the event first, even while a game captures the
        // keyboard: a menu opened from the UI must close on Escape.
        if !self.ui.route_overlay_input(event) {
            if editing {
                // An open text field owns the keyboard (text, arrows, IME);
                // Enter / Escape were already handled as app commands.
                self.ui.route_ui_input(event);
            } else if captured && keyboard {
                // The running game owns the keyboard; Escape is the one way
                // back to the UI.
                if matches!(event, InputEvent::KeyDown { key: Key::Escape }) {
                    self.escape_game();
                } else if let Some(bindings) = self.bindings.get(&self.active_system) {
                    match event {
                        InputEvent::KeyDown { key } => {
                            bindings.apply(*key, true, &mut self.input, 0)
                        }
                        InputEvent::KeyUp { key } => {
                            bindings.apply(*key, false, &mut self.input, 0)
                        }
                        _ => {}
                    }
                }
            } else {
                // No game, or paused: the UI owns the keyboard. Tab moves the
                // focus; Enter / Space activate the focused control.
                if !self.ui.route_ui_input(event) {
                    if let InputEvent::KeyDown { key } = event {
                        match key {
                            Key::Tab => {
                                self.ui.move_focus(self.modifiers.shift);
                            }
                            Key::Enter | Key::Space => {
                                self.ui.activate_focus();
                            }
                            _ => {}
                        }
                    }
                }
                if matches!(event, InputEvent::KeyDown { key: Key::Escape }) {
                    self.escape_game();
                }
            }
        }
        self.handle_actions();
        // The resize handle owns the middle width. Feed the live value back so
        // the grid's column count follows it, and persist the width when the
        // drag ends.
        let middle_width = self.ui.middle_width();
        if middle_width != self.model.middle_width {
            self.model.middle_width = middle_width;
        }
        let released = matches!(event, InputEvent::PointerUp { .. });
        self.sync_grid_columns(released);
        if released && middle_width != self.settings.middle_width {
            self.settings.middle_width = middle_width;
            let _ = self.settings.save(&self.paths.settings_json);
        }
        // A wheel or scrollbar move changes the offset. Feed it back so the
        // grid can re-window; scrolling inside the mounted rows is a plain
        // repaint, so only mark the tree dirty when the window no longer
        // covers the viewport.
        let scroll_after = self.ui.scroll_offset();
        if matches!(self.model.section, Section::Library | Section::Screenshots)
            && !self.model.fullscreen
            && scroll_after != scroll_before
        {
            self.model.grid_offset = scroll_after;
            if !self.ui.grid_window_covers(&self.model) {
                self.dirty = true;
            }
        }
    }

    /// Step the grid's column count from the middle width, at most once per
    /// [`COLUMNS_CHECK_INTERVAL`] (or immediately when `force`). A change marks
    /// the UI dirty, so it rebuilds with the new column count.
    fn sync_grid_columns(&mut self, force: bool) {
        let now = Instant::now();
        if !force && now.duration_since(self.last_columns_check) < COLUMNS_CHECK_INTERVAL {
            return;
        }
        self.last_columns_check = now;
        let columns = library_columns(self.ui.middle_width());
        if columns != self.model.grid_columns {
            self.model.grid_columns = columns;
            self.dirty = true;
        }
    }

    /// Act on whatever the UI recorded this event.
    fn handle_actions(&mut self) {
        let actions = self.actions.drain();
        if actions.is_empty() {
            return;
        }
        let mut opened_overlay = false;
        for action in actions {
            match action {
                Action::GameContextMenu { index, position } => {
                    if let Some(game) = self.model.games.get(index) {
                        self.ui
                            .open_game_menu(self.theme, game, index, position, &self.actions);
                    }
                    opened_overlay = true;
                }
                Action::OpenCoreMenu { system, anchor } => {
                    self.ui.open_core_menu(
                        self.theme,
                        system,
                        anchor,
                        &self.model.cores,
                        &self.actions,
                    );
                    opened_overlay = true;
                }
                Action::Show(section) => {
                    let changed = self.model.section != section;
                    self.model.section = section;
                    // Leaving the page drops any in-progress edit.
                    self.model.editing = None;
                    // The screenshots section follows the playing game unless a
                    // card sent it to a specific one.
                    if section == Section::Screenshots && self.model.screenshot_game.is_none() {
                        self.model.screenshot_game = self.selected_game_id();
                    }
                    if section == Section::Saves {
                        self.refresh_saves();
                    }
                    if section == Section::Cheats {
                        if self.session.is_none() {
                            self.cheats.clear();
                            self.cheat_path = None;
                        }
                        self.populate_cheats();
                    }
                    // A page switch starts its scroll at the top; keep the
                    // grid's window in step with that.
                    if changed && matches!(section, Section::Library | Section::Screenshots) {
                        self.model.grid_offset = 0.0;
                    }
                    self.dirty = true;
                }
                Action::Play(index) => self.start_game(index),
                Action::TogglePause => {
                    if let Some(session) = self.session.as_mut() {
                        session.toggle_pause();
                    }
                    // A pause stops the clock, so bank what it has run.
                    if self
                        .session
                        .as_ref()
                        .is_some_and(|session| session.paused())
                    {
                        self.flush_playtime();
                    }
                    self.dirty = true;
                }
                Action::ToggleFullscreen => self.toggle_fullscreen(),
                Action::Reset => {
                    if let Some(session) = self.session.as_ref() {
                        session.reset();
                    }
                }
                Action::Rewind => {
                    if let Some(session) = self.session.as_mut() {
                        if session.can_rewind() {
                            session.rewind_step();
                        }
                    }
                    self.ui.request_repaint();
                    self.dirty = true;
                }
                Action::SetShader(kind) => self.set_shader(kind),
                Action::SetThemeChoice(choice) => self.set_theme(choice, self.light),
                Action::SetLight(light) => self.set_theme(self.theme_choice, light),
                Action::CycleCoreOption(index, delta) => self.cycle_core_option(index, delta),
                Action::SaveState(slot) => {
                    let (message, kind) = match self.session.as_ref() {
                        Some(session) => match session.save_state(slot) {
                            Ok(()) => (format!("已存档（槽位 {slot}）"), StatusKind::Success),
                            Err(error) => (error, StatusKind::Error),
                        },
                        None => ("没有正在运行的游戏".to_string(), StatusKind::Info),
                    };
                    self.model.set_status(message, kind);
                    self.dirty = true;
                }
                Action::LoadState(slot) => {
                    let (message, kind) = match self.session.as_ref() {
                        Some(session) => match session.load_state(slot) {
                            Ok(()) => (format!("已读档（槽位 {slot}）"), StatusKind::Success),
                            Err(error) => (error, StatusKind::Error),
                        },
                        None => ("没有正在运行的游戏".to_string(), StatusKind::Info),
                    };
                    self.model.set_status(message, kind);
                    self.dirty = true;
                }
                Action::AddGames => self.add_games_dialog(),
                Action::SwitchLibrary => self.switch_library(),
                Action::FilterSystem(system) => {
                    self.model.system_filter = system;
                    self.rebuild_game_rows();
                }
                Action::SelectCore(index) => self.select_core(index),
                Action::TogglePin(index) => self.toggle_pin(index),
                Action::RequestDelete(confirm) => {
                    self.pending_confirm = Some(confirm);
                    self.ui.confirm_destructive(
                        confirm.title(),
                        confirm.message(),
                        Action::ConfirmDelete,
                        &self.actions,
                    );
                    opened_overlay = true;
                }
                Action::ConfirmDelete => self.confirm_delete(),
                Action::StartRename(id) => self.start_edit(id, EditKind::Name),
                Action::StartTagEdit(id) => self.start_edit(id, EditKind::Tags),
                Action::StartSearch => self.start_search(),
                Action::ClearSearch => self.clear_search(),
                Action::CommitEdit => self.commit_edit(),
                Action::CancelEdit => self.cancel_edit(),
                Action::Sort(key) => self.set_sort(key),
                Action::ToggleSortOrder => self.set_sort(self.model.sort),
                Action::Screenshot => self.capture_screenshot(false),
                Action::ScreenshotCover => self.capture_screenshot(true),
                Action::ShowScreenshots(game_id) => {
                    self.model.section = Section::Screenshots;
                    self.model.screenshot_game = Some(game_id);
                    self.dirty = true;
                }
                Action::PreviewScreenshot(id) => self.preview_screenshot(id),
                Action::ClosePreview => self.close_preview(),
                Action::StepPreview(delta) => self.step_preview(delta),
                Action::SetCover(id) => self.set_cover(id),
                Action::RevealScreenshot(id) => self.reveal_screenshot(id),
                Action::ToggleScreenshotSelect => {
                    self.screenshot_select = !self.screenshot_select;
                    if !self.screenshot_select {
                        self.selected_shots.clear();
                    }
                    self.rebuild_screenshot_rows();
                    self.dirty = true;
                }
                Action::ToggleScreenshotSelected(id) => {
                    if let Some(index) = self.selected_shots.iter().position(|shot| *shot == id) {
                        self.selected_shots.remove(index);
                    } else {
                        self.selected_shots.push(id);
                    }
                    self.rebuild_screenshot_rows();
                    self.dirty = true;
                }
                Action::DeleteSelectedScreenshots => self.delete_selected_screenshots(),
                Action::OpenScreenshotsFolder => self.open_screenshots_folder(),
                Action::SaveToSlot(slot) => self.save_to_slot(slot),
                Action::LoadFromSlot(slot) => self.load_from_slot(slot),
                Action::DeleteSlot(slot) => self.delete_slot(slot),
                Action::ImportCheats => self.import_cheats(),
                Action::ToggleCheat(index) => self.toggle_cheat(index),
            }
        }
        // A menu item action closes the menu; opening one keeps it.
        if !opened_overlay {
            self.ui.close_overlays();
        }
    }

    /// Run the destructive action the confirmation dialog was asking about.
    fn confirm_delete(&mut self) {
        let Some(confirm) = self.pending_confirm.take() else {
            return;
        };
        self.dirty = true;
        match confirm {
            Confirm::DeleteGame(id) => self.delete_game(id),
            Confirm::DeleteScreenshot(id) => self.remove_screenshot(id),
        }
    }

    /// Escape while a game is running: pause it so the keyboard returns to the
    /// UI, or resume a paused one. In fullscreen it also leaves fullscreen, so
    /// Escape is always "give me back the window".
    fn escape_game(&mut self) {
        let Some(paused) = self.session.as_mut().map(|session| {
            session.toggle_pause();
            session.paused()
        }) else {
            return;
        };
        if paused {
            self.flush_playtime();
        }
        self.model.paused = paused;
        self.model.set_status(
            if paused {
                "已暂停（再按 Esc 继续）"
            } else {
                "已继续"
            },
            StatusKind::Info,
        );
        if self.model.fullscreen {
            self.toggle_fullscreen();
        }
        self.dirty = true;
        self.ui.request_repaint();
    }

    /// Begin editing a game's name or tags; the app seeds the shared edit state
    /// the view mounts a `TextInput` from.
    fn start_edit(&mut self, game_id: i64, kind: EditKind) {
        // A search is not tied to a game; rename / tag edits are.
        let text = if kind == EditKind::Search {
            self.model.search.clone()
        } else {
            let Some(game) = self.game_source.iter().find(|game| game.id == game_id) else {
                return;
            };
            match kind {
                EditKind::Name => game.name.clone(),
                EditKind::Tags => game.tags.join(", "),
                EditKind::Search => unreachable!("handled above"),
            }
        };
        self.actions
            .set_edit(Rc::new(RefCell::new(TextEdit::new(text))));
        self.model.editing = Some(EditState { game_id, kind });
        self.dirty = true;
    }

    /// Begin typing a library search; the app takes the keyboard.
    fn start_search(&mut self) {
        self.start_edit(-1, EditKind::Search);
    }

    /// Clear the search and leave any edit.
    fn clear_search(&mut self) {
        self.model.search.clear();
        self.model.editing = None;
        self.actions.clear_edit();
        self.rebuild_game_rows();
        self.dirty = true;
    }

    /// Discard the pending edit. A search edit also clears the query.
    fn cancel_edit(&mut self) {
        let was_search = self
            .model
            .editing
            .as_ref()
            .is_some_and(|edit| edit.kind == EditKind::Search);
        self.model.editing = None;
        self.actions.clear_edit();
        if was_search {
            self.model.search.clear();
            self.rebuild_game_rows();
        }
        self.dirty = true;
    }

    /// Commit the pending edit to the database and the in-memory rows.
    fn commit_edit(&mut self) {
        let Some(edit) = self.model.editing.take() else {
            return;
        };
        let text = self.actions.edit_text();
        self.actions.clear_edit();
        // A search is applied as it is typed; committing just closes it.
        if edit.kind == EditKind::Search {
            let message = if self.model.search.is_empty() {
                String::new()
            } else {
                format!("搜索：{}", self.model.search)
            };
            self.model.set_status(message, StatusKind::Info);
            self.dirty = true;
            return;
        }
        let Some(path) = self
            .game_source
            .iter()
            .find(|game| game.id == edit.game_id)
            .map(|game| game.path.clone())
        else {
            return;
        };
        match edit.kind {
            EditKind::Name => {
                let name = text.trim().to_string();
                if name.is_empty() {
                    self.model
                        .set_status("名字不能为空".to_string(), StatusKind::Error);
                    self.dirty = true;
                    return;
                }
                if let Some(library) = &self.library {
                    let _ = library.rename(&path, &name);
                }
                if let Some(game) = self
                    .game_source
                    .iter_mut()
                    .find(|game| game.id == edit.game_id)
                {
                    game.name = name.clone();
                }
                self.model
                    .set_status(format!("已改名为：{name}"), StatusKind::Success);
            }
            EditKind::Tags => {
                let tags: Vec<String> = text
                    .split([',', '，', ' '])
                    .map(str::trim)
                    .filter(|tag| !tag.is_empty())
                    .map(str::to_string)
                    .collect();
                if let Some(library) = &self.library {
                    let _ = library.set_tags(&path, &tags);
                }
                if let Some(game) = self
                    .game_source
                    .iter_mut()
                    .find(|game| game.id == edit.game_id)
                {
                    game.tags = tags.clone();
                }
                self.model.set_status(
                    format!("已更新标签（{} 个）", tags.len()),
                    StatusKind::Success,
                );
            }
            EditKind::Search => {}
        }
        self.rebuild_game_rows();
        self.dirty = true;
    }

    fn start_game(&mut self, index: usize) {
        let Some(game) = self.model.games.get(index).cloned() else {
            return;
        };
        self.model.editing = None;
        self.model.selected = Some(index);
        self.start_path(Path::new(&game.path));
    }

    /// Start a ROM by path: read it, pick a core, build a [`Session`].
    fn start_path(&mut self, rom_path: &Path) {
        // The keyboard now feeds this console's binding set.
        self.active_system = system_for_path(&rom_path.to_string_lossy());
        let data = match std::fs::read(rom_path) {
            Ok(data) => data,
            Err(error) => {
                self.model
                    .set_status(format!("读取 ROM 失败：{error}"), StatusKind::Error);
                self.dirty = true;
                return;
            }
        };

        let spec = match self.resolve_core(rom_path) {
            Ok(spec) => spec,
            Err(status) => {
                self.model.set_status(status, StatusKind::Info);
                self.dirty = true;
                return;
            }
        };

        if !spec.module.is_file() {
            self.model.set_status(
                format!(
                    "找不到核心 {}：先跑 ./scripts/build-cores.sh，或用 --core 指定模块",
                    spec.module.display()
                ),
                StatusKind::Error,
            );
            self.model.core_name = spec.name.clone();
            self.dirty = true;
            return;
        }

        // One core may be live at a time. The host publishes itself in a
        // process-wide slot and a core dylib is a single instance, so starting
        // the new session before the old one drops would `retro_init` the same
        // core again and then `retro_deinit` the new machine when the old
        // session falls (a segfault). Drop the old machine first; this also
        // flushes its battery save and its play time.
        self.flush_playtime();
        self.session = None;
        self.model.preview = None;
        self.preview_paused = false;

        let started = {
            let Some(shared) = self.backend.clone() else {
                return;
            };
            let mut backend = shared.borrow_mut();
            Session::start(
                &spec,
                &self.paths.system,
                &self.paths.saves,
                rom_path,
                &data,
                &mut backend,
            )
        };

        match started {
            Ok(session) => {
                self.model.core_name = session.core_name().to_string();
                self.model.playing = true;
                self.model.paused = false;
                self.model.status.clear();
                self.session = Some(session);
                // Reset the on-screen FPS window for the new machine; show the
                // core's nominal rate until the first measured window.
                self.fps = self
                    .session
                    .as_ref()
                    .map(|session| session.target_fps())
                    .unwrap_or(0.0);
                self.fps_frames = self
                    .session
                    .as_ref()
                    .map(|session| session.frame_index())
                    .unwrap_or(0);
                self.fps_time = Instant::now();
                // Cheats are per game and applied right after load.
                let cheat_path = cgb_library::cheat_file(&self.paths.cheats, rom_path);
                self.cheats = cgb_library::load_cheats(&cheat_path);
                self.cheat_path = Some(cheat_path);
                if let Some(session) = self.session.as_ref() {
                    session.apply_cheats(&self.cheats);
                }
                if !self.cheats.is_empty() {
                    self.model.set_status(
                        format!("已应用 {} 条金手指", self.cheats.len()),
                        StatusKind::Success,
                    );
                }
                self.populate_cheats();
                // The picture effect follows the session.
                self.apply_shader();
                // Core options are known only after load.
                self.reload_core_options();
                // Count the run and stamp it; this also re-points the selection
                // at the row, which a sort by "recent" may have moved.
                self.note_started(rom_path);
                // The saves list follows the running core.
                self.refresh_saves();
            }
            Err(error) => {
                // An arcade ROM usually fails because its BIOS `.zip` is not in
                // the system directory; say where to put it.
                let message = if self.active_system == SystemId::Arcade {
                    format!(
                        "{error}（街机 ROM 需要对应的 BIOS .zip，放到 {}）",
                        self.paths.system.display()
                    )
                } else {
                    error
                };
                self.model.set_status(message, StatusKind::Error);
                self.model.playing = false;
                // A failed load leaves nothing to fill the immersive view.
                self.set_fullscreen(false);
            }
        }
        self.dirty = true;
    }

    /// Resolve which core runs this ROM. `--core` wins over the saved pick,
    /// which wins over the manifest's first core for the console. A key is
    /// looked up in the merged manifests; a `--core <path>` module is used as
    /// given.
    fn resolve_core(&self, rom_path: &Path) -> Result<CoreSpec, String> {
        let path = rom_path.to_string_lossy();
        let system = system_for_path(&path);
        let spec = match &self.core_override {
            Some(CoreOverride::Key(key)) => choose_core(&self.cores, system, Some(key))
                .ok_or_else(|| {
                    format!("没有核心 `{key}` 支持 {}（见 cores.json）", system.short())
                })?,
            Some(CoreOverride::Module(module)) => {
                return Ok(CoreSpec::custom(module.clone(), system));
            }
            None => choose_core(&self.cores, system, self.settings.core_key(system))
                .ok_or_else(|| format!("{} 没有可用核心（见 cores.json）", system.short()))?,
        };
        let mut spec = spec.clone();
        spec.module = self.find_module(&spec.module);
        Ok(spec)
    }

    /// Locate a module: an absolute or existing path is used as given; a bare
    /// file name is searched in the packaged `cores/` dir, then the dev build
    /// output in `cores/dist` (see `cores/README.md`).
    fn find_module(&self, module: &Path) -> PathBuf {
        if module.is_absolute() || module.is_file() {
            return module.to_path_buf();
        }
        let packaged = self.paths.cores.join(module);
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

    /// Toggle the immersive fullscreen play view. Entering it needs a loaded
    /// game (the view *is* the game); leaving it always works.
    fn toggle_fullscreen(&mut self) {
        self.set_fullscreen(!self.fullscreen_target);
    }

    /// Ask the window for `on`, keeping the lightweight play view mounted until
    /// the OS animation settles (the heavy shell is rebuilt afterwards).
    fn set_fullscreen(&mut self, on: bool) {
        let on = on && self.session.is_some();
        if self.fullscreen_target == on
            && self.pending_fullscreen.is_none()
            && self.transition.is_none()
        {
            return;
        }
        self.fullscreen_target = on;
        // Entering hides the UI now; the window request is issued on the next
        // frame, once this tree has been built and presented, so the OS
        // animates from a blank surface. Leaving keeps the play view and only
        // rebuilds the shell after the window settles.
        if on {
            self.model.ui_hidden = true;
        }
        self.dirty = true;
        self.pending_fullscreen = Some(PendingFullscreen {
            target: on,
            at: Instant::now() + Duration::from_millis(100),
        });
        self.hidden_painted = false;
        self.ui.request_repaint();
    }

    fn rebuild_ui(&mut self) {
        self.ui.rebuild(self.theme, &self.model, &self.actions);
        if let Some(measurer) = self.measurer.clone() {
            self.ui.install_measurer(measurer);
        }
        self.dirty = false;
    }

    fn step_gamepad(&mut self) {
        if let Some(gamepads) = self.gamepads.as_mut() {
            gamepads.poll(&mut self.input);
        }
    }

    fn advance_frame(&mut self, dt: f32) {
        // Issue a pending window request only after the (hidden) tree was
        // presented and the black screen has shown for a moment, so the OS
        // animates from a settled surface.
        if let Some(pending) = self.pending_fullscreen {
            if self.hidden_painted && Instant::now() >= pending.at {
                self.pending_fullscreen = None;
                self.hidden_painted = false;
                let on = pending.target;
                if let Some(window) = self.window.clone() {
                    apply_window_fullscreen(&window, on);
                }
                self.transition = Some(FullscreenTransition {
                    target: on,
                    last_activity: Instant::now(),
                });
            }
        }
        // A fullscreen transition settles once the window stops resizing; only
        // then apply the target (rebuilding the heavy shell on exit).
        if let Some(transition) = self.transition {
            if transition.last_activity.elapsed() >= Duration::from_millis(200) {
                self.transition = None;
                self.model.ui_hidden = false;
                self.model.fullscreen = transition.target;
                self.dirty = true;
            }
        }
        self.flush_drops();
        // Overlay timers (a message counting down) advance with the clock.
        self.ui.update(dt);
        // A running overlay needs a fresh paint each frame, not a replay.
        if self.ui.overlays_animating() {
            self.ui.request_repaint();
        }
        self.step_gamepad();

        if self.rewinding {
            // Holding the rewind key steps back a couple of snapshots a frame.
            if let Some(session) = self.session.as_mut() {
                for _ in 0..2 {
                    if !session.rewind_step() {
                        break;
                    }
                }
            }
            self.ui.request_repaint();
        } else if let Some(session) = self.session.as_mut() {
            if !session.paused() {
                if let Some(backend) = self.backend.clone() {
                    session.advance(dt as f64, &mut backend.borrow_mut(), &self.input);
                }
            }
        }

        // Bank play time every so often, so a crash or a kill loses at most
        // this window rather than the whole session.
        if self.last_play_flush.elapsed() >= Duration::from_secs(15) {
            self.flush_playtime();
            self.last_play_flush = Instant::now();
        }

        if let Some(session) = self.session.as_ref() {
            self.model.frame = session.frame();
            self.model.paused = session.paused();
            self.model.playing = true;
            // A core message (SET_MESSAGE) goes to the status line, but only
            // when it changed (a core may repeat the same message each frame).
            if let Some(message) = session.take_message() {
                if self.model.status != message {
                    self.model.set_status(message, StatusKind::Info);
                    self.dirty = true;
                }
            }
        }
        self.refresh_play_info();

        if self.dirty {
            self.rebuild_ui();
        }
        // Rebuild the draw list only when something changed. A running game
        // updates its texture in place, so its frames re-submit the previous
        // list instead of laying out and painting the whole UI again.
        self.repaint = self.ui.take_repaint() || self.draw_list.is_none();
    }

    /// Recompute the play column's live info (FPS / resolution / core) and push
    /// it into the mounted text node, without rebuilding the tree.
    fn refresh_play_info(&mut self) {
        let (frames, core_name) = match self.session.as_ref() {
            Some(session) => (session.frame_index(), session.core_name().to_string()),
            None => {
                self.fps = 0.0;
                if !self.model.info.is_empty() {
                    self.model.info.clear();
                    self.ui.set_info("");
                }
                return;
            }
        };
        // A half-second window: long enough to be stable, short enough to feel
        // live. Between ticks the mounted readout is left as is (unless it has
        // not been mounted yet).
        let now = Instant::now();
        let elapsed = now.duration_since(self.fps_time).as_secs_f32();
        let tick = elapsed >= 0.5;
        if tick {
            let instant = frames.wrapping_sub(self.fps_frames) as f32 / elapsed;
            self.fps = if self.fps <= 0.0 {
                instant
            } else {
                self.fps * 0.5 + instant * 0.5
            };
            self.fps_frames = frames;
            self.fps_time = now;
        }
        if !tick && !self.model.info.is_empty() {
            return;
        }
        let (width, height) = self
            .model
            .frame
            .as_ref()
            .map(|frame| (frame.width, frame.height))
            .unwrap_or((0, 0));
        self.model.info = format!(
            "{} FPS · {width}×{height} · {core_name}",
            self.fps.round() as i32
        );
        self.ui.set_info(&self.model.info);
    }

    /// Resolve layout when the tree changed. The library / screenshots grids
    /// mount only the rows the viewport covers, so the resolved offset and
    /// viewport go back into the model; a scroll past the mounted rows asks for
    /// one more rebuild, while scrolling inside them is just a repaint.
    fn layout_ui(&mut self, viewport: ViewportSize) {
        if !self.repaint {
            return;
        }
        let started = Instant::now();
        self.ui.layout(viewport);
        if matches!(self.model.section, Section::Library | Section::Screenshots)
            && !self.model.fullscreen
        {
            let offset = self.ui.scroll_offset();
            let viewport_height = self.ui.scroll_viewport();
            if offset != self.model.grid_offset || viewport_height != self.model.grid_viewport {
                self.model.grid_offset = offset;
                self.model.grid_viewport = viewport_height;
                if !self.ui.grid_window_covers(&self.model) {
                    self.dirty = true;
                }
            }
        }
        self.last_layout = started.elapsed();
    }

    /// Emit this frame's commands. An unchanged UI replays the previous draw
    /// list instead of laying out and painting the tree again.
    fn paint_ui(&mut self, paint: &mut PaintContext) {
        if self.repaint {
            let started = Instant::now();
            self.ui.paint(paint);
            self.draw_list = Some(DrawList::from(paint.draw_list().to_vec()));
            self.last_paint = started.elapsed();
            self.profile_frame(
                self.repaint,
                self.last_layout,
                self.last_paint,
                Duration::ZERO,
            );
        } else if let Some(list) = self.draw_list.as_ref() {
            paint.extend(list);
        }
        self.hidden_painted = true;
    }

    /// Record one frame into the `CGB_PERF` profiler. It prints the per-frame
    /// breakdown, audits the draw list for structural problems, and every
    /// [`PERF_REPORT_FRAMES`] prints the aggregate summary. A no-op without
    /// `CGB_PERF`.
    fn profile_frame(
        &mut self,
        repaint: bool,
        layout_time: Duration,
        paint_time: Duration,
        submit_time: Duration,
    ) {
        let Some(profiler) = self.profiler.as_mut() else {
            return;
        };
        let commands = self
            .draw_list
            .as_ref()
            .map_or(0, |list| list.commands().len());
        let millis = |time: Duration| time.as_secs_f32() * 1000.0;
        let stats = FrameStats {
            index: profiler.next_index(),
            frame_ms: millis(layout_time) + millis(paint_time) + millis(submit_time),
            stages: StageTimes::new(
                0.0,
                millis(layout_time),
                millis(paint_time),
                millis(submit_time),
            ),
            counters: FrameCounters::new(self.ui.tree().node_count(), 0, commands, 1),
        };

        if let Some(list) = self.draw_list.as_ref() {
            let report = inspect(list, &stats);
            for finding in report.findings() {
                // `Info` findings (e.g. a zero-glyph text command) are not
                // actionable per frame; keep the log to warnings and errors.
                if finding.severity == Severity::Info {
                    continue;
                }
                eprintln!(
                    "cgb perf [{}] {}",
                    finding.severity.label(),
                    finding.summary()
                );
            }
        }

        eprintln!(
            "cgb perf: repaint={repaint} layout={:.3}ms paint={:.3}ms submit={:.3}ms \
             nodes={} commands={commands}",
            stats.stages.layout_ms,
            stats.stages.paint_ms,
            stats.stages.render_ms,
            stats.counters.scene_nodes,
        );

        profiler.record(stats);
        if profiler.total_recorded() % PERF_REPORT_FRAMES == 0 {
            if let Some(summary) = profiler.summary() {
                eprintln!(
                    "cgb perf summary: {} frames avg={:.2}ms max={:.2}ms fps={:.1} \
                     layout={:.2}ms paint={:.2}ms submit={:.2}ms commands<={}",
                    summary.frames,
                    summary.avg_frame_ms,
                    summary.max_frame_ms,
                    summary.fps(),
                    summary.avg_stages.layout_ms,
                    summary.avg_stages.paint_ms,
                    summary.avg_stages.render_ms,
                    summary.max_draw_commands,
                );
            }
        }
    }
}

impl AppLogic for App {
    fn init(&mut self, ctx: &InitContext<'_>) {
        if let Some(backend) = ctx.service::<SharedBackend>() {
            self.backend = Some(backend.clone());
            backend
                .borrow_mut()
                .set_clear_color(self.theme.background());
        }
        if let Some(window) = ctx.service::<SharedWindow>() {
            self.window = window.borrow().clone();
        }
        if let Some(measurer) = ctx.service::<Rc<dyn TextMeasurer>>() {
            self.measurer = Some(measurer.clone());
        }
        // The window exists now, so its platform chrome (the macOS title bar)
        // can be reserved in the header.
        self.model.safe_area = safe_area();
        self.dirty = true;
        // Covers and icons could not be uploaded before the backend existed.
        self.install_icon_textures();
        self.refresh_cover_textures();
        self.refresh_screenshot_textures();
        self.rebuild_game_rows();
        self.rebuild_screenshot_rows();
        // A `--rom` on the command line starts eagerly, before the first frame.
        if let Some(pending) = self.pending_rom.take() {
            self.start_path(&pending);
        }
    }

    fn event(&mut self, _ctx: &EventContext<'_>, event: &InputEvent) -> EventResult {
        if let InputEvent::ModifiersChanged(modifiers) = event {
            self.modifiers = *modifiers;
            return EventResult::Ignored;
        }
        self.handle_hotkey(event);
        self.feed(event);
        // A search filters as it is typed: read the field back and rebuild the
        // rows when the query changed.
        if self
            .model
            .editing
            .as_ref()
            .is_some_and(|edit| edit.kind == EditKind::Search)
        {
            let text = self.actions.edit_text();
            if text != self.model.search {
                self.model.search = text;
                self.rebuild_game_rows();
                self.dirty = true;
            }
        }
        EventResult::Handled
    }

    fn update(&mut self, ctx: &FrameContext<'_>) {
        self.advance_frame(ctx.delta());
    }

    fn layout(&mut self, ctx: &FrameContext<'_>) {
        // Re-layout only when the viewport actually changed: a fullscreen
        // transition fires many `Resized` events, and a repeated same-size one
        // must not re-lay-out the (heavy) library shell.
        let viewport = ctx.viewport();
        if self.last_viewport != Some(viewport) {
            self.last_viewport = Some(viewport);
            self.repaint = true;
            if let Some(transition) = self.transition.as_mut() {
                transition.last_activity = Instant::now();
            }
        }
        self.layout_ui(viewport);
    }

    fn paint(&mut self, _ctx: &FrameContext<'_>, paint: &mut PaintContext) {
        self.paint_ui(paint);
    }

    fn needs_frame(&self) -> bool {
        // A running game drives its own clock; an overlay timer or a held
        // rewind key needs frames too. Otherwise the loop waits for an event.
        self.rewinding
            || self.transition.is_some()
            || self.pending_fullscreen.is_some()
            || self.ui.overlays_animating()
            || self
                .session
                .as_ref()
                .is_some_and(|session| !session.paused())
    }

    fn caret(&self) -> Option<Rect> {
        focused_caret(self.ui.tree())
    }
}

impl Drop for App {
    fn drop(&mut self) {
        // The stock runner exits without a hook, so bank the running game's
        // play time here (it is also flushed every 15 seconds).
        self.flush_playtime();
    }
}

/// Wall-clock milliseconds since the Unix epoch.
fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// Reveal a file in the platform file browser (Finder on macOS).
fn reveal_path(path: &Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn();
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(parent) = path.parent() {
            open_path(parent);
        }
    }
}

/// Open a directory in the platform file browser.
fn open_path(path: &Path) {
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(not(target_os = "macos"))]
    let opener = "xdg-open";
    let _ = std::process::Command::new(opener).arg(path).spawn();
}

/// The core manifest: the packaged `<app data>/cores/cores.json`, else the
/// bundle's `Resources/cores/cores.json`, else the dev checkout's
/// `cores/cores.json`. Missing is not an error — the app then just has no core
/// to run, and says so when a game is started.
fn load_core_manifest(paths: &Paths) -> Vec<CoreSpec> {
    let packaged = paths.cores.join("cores.json");
    if packaged.is_file() {
        return load_cores(&packaged);
    }
    if let Some(resources) = resource_dir() {
        let bundled = resources.join("cores/cores.json");
        if bundled.is_file() {
            return load_cores(&bundled);
        }
    }
    load_cores(Path::new("cores/cores.json"))
}

/// The macOS bundle's `Contents/Resources`, when running from a packaged app
/// (`Contents/MacOS/<exe>` → `Contents/Resources`). `None` otherwise, so a dev
/// checkout falls back to paths relative to the working directory.
pub(crate) fn resource_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let resources = exe.parent()?.parent()?.join("Resources");
    resources.is_dir().then_some(resources)
}

/// Map the UI preset to the backend's effect.
fn texture_effect(kind: ShaderKind) -> TextureEffect {
    match kind {
        ShaderKind::Off => TextureEffect::None,
        ShaderKind::Scanlines => TextureEffect::Scanlines,
        ShaderKind::Crt => TextureEffect::Crt,
        ShaderKind::Lcd => TextureEffect::Lcd,
        ShaderKind::Sharpen => TextureEffect::Sharpen,
    }
}

/// Order games the way the library shows them: pinned first, then the sort
/// key within each group. Pure, so the ordering can be tested without a window.
fn order_games(games: &mut [Game], key: SortKey, desc: bool) {
    games.sort_by(|a, b| {
        let order = match key {
            SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortKey::Size => a.size.cmp(&b.size),
            SortKey::LastPlayed => a.last_played_at.cmp(&b.last_played_at),
            SortKey::Playtime => a.play_seconds.cmp(&b.play_seconds),
            SortKey::Added => a.added_at.cmp(&b.added_at),
        };
        let order = if desc { order.reverse() } else { order };
        b.pinned.cmp(&a.pinned).then(order)
    });
}

/// How many games the library holds per console, in `SYSTEMS` order, keeping
/// only the consoles that have games. Pure, so the tally can be tested without
/// a window.
fn system_counts(games: &[Game]) -> Vec<SystemCount> {
    cgb_systems::SYSTEMS
        .iter()
        .filter_map(|&system| {
            let count = games.iter().filter(|game| game.system == system).count();
            (count > 0).then_some(SystemCount { system, count })
        })
        .collect()
}

/// Whether `game` matches a library search query.
///
/// Whitespace-separated tokens are AND-ed. A token starting with `#` matches a
/// tag (substring, case-insensitive); any other token matches the display name
/// (substring, case-insensitive). So `mario #rpg` finds names containing
/// "mario" that also carry an "rpg" tag.
fn game_matches_search(game: &Game, query: &str) -> bool {
    query.split_whitespace().all(|token| {
        if let Some(tag) = token.strip_prefix('#') {
            let tag = tag.to_lowercase();
            tag.is_empty()
                || game
                    .tags
                    .iter()
                    .any(|candidate| candidate.to_lowercase().contains(&tag))
        } else {
            game.name.to_lowercase().contains(&token.to_lowercase())
        }
    })
}

/// Project a library row into the view model's row.
fn game_row(game: Game, cover: Option<FrameHandle>) -> GameRow {
    GameRow {
        id: game.id,
        name: game.name,
        file_name: game.file_name,
        system: game.system,
        path: game.path,
        size: game.size,
        pinned: game.pinned,
        play_count: game.play_count,
        play_seconds: game.play_seconds,
        last_played_at: game.last_played_at,
        tags: game.tags,
        screenshots: game.screenshots,
        cover,
    }
}

/// One row per joypad button that has keys bound, for the settings page.
fn binding_rows(bindings: &KeyboardBindings) -> Vec<BindingRow> {
    JoypadButton::ALL
        .iter()
        .filter_map(|button| {
            let keys: Vec<String> = bindings
                .entries()
                .iter()
                .filter(|(_, bound)| bound == button)
                .map(|(key, _)| key_label(*key))
                .collect();
            (!keys.is_empty()).then(|| BindingRow {
                button: button.label().to_string(),
                keys: keys.join(" / "),
            })
        })
        .collect()
}

/// A key's printable name for the bindings list.
fn key_label(key: Key) -> String {
    match key {
        Key::Character(c) => c.to_ascii_uppercase().to_string(),
        Key::Enter => "Enter".to_string(),
        Key::Escape => "Esc".to_string(),
        Key::Backspace => "Backspace".to_string(),
        Key::Delete => "Delete".to_string(),
        Key::Tab => "Tab".to_string(),
        Key::Space => "Space".to_string(),
        Key::Home => "Home".to_string(),
        Key::End => "End".to_string(),
        Key::ArrowUp => "↑".to_string(),
        Key::ArrowDown => "↓".to_string(),
        Key::ArrowLeft => "←".to_string(),
        Key::ArrowRight => "→".to_string(),
        Key::F1 => "F1".to_string(),
        Key::F2 => "F2".to_string(),
        Key::F3 => "F3".to_string(),
        Key::F4 => "F4".to_string(),
        Key::F5 => "F5".to_string(),
        Key::F6 => "F6".to_string(),
        Key::F7 => "F7".to_string(),
        Key::F8 => "F8".to_string(),
        Key::F9 => "F9".to_string(),
        Key::F10 => "F10".to_string(),
        Key::F11 => "F11".to_string(),
        Key::F12 => "F12".to_string(),
    }
}

/// The status line after adding games: how many were copied and the first
/// reason any were skipped.
fn import_status(report: &ImportReport) -> String {
    let copied = report.copied_count();
    let skipped = report.skipped_count();
    let mut parts = Vec::new();
    if copied > 0 {
        parts.push(format!("已添加 {copied} 个游戏到游戏库"));
    }
    if let Some((path, error)) = report.failed.first() {
        parts.push(format!("拷贝失败 {}（{error}）", path.display()));
    }
    if parts.is_empty() {
        return match (report.unknown.first(), report.already_inside.is_empty()) {
            (Some(path), _) => format!("跳过不认识的 ROM：{}", path.display()),
            (None, false) => "这些游戏已经在游戏库里了".to_string(),
            _ => "没有新增游戏".to_string(),
        };
    }
    let mut message = parts.join("，");
    if skipped > 0 {
        message.push_str(&format!("（跳过 {skipped} 个）"));
    }
    message
}

/// True when `path` is `root` itself or inside it. Both sides are canonicalized
/// first, so a `..` segment cannot make an outside path look like it is inside.
fn inside_library(root: &Path, path: &Path) -> bool {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    path.starts_with(&root)
}

/// The save-state hotkey for a key, matching the old front end's layout: `F5`
/// quick-saves, `F6` quick-loads, `F1`–`F3` save slots 1–3, and
/// `Shift`+`F1`–`F3` loads them. `F11` toggles fullscreen.
fn state_shortcut(key: Key, shift: bool) -> Option<Action> {
    match key {
        Key::F11 => Some(Action::ToggleFullscreen),
        Key::F5 => Some(Action::SaveState(0)),
        Key::F6 => Some(Action::LoadState(0)),
        Key::F1 if shift => Some(Action::LoadState(1)),
        Key::F2 if shift => Some(Action::LoadState(2)),
        Key::F3 if shift => Some(Action::LoadState(3)),
        Key::F1 => Some(Action::SaveState(1)),
        Key::F2 => Some(Action::SaveState(2)),
        Key::F3 => Some(Action::SaveState(3)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
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
        let names =
            |games: &[Game]| -> Vec<String> { games.iter().map(|g| g.name.clone()).collect() };
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
        assert_eq!(state_shortcut(Key::F5, false), Some(Action::SaveState(0)));
        assert_eq!(state_shortcut(Key::F6, false), Some(Action::LoadState(0)));
        assert_eq!(state_shortcut(Key::F1, false), Some(Action::SaveState(1)));
        assert_eq!(state_shortcut(Key::F1, true), Some(Action::LoadState(1)));
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
}

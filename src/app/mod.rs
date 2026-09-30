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
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use igui::igui_app::{
    App as IguiApp, AppBuilder, AppConfig, AppLogic, EventContext, EventResult, FrameContext,
    InitContext, PlatformEvent, PlatformObserver, Plugin,
};
use igui::igui_backend_wgpu::{FontConfig, FontMode, TextureEffect};
use igui::igui_core::{Cursor, InputEvent, Key, Modifiers, Rect, ViewportSize};
use igui::igui_profile::{inspect, FrameCounters, FrameStats, Profiler, Severity, StageTimes};
use igui::igui_render::{DrawList, PaintContext, TextureId};
use igui::igui_theme::{Mode, Theme};
use igui::igui_ui::{focused_caret, TextEdit, TextMeasurer};
use igui_winit::{
    ClipboardPlugin, GpuConfig, ImePlugin, KeyboardPlugin, PointerPlugin, SharedBackend,
    SharedWindow, TextMeasurePlugin, TitlebarMode, WgpuPlugin, WindowConfig,
};
use winit::event::WindowEvent;
use winit::window::{Fullscreen, Window};

use crate::ui::{
    library_columns, Action, Actions, BindingRow, CatalogRow, Confirm, CoreOptionRow, CoreRow,
    EditKind, EditState, FrameHandle, GameRow, InputDescriptorRow, MsaaKind, SafeArea, SaveSlotRow,
    ScreenshotRow, Section, ShaderKind, SortKey, StatusKind, SystemCount, ThemeChoice, Ui,
    ViewModel, MIDDLE_MAX_WIDTH, MIDDLE_MIN_WIDTH,
};
use cgb_input::{Gamepads, InputState, KeyboardBindings};
use cgb_library::{
    cache_path, collect_games, decode_png, download_core_with_progress, encode_png, import_roms,
    load_cores, register_downloaded, registry_path, seed_dir, seed_dir_recursive, update_catalog,
    write_catalog, Catalog, Game, ImportReport, Library, Paths, Platform, Settings, DEFAULT_SOURCE,
};
use cgb_systems::{choose_core, system_for_path, CoreSpec, JoypadButton, SystemId};

use crate::cli::{Args, CoreOverride};
use crate::session::Session;

mod cheats;
mod cores;
mod helpers;
mod input;
mod library;
mod project;
mod saves;
mod screenshots;
mod settings;
#[cfg(test)]
mod tests;
mod textures;
mod window;

pub(crate) use helpers::*;
pub(crate) use project::*;

/// The shortest gap between grid column-count recomputations while the middle
/// divider is dragged. A column change rebuilds the tree; throttling keeps a
/// drag from rebuilding on every pointer move.
const COLUMNS_CHECK_INTERVAL: Duration = Duration::from_millis(80);

/// How often `CGB_PERF` prints the aggregate frame summary (roughly two seconds
/// at 60 FPS).
const PERF_REPORT_FRAMES: u64 = 120;

/// Cover textures start above the game framebuffer's id, one per game.
const COVER_TEXTURE_BASE: u32 = 0x1000;

/// How many catalog rows the settings page mounts at once. The whole list is
/// hundreds of cores; the search box narrows it, and this caps the layout cost.
const CATALOG_LIST_LIMIT: usize = 40;

/// What a background core download reports back to the UI thread.
enum DownloadEvent {
    /// Bytes received, and the total when the server sent `Content-Length`.
    Progress { received: u64, total: Option<u64> },
    /// A download finished; `registered` is whether the core joined the manifest.
    Done { name: String, registered: bool },
    /// A download or catalog refresh failed.
    Failed { name: String, error: String },
    /// The catalog cache was rewritten.
    CatalogRefreshed(Box<Catalog>),
}

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

/// The FreeJ2ME-Plus bundle: beside the core dylib it carries the
/// `freej2me_plus-lr.jar` the core loads and a `jlink`-trimmed JRE under
/// `runtime/`. Built by `cores/freej2me_plus/build.sh`; packaged into the app's
/// Resources (`freej2me_plus/`). The core starts a Java VM from the system
/// `PATH`, so the runtime is exposed that way rather than copied.
const BUNDLED_J2ME: &str = "freej2me_plus";

/// The PPSSPP assets (`compat.ini`, fonts, `shaders/`, `lang/`, …). Built into
/// `cores/dist/ppsspp/` by `cores/ppsspp/build.sh` and packaged into the app's
/// Resources (`ppsspp/`). The core reads them from `<system dir>/PPSSPP/`, so
/// they are seeded there — without `compat.ini` it warns at init.
const BUNDLED_PPSSPP: &str = "ppsspp";

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
    /// The last card single click (index, time), so two in quick succession
    /// start the game — a card plays on double click, the play button on one.
    card_click: Option<(usize, i64)>,
    /// Whether the rewind key is held (the game steps back each frame).
    rewinding: bool,
    /// The game-picture post-process preset.
    shader: ShaderKind,
    /// Geometry anti-aliasing (MSAA) mode.
    msaa: MsaaKind,
    gamepads: Option<Gamepads>,
    session: Option<Session>,
    /// `--rom` path to start once the window exists.
    pending_rom: Option<PathBuf>,
    /// `--core` override: a core key or a module path, applied to every game
    /// this run starts.
    core_override: Option<CoreOverride>,
    /// Every core declared in `cores.json`, in manifest order.
    cores: Vec<CoreSpec>,
    /// The downloadable-core catalog (the user cache, else the built-in
    /// snapshot), for the settings page's search list.
    catalog: Catalog,
    /// A background download / catalog refresh in flight, with its events.
    download_rx: Option<Receiver<DownloadEvent>>,
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
    /// A fullscreen request waiting for the hidden tree to be presented first,
    /// so the OS animates from a settled surface rather than the old UI.
    pending_fullscreen: Option<bool>,
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
        // J2ME runs the game in a child Java VM: seed the jar into the system
        // dir the core reads, and put the bundled JRE first on `PATH` so the
        // core's `execvp("java")` finds it. Without a bundle (never built) the
        // `PATH` is left alone, so a system `java` still works.
        if let Some(j2me) = j2me_dir() {
            // The shipped jar always wins: it is ours, not player data, so a
            // rebuilt jar (with an upstream fix, say) replaces the copy in the
            // system dir. `seed_dir` would skip it, since it keeps existing
            // files to protect a player-supplied BIOS.
            let jar = j2me.join("freej2me_plus-lr.jar");
            if jar.is_file() {
                let _ = std::fs::copy(&jar, paths.system.join("freej2me_plus-lr.jar"));
            }
            if let Ok(runtime_bin) = std::fs::canonicalize(j2me.join("runtime/bin")) {
                prepend_path(&runtime_bin);
            }
        }
        // PPSSPP reads its assets from `<system dir>/PPSSPP/`: `compat.ini`
        // (per-game compatibility fixes) plus fonts, shaders and translations
        // for its own screens. They are a tree, so seed recursively; a file the
        // player drops there wins.
        if let Some(ppsspp) = ppsspp_assets_dir() {
            let _ = seed_dir_recursive(&ppsspp, &paths.system.join("PPSSPP"));
        }
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
        let msaa = MsaaKind::from_key(&settings.msaa);
        let middle_width = if settings.middle_width > 0.0 {
            settings
                .middle_width
                .clamp(MIDDLE_MIN_WIDTH, MIDDLE_MAX_WIDTH)
        } else {
            320.0
        };
        let library = Library::open(&paths.library_db).ok();
        let cores = load_core_manifest(&paths);
        let catalog = Catalog::load(&cache_path(&paths.cores));

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
            card_click: None,
            rewinding: false,
            shader,
            msaa,
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
            catalog,
            download_rx: None,
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
            let system = system_for_path(&pending.to_string_lossy());
            self.start_path(&pending, system);
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
        if let Some(kind) = self.model.editing.as_ref().map(|edit| edit.kind) {
            let text = self.actions.edit_text();
            match kind {
                EditKind::Search => {
                    if text != self.model.search {
                        self.model.search = text;
                        self.rebuild_game_rows();
                        self.dirty = true;
                    }
                }
                EditKind::CatalogSearch => {
                    if text != self.model.catalog_query {
                        self.model.catalog_query = text;
                        self.rebuild_catalog();
                        self.dirty = true;
                    }
                }
                EditKind::Name | EditKind::Tags => {}
            }
        }
        EventResult::Handled
    }

    /// Hand the window the cursor for whatever the pointer is over, so a card
    /// or button shows the pointing hand instead of the arrow.
    fn cursor(&self) -> Option<Cursor> {
        self.ui.cursor()
    }

    fn update(&mut self, ctx: &FrameContext<'_>) {
        self.poll_downloads();
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
            || self.download_rx.is_some()
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

/// The macOS bundle's `Contents/Resources`, when running from a packaged app
/// (`Contents/MacOS/<exe>` → `Contents/Resources`). `None` otherwise, so a dev
/// checkout falls back to paths relative to the working directory.
pub(crate) fn resource_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let resources = exe.parent()?.parent()?.join("Resources");
    resources.is_dir().then_some(resources)
}

//! The `winit` + `wgpu` host: the frame loop and the wiring between the UI,
//! the libretro core, the audio device and the gamepad.
//!
//! quill is event-driven by default; an emulator is not. While a game is
//! running the loop schedules a redraw at the core's frame rate
//! (`ControlFlow::WaitUntil`); with no game, or when paused, it falls back to
//! `Wait` and does no work. See `docs/architecture/quill-native-migration.md`
//! §8.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use draw_backend_wgpu::{wgpu, FontConfig, FontMetrics, FontMode, TextureEffect, WgpuBackend};
use draw_core::{FontWeight, InputEvent, Key, PointerButton, Size, Vec2, ViewportSize};
use draw_profile::{inspect, FrameCounters, FrameStats, Profiler, StageTimes};
use draw_render::{DrawList, PaintContext, RenderBackend, TextureId};
use draw_theme::{default_theme, Mode, Theme};
use draw_ui::TextMeasurer;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};
#[cfg(target_os = "macos")]
use winit::platform::macos::WindowAttributesExtMacOS;
use winit::window::{Fullscreen, Window, WindowId};

use cgb_input::{Gamepads, InputState, KeyboardBindings};
use cgb_library::{
    collect_games, decode_png, encode_png, import_roms, load_cores, seed_dir, Game, ImportReport,
    Library, Paths, Settings,
};
use cgb_systems::{choose_core, system_for_path, CoreSpec, JoypadButton, SystemId};
use cgb_ui::{
    library_columns, Action, Actions, BindingRow, Confirm, CoreOptionRow, CoreRow, EditKind,
    EditState, FrameHandle, GameRow, InputDescriptorRow, SafeArea, SaveSlotRow, ScreenshotRow,
    Section, ShaderKind, SortKey, Ui, ViewModel, MIDDLE_MAX_WIDTH, MIDDLE_MIN_WIDTH,
};

use crate::cli::{Args, CoreOverride};
use crate::session::Session;

/// One wheel notch scrolls about three text lines.
const WHEEL_LINE_HEIGHT: f32 = 48.0;

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

/// Runs the app until the window closes.
pub fn run(args: Args) {
    let event_loop = EventLoop::new().expect("create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::new(args);
    event_loop.run_app(&mut app).expect("run event loop");
}

/// Owns everything, one frame at a time.
struct App {
    instance: wgpu::Instance,
    window: Option<Arc<Window>>,
    surface: Option<wgpu::Surface<'static>>,
    backend: Option<WgpuBackend>,
    config: Option<wgpu::SurfaceConfiguration>,
    scale_factor: f64,
    cursor: Vec2,

    theme: &'static dyn Theme,
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
    modifiers: ModifiersState,
    /// Files dropped onto the window since the last frame. winit delivers one
    /// `DroppedFile` event per file, so they are buffered and added in one batch.
    pending_drops: Vec<PathBuf>,

    last_frame: Instant,
    /// When the running game's play time was last flushed to the database.
    last_play_flush: Instant,
    /// When the grid's column count was last recomputed from the middle width.
    /// Throttles the rebuild a column change triggers while the divider is
    /// dragged.
    last_columns_check: Instant,
}

impl App {
    fn new(args: Args) -> Self {
        let paths = Paths::platform();
        let _ = paths.ensure();
        // Arcade cores need a BIOS. Seed the writable system dir the core
        // actually reads from the bundled assets; a player-supplied file wins.
        // A packaged app keeps its assets in the bundle's Resources.
        let bundled_arcade = resource_dir()
            .map(|resources| resources.join(BUNDLED_ARCADE_SYSTEM))
            .filter(|dir| dir.is_dir())
            .unwrap_or_else(|| PathBuf::from(BUNDLED_ARCADE_SYSTEM));
        let _ = seed_dir(&bundled_arcade, &paths.system);
        let settings = Settings::load(&paths.settings_json);
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
        let theme = default_theme(Mode::Dark);
        let model = ViewModel {
            core_name: "—".to_string(),
            middle_width,
            grid_columns: library_columns(middle_width),
            ..ViewModel::default()
        };
        let ui = Ui::new(theme, &model, &actions);

        let mut app = Self {
            instance: wgpu::Instance::default(),
            window: None,
            surface: None,
            backend: None,
            config: None,
            scale_factor: 1.0,
            cursor: Vec2::ZERO,
            theme,
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
            input: InputState::new(),
            bindings: cgb_systems::SYSTEMS
                .iter()
                .map(|system| (*system, KeyboardBindings::default_bindings()))
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
            modifiers: ModifiersState::empty(),
            pending_drops: Vec::new(),
            last_frame: Instant::now(),
            last_play_flush: Instant::now(),
            last_columns_check: Instant::now(),
        };
        app.refresh_library();
        app.rebuild_settings_view();
        app
    }

    /// Create the window, surface, backend and swap chain on first resume.
    fn init(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let mut attributes = Window::default_attributes()
            .with_title("Classic Game Box")
            .with_inner_size(LogicalSize::new(1100.0, 760.0));
        // The content runs under a transparent, title-less macOS title bar; the
        // header reserves the safe area so nothing hides behind the traffic
        // lights.
        #[cfg(target_os = "macos")]
        {
            attributes = attributes
                .with_titlebar_transparent(true)
                .with_title_hidden(true)
                .with_fullsize_content_view(true);
        }
        let window = Arc::new(event_loop.create_window(attributes).expect("create window"));

        let surface = self
            .instance
            .create_surface(window.clone())
            .expect("create surface");
        let mut backend = WgpuBackend::from_instance(
            &self.instance,
            Some(&surface),
            wgpu::PowerPreference::HighPerformance,
        )
        .expect("create wgpu backend");

        let capabilities = surface.get_capabilities(backend.adapter());
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| !format.is_srgb())
            .unwrap_or(capabilities.formats[0]);

        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: capabilities.alpha_modes[0],
            view_formats: Vec::new(),
        };
        surface.configure(backend.device(), &config);

        self.scale_factor = window.scale_factor();
        backend.set_scale_factor(self.scale_factor as f32);
        backend.set_clear_color(draw_core::Color::new(0.039, 0.039, 0.039, 1.0));
        if let Err(error) = backend.set_font_config(FontConfig {
            mode: FontMode::System,
            device_pixel_rasterization: true,
            ..Default::default()
        }) {
            eprintln!("font setup failed, using fallback: {error}");
        }

        self.window = Some(window);
        self.surface = Some(surface);
        self.backend = Some(backend);
        self.config = Some(config);
        self.last_frame = Instant::now();
        // The window exists now, so its platform chrome (the macOS title bar)
        // can be reserved in the header.
        self.model.safe_area = safe_area();

        // The real font metrics can only be installed once the backend exists.
        self.dirty = true;

        // Covers could not be uploaded before the backend existed; do it now.
        self.install_icon_textures();
        self.refresh_cover_textures();
        self.refresh_screenshot_textures();
        self.rebuild_game_rows();
        self.rebuild_screenshot_rows();

        // A `--rom` on the command line starts eagerly, before the first frame.
        if let Some(pending) = self.pending_rom.take() {
            self.start_path(&pending);
        }
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    /// Re-read the library folders and rebuild the game rows.
    ///
    /// The folder is the truth about what exists, so every refresh rescans it
    /// and reconciles the database: new files are inserted, vanished files are
    /// dropped, changed files have their facts refreshed. The rows then come
    /// from the database, which is the model — the name, pin, play statistics,
    /// screenshots and cover a scan cannot know live there.
    fn refresh_library(&mut self) {
        // Configured folders plus the built-in ROM folder.
        let mut dirs: Vec<PathBuf> = self
            .settings
            .library_dirs
            .iter()
            .map(PathBuf::from)
            .collect();
        dirs.push(self.paths.roms.clone());

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
        let Some(backend) = self.backend.as_mut() else {
            return;
        };
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
        let Some(backend) = self.backend.as_mut() else {
            return;
        };
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
        let Some(backend) = self.backend.as_mut() else {
            return;
        };
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
        self.model.status = "已设为封面".to_string();
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
        self.model.status = "已删除截图".to_string();
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
        self.model.status = format!("已删除 {count} 张截图");
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
        self.model.status = match self.session.as_ref() {
            Some(session) => match session.save_state(slot) {
                Ok(()) => format!("已存档（槽位 {}）", slot + 1),
                Err(error) => error,
            },
            None => "没有正在运行的游戏".to_string(),
        };
        self.refresh_saves();
        self.dirty = true;
    }

    /// Load a save state from a slot.
    fn load_from_slot(&mut self, slot: u8) {
        self.model.status = match self.session.as_ref() {
            Some(session) => match session.load_state(slot) {
                Ok(()) => format!("已读档（槽位 {}）", slot + 1),
                Err(error) => error,
            },
            None => "没有正在运行的游戏".to_string(),
        };
        self.dirty = true;
    }

    /// Delete a slot's state and thumbnail.
    fn delete_slot(&mut self, slot: u8) {
        if let Some(session) = self.session.as_ref() {
            session.delete_save(slot);
        }
        self.model.status = format!("已删除存档（槽位 {}）", slot + 1);
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
                    if let Some(backend) = self.backend.as_mut() {
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
            self.model.status = "没有正在运行的游戏".to_string();
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
            self.model.status = "读取金手指文件失败".to_string();
            self.dirty = true;
            return;
        };
        let cheats = cgb_library::parse_cht(&text);
        if cheats.is_empty() {
            self.model.status = "文件里没有可用的金手指".to_string();
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
        self.model.status = format!("已导入 {count} 条金手指");
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
        self.model.status = format!("{}：{desc}", if enabled { "已开启" } else { "已关闭" });
    }

    /// Pick the picture post-process, persist it, and apply it.
    fn set_shader(&mut self, kind: ShaderKind) {
        self.shader = kind;
        self.settings.shader = kind.key().to_string();
        let _ = self.settings.save(&self.paths.settings_json);
        self.apply_shader();
        self.rebuild_settings_view();
        self.model.status = format!("画面效果：{}", kind.label());
    }

    /// Apply the current preset to the running game's texture.
    fn apply_shader(&mut self) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let Some(backend) = self.backend.as_mut() else {
            return;
        };
        session.set_effect(backend, texture_effect(self.shader));
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
        self.model.status = format!("{option_key} = {display}");
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
        let mut games = self.game_source.clone();
        if !self.model.search.is_empty() {
            let needle = self.model.search.to_lowercase();
            games.retain(|game| game.name.to_lowercase().contains(&needle));
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
        self.model.library_dirs = self.settings.library_dirs.clone();
        self.model.bindings = self
            .bindings
            .get(&self.active_system)
            .map(binding_rows)
            .unwrap_or_default();
        self.model.bindings_system = self.active_system.name().to_string();
        self.model.shader = self.shader;
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

    /// Ask for a folder and add it to the scanned library.
    fn add_library_dir(&mut self) {
        let Some(dir) = rfd::FileDialog::new()
            .set_title("选择游戏目录")
            .pick_folder()
        else {
            return;
        };
        let dir = dir.to_string_lossy().into_owned();
        if !self.settings.library_dirs.contains(&dir) {
            self.settings.library_dirs.push(dir);
            let _ = self.settings.save(&self.paths.settings_json);
        }
        self.reload_library();
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
    /// A dropped folder joins the scanned folders; a ROM file is **copied**
    /// into the library folder, so the library stays one self-contained
    /// folder. This is the legacy front end's rule (`Library.add`): a game
    /// that was only pointed at would break the moment its file moved.
    fn add_game_paths(&mut self, paths: Vec<PathBuf>) {
        let mut dirs_added = 0usize;
        let mut files = Vec::new();
        for path in paths {
            if path.is_dir() {
                let dir = path.to_string_lossy().into_owned();
                if !self.settings.library_dirs.contains(&dir) {
                    self.settings.library_dirs.push(dir);
                    dirs_added += 1;
                }
            } else {
                files.push(path);
            }
        }
        let report = import_roms(&self.paths.roms, &files);
        if dirs_added > 0 {
            let _ = self.settings.save(&self.paths.settings_json);
        }
        self.reload_library();
        self.model.status = import_status(&report, dirs_added);
    }

    /// Add any files dropped since the last frame, in one batch.
    fn flush_drops(&mut self) {
        if self.pending_drops.is_empty() {
            return;
        }
        let paths = std::mem::take(&mut self.pending_drops);
        self.add_game_paths(paths);
    }

    /// Stop scanning a library folder and forget its games.
    fn remove_library_dir(&mut self, index: usize) {
        if index >= self.settings.library_dirs.len() {
            return;
        }
        self.settings.library_dirs.remove(index);
        let _ = self.settings.save(&self.paths.settings_json);
        self.reload_library();
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
        self.model.status = message;
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
                self.model.status = format!("删除失败：{error}");
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
        self.model.status = format!("已删除：{name}");
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
        self.model.status = format!("{} 的核心已切换为 {key}", system.short());
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
            self.model.status = "没有正在运行的游戏".to_string();
            self.dirty = true;
            return;
        };
        let rom_path = session.rom_path().to_string_lossy().into_owned();
        let Some((width, height, pixels)) = session.last_pixels() else {
            self.model.status = "还没有画面可以截图".to_string();
            self.dirty = true;
            return;
        };
        let png = match encode_png(width, height, pixels) {
            Ok(png) => png,
            Err(error) => {
                self.model.status = format!("截图失败：{error}");
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
                self.model.status = "游戏库不可用".to_string();
                self.dirty = true;
                return;
            }
        };
        match saved {
            Ok(Some(_)) => {
                self.model.status = if as_cover {
                    "已截图并设为封面".to_string()
                } else {
                    "已截图".to_string()
                };
                self.reload_from_db();
            }
            Ok(None) => {
                self.model.status = "该游戏不在游戏库中".to_string();
                self.dirty = true;
            }
            Err(error) => {
                self.model.status = format!("截图失败：{error}");
                self.dirty = true;
            }
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        let (Some(surface), Some(backend), Some(config)) = (
            self.surface.as_ref(),
            self.backend.as_ref(),
            self.config.as_mut(),
        ) else {
            return;
        };
        if width == 0 || height == 0 {
            return;
        }
        // A resize event often repeats the same size; reconfiguring the surface
        // for each one is the expensive part, so skip it when nothing changed.
        if config.width == width && config.height == height {
            return;
        }
        config.width = width;
        config.height = height;
        surface.configure(backend.device(), config);
        // The viewport changed, so the layout and the draw list must be redone.
        self.ui.request_repaint();
    }

    /// Route one input event: the UI first, then the emulator bindings.
    fn feed(&mut self, event: &InputEvent) {
        let scroll_before = self.ui.scroll_offset();
        self.ui.route_input(event);
        // The keyboard feeds the binding set for the console in the machine.
        if let Some(bindings) = self.bindings.get(&self.active_system) {
            match event {
                InputEvent::KeyDown { key } => bindings.apply(*key, true, &mut self.input, 0),
                InputEvent::KeyUp { key } => bindings.apply(*key, false, &mut self.input, 0),
                _ => {}
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
        for action in self.actions.drain() {
            match action {
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
                Action::CycleCoreOption(index, delta) => self.cycle_core_option(index, delta),
                Action::SaveState(slot) => {
                    self.model.status = match self.session.as_ref() {
                        Some(session) => match session.save_state(slot) {
                            Ok(()) => format!("已存档（槽位 {slot}）"),
                            Err(error) => error,
                        },
                        None => "没有正在运行的游戏".to_string(),
                    };
                    self.dirty = true;
                }
                Action::LoadState(slot) => {
                    self.model.status = match self.session.as_ref() {
                        Some(session) => match session.load_state(slot) {
                            Ok(()) => format!("已读档（槽位 {slot}）"),
                            Err(error) => error,
                        },
                        None => "没有正在运行的游戏".to_string(),
                    };
                    self.dirty = true;
                }
                Action::AddGames => self.add_games_dialog(),
                Action::OpenRom => self.add_library_dir(),
                Action::SelectCore(index) => self.select_core(index),
                Action::RemoveLibraryDir(index) => self.remove_library_dir(index),
                Action::TogglePin(index) => self.toggle_pin(index),
                Action::RequestDelete(confirm) => {
                    self.model.confirm = Some(confirm);
                    self.dirty = true;
                }
                Action::ConfirmDelete => self.confirm_delete(),
                Action::CancelDelete => {
                    self.model.confirm = None;
                    self.dirty = true;
                }
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
    }

    /// Run the destructive action the confirmation bar was asking about.
    fn confirm_delete(&mut self) {
        let Some(confirm) = self.model.confirm.take() else {
            return;
        };
        self.dirty = true;
        match confirm {
            Confirm::DeleteGame(id) => self.delete_game(id),
            Confirm::DeleteScreenshot(id) => self.remove_screenshot(id),
        }
    }

    /// Begin editing a game's name or tags; the app takes the keyboard.
    fn start_edit(&mut self, game_id: i64, kind: EditKind) {
        let Some(game) = self.game_source.iter().find(|game| game.id == game_id) else {
            return;
        };
        let text = match kind {
            EditKind::Name => game.name.clone(),
            EditKind::Tags => game.tags.join(", "),
            EditKind::Search => self.model.search.clone(),
        };
        let caret = text.len();
        self.model.editing = Some(EditState {
            game_id,
            kind,
            text,
            caret,
        });
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
        // A search is applied as it is typed; committing just closes it.
        if edit.kind == EditKind::Search {
            self.model.status = if self.model.search.is_empty() {
                String::new()
            } else {
                format!("搜索：{}", self.model.search)
            };
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
                let name = edit.text.trim().to_string();
                if name.is_empty() {
                    self.model.status = "名字不能为空".to_string();
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
                self.model.status = format!("已改名为：{name}");
            }
            EditKind::Tags => {
                let tags: Vec<String> = edit
                    .text
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
                self.model.status = format!("已更新标签（{} 个）", tags.len());
            }
            EditKind::Search => {}
        }
        self.rebuild_game_rows();
        self.dirty = true;
    }

    /// One key press while editing. Enter commits, Escape cancels, the arrows
    /// move the caret, Backspace deletes, and any typed text is inserted.
    fn edit_key(&mut self, event: &winit::event::KeyEvent) {
        match &event.logical_key {
            WinitKey::Named(NamedKey::Enter) => {
                self.commit_edit();
                return;
            }
            WinitKey::Named(NamedKey::Escape) => {
                self.cancel_edit();
                return;
            }
            _ => {}
        }
        let searching = self
            .model
            .editing
            .as_ref()
            .is_some_and(|edit| edit.kind == EditKind::Search);
        let Some(edit) = self.model.editing.as_mut() else {
            return;
        };
        match &event.logical_key {
            WinitKey::Named(NamedKey::Backspace) => {
                let previous = prev_boundary(&edit.text, edit.caret);
                edit.text.replace_range(previous..edit.caret, "");
                edit.caret = previous;
            }
            WinitKey::Named(NamedKey::ArrowLeft) => {
                edit.caret = prev_boundary(&edit.text, edit.caret);
            }
            WinitKey::Named(NamedKey::ArrowRight) => {
                edit.caret = next_boundary(&edit.text, edit.caret);
            }
            _ => {
                if let Some(text) = &event.text {
                    for ch in text.chars().filter(|ch| !ch.is_control()) {
                        edit.text.insert(edit.caret, ch);
                        edit.caret += ch.len_utf8();
                    }
                }
            }
        }
        // A search filters as it is typed.
        if searching {
            let text = self
                .model
                .editing
                .as_ref()
                .map(|edit| edit.text.clone())
                .unwrap_or_default();
            self.model.search = text;
            self.rebuild_game_rows();
        }
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
                self.model.status = format!("读取 ROM 失败：{error}");
                self.dirty = true;
                return;
            }
        };

        let spec = match self.resolve_core(rom_path) {
            Ok(spec) => spec,
            Err(status) => {
                self.model.status = status;
                self.dirty = true;
                return;
            }
        };

        if !spec.module.is_file() {
            self.model.status = format!(
                "找不到核心 {}：先跑 ./scripts/build-cores.sh，或用 --core 指定模块",
                spec.module.display()
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

        let backend = match self.backend.as_mut() {
            Some(backend) => backend,
            None => return,
        };
        let started = Session::start(
            &spec,
            &self.paths.system,
            &self.paths.saves,
            rom_path,
            &data,
            backend,
        );

        match started {
            Ok(session) => {
                self.model.core_name = session.core_name().to_string();
                self.model.playing = true;
                self.model.paused = false;
                self.model.status.clear();
                self.session = Some(session);
                // Cheats are per game and applied right after load.
                let cheat_path = cgb_library::cheat_file(&self.paths.cheats, rom_path);
                self.cheats = cgb_library::load_cheats(&cheat_path);
                self.cheat_path = Some(cheat_path);
                if let Some(session) = self.session.as_ref() {
                    session.apply_cheats(&self.cheats);
                }
                if !self.cheats.is_empty() {
                    self.model.status = format!("已应用 {} 条金手指", self.cheats.len());
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
                self.model.status = if self.active_system == SystemId::Arcade {
                    format!(
                        "{error}（街机 ROM 需要对应的 BIOS .zip，放到 {}）",
                        self.paths.system.display()
                    )
                } else {
                    error
                };
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
        self.set_fullscreen(!self.model.fullscreen);
    }

    /// Show or hide the immersive play view and ask the window to match.
    fn set_fullscreen(&mut self, on: bool) {
        let on = on && self.session.is_some();
        if let Some(window) = self.window.as_ref() {
            window.set_fullscreen(if on {
                Some(Fullscreen::Borderless(None))
            } else {
                None
            });
        }
        self.model.fullscreen = on;
        self.dirty = true;
        self.ui.request_repaint();
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn rebuild_ui(&mut self) {
        self.ui.rebuild(self.theme, &self.model, &self.actions);
        if let Some(backend) = self.backend.as_ref() {
            self.ui.install_measurer(Rc::new(BackendTextMeasurer {
                metrics: backend.text_metrics(),
            }));
        }
        self.dirty = false;
    }

    fn step_gamepad(&mut self) {
        if let Some(gamepads) = self.gamepads.as_mut() {
            gamepads.poll(&mut self.input);
        }
    }

    fn render(&mut self) {
        self.flush_drops();
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f64().min(0.25);
        self.last_frame = now;

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
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
        } else if let (Some(session), Some(backend)) =
            (self.session.as_mut(), self.backend.as_mut())
        {
            if !session.paused() {
                session.advance(dt, backend, &self.input);
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
            // A core message (SET_MESSAGE) goes to the status line.
            if let Some(message) = session.take_message() {
                self.model.status = message;
                self.dirty = true;
            }
        }

        if self.dirty {
            self.rebuild_ui();
        }

        let (Some(surface), Some(backend), Some(config)) = (
            self.surface.as_ref(),
            self.backend.as_mut(),
            self.config.as_ref(),
        ) else {
            return;
        };

        let logical = Size::new(
            config.width as f32 / self.scale_factor as f32,
            config.height as f32 / self.scale_factor as f32,
        );
        let viewport = ViewportSize::new(logical);

        // Rebuild the draw list only when something changed. A running game
        // updates its texture in place, so its frames re-submit the previous
        // list instead of laying out and painting the whole UI again.
        let repaint = self.ui.take_repaint() || self.draw_list.is_none();
        let mut layout_time = Duration::ZERO;
        let mut paint_time = Duration::ZERO;
        if repaint {
            let started = Instant::now();
            self.ui.layout(viewport);
            layout_time = started.elapsed();
            // The library / screenshots grids mount only the rows the viewport
            // covers, so the resolved offset and viewport go back into the
            // model; a scroll past the mounted rows asks for one more rebuild,
            // while scrolling inside them is just a repaint.
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
                        if let Some(window) = self.window.as_ref() {
                            window.request_redraw();
                        }
                    }
                }
            }
            let started = Instant::now();
            let mut ctx = PaintContext::new();
            self.ui.paint(&mut ctx);
            self.draw_list = Some(ctx.into_draw_list());
            paint_time = started.elapsed();
        }

        let surface_texture = match surface.get_current_texture() {
            Ok(texture) => texture,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                surface.configure(backend.device(), config);
                return;
            }
            Err(wgpu::SurfaceError::Timeout) => return,
            Err(error) => {
                eprintln!("surface error: {error}");
                return;
            }
        };

        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        if backend
            .begin_frame_with_view(view, config.width, config.height, config.format, viewport)
            .is_ok()
        {
            let started = Instant::now();
            if let Some(list) = self.draw_list.as_ref() {
                let _ = backend.submit(list);
            }
            let _ = backend.end_frame();
            self.profile_frame(repaint, layout_time, paint_time, started.elapsed());
        }
        surface_texture.present();
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

    /// The next time the event loop should wake, if a game is running.
    fn next_deadline(&self) -> Option<Instant> {
        let session = self.session.as_ref()?;
        if session.paused() {
            return None;
        }
        Some(Instant::now() + Duration::from_secs_f64(session.frame_seconds()))
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.init(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // A running game drives its own clock: wake at the next frame and draw.
        // Otherwise wait for an event and do nothing.
        match self.next_deadline() {
            Some(deadline) => {
                event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        if matches!(event, WindowEvent::RedrawRequested) {
            self.render();
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                self.flush_playtime();
                event_loop.exit();
                return;
            }
            WindowEvent::Resized(size) => self.resize(size.width, size.height),
            WindowEvent::DroppedFile(path) => self.pending_drops.push(path),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale_factor = scale_factor;
                if let Some(backend) = self.backend.as_mut() {
                    backend.set_scale_factor(scale_factor as f32);
                }
                self.ui.request_repaint();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = self.to_logical(position);
                self.feed(&InputEvent::PointerMove {
                    position: self.cursor,
                });
            }
            WindowEvent::CursorLeft { .. } => self.feed(&InputEvent::PointerLeave),
            WindowEvent::MouseInput { state, button, .. } => {
                let position = self.cursor;
                let button = pointer_button(button);
                let event = match state {
                    ElementState::Pressed => InputEvent::PointerDown { position, button },
                    ElementState::Released => InputEvent::PointerUp { position, button },
                };
                self.feed(&event);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let delta = wheel_pixels(delta, self.scale_factor as f32);
                let position = self.cursor;
                self.feed(&InputEvent::Wheel {
                    position,
                    delta: Vec2::new(0.0, delta),
                });
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                // While a text edit is open the app owns the keyboard: every
                // key goes to the field, not to the UI or the joypad bindings.
                if self.model.editing.is_some() {
                    if event.state == ElementState::Pressed {
                        self.edit_key(&event);
                        if let Some(window) = self.window.as_ref() {
                            window.request_redraw();
                        }
                    }
                    return;
                }
                // Backspace is the rewind key: hold it to step the game back.
                if matches!(event.logical_key, WinitKey::Named(NamedKey::Backspace)) {
                    self.rewinding = event.state == ElementState::Pressed;
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                    }
                    return;
                }
                // Save-state hotkeys are app commands, not joypad bindings, and
                // only fire on press (so a held key does not re-save).
                if event.state == ElementState::Pressed {
                    // Escape dismisses a pending delete confirmation, or leaves
                    // the immersive view. Either way it is an app command, not
                    // a joypad key.
                    if matches!(event.logical_key, WinitKey::Named(NamedKey::Escape)) {
                        if self.model.confirm.is_some() {
                            self.model.confirm = None;
                            self.dirty = true;
                        } else if self.model.fullscreen {
                            self.actions.push(Action::ToggleFullscreen);
                            self.handle_actions();
                            return;
                        }
                    }
                    if let Some(action) =
                        state_shortcut(&event.logical_key, self.modifiers.shift_key())
                    {
                        self.actions.push(action);
                        self.handle_actions();
                    }
                    // F12 is the screenshot key; Shift+F12 also sets the cover.
                    if matches!(event.logical_key, WinitKey::Named(NamedKey::F12)) {
                        self.capture_screenshot(self.modifiers.shift_key());
                    }
                }
                let Some(key) = map_key(&event.logical_key) else {
                    return;
                };
                let input = match event.state {
                    ElementState::Pressed => InputEvent::KeyDown { key },
                    ElementState::Released => InputEvent::KeyUp { key },
                };
                self.feed(&input);
            }
            _ => {}
        }

        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}

impl App {
    fn to_logical(&self, position: PhysicalPosition<f64>) -> Vec2 {
        Vec2::new(
            position.x as f32 / self.scale_factor as f32,
            position.y as f32 / self.scale_factor as f32,
        )
    }
}

/// Adapts the backend's font metrics to the layout engine, so text is measured
/// with the exact advances it is painted with.
struct BackendTextMeasurer {
    metrics: FontMetrics,
}

impl TextMeasurer for BackendTextMeasurer {
    fn advance(&self, ch: char, font_size: f32) -> f32 {
        self.metrics.advance(ch, font_size)
    }

    fn advance_weighted(&self, ch: char, font_size: f32, weight: FontWeight) -> f32 {
        self.metrics.advance_weighted(ch, font_size, weight)
    }

    fn line_height(&self, font_size: f32) -> f32 {
        self.metrics.line_height(font_size)
    }

    fn ascent(&self, font_size: f32) -> f32 {
        self.metrics.ascent(font_size)
    }

    fn measure_run(&self, text: &str, font_size: f32) -> f32 {
        self.metrics.measure_run(text, font_size)
    }

    fn measure_run_weighted(&self, text: &str, font_size: f32, weight: FontWeight) -> f32 {
        self.metrics.measure_run_weighted(text, font_size, weight)
    }
}

/// The byte index before `caret`, the previous character boundary.
fn prev_boundary(text: &str, caret: usize) -> usize {
    let caret = caret.min(text.len());
    text[..caret]
        .char_indices()
        .last()
        .map(|(index, _)| index)
        .unwrap_or(0)
}

/// The byte index after `caret`, the next character boundary.
fn next_boundary(text: &str, caret: usize) -> usize {
    let caret = caret.min(text.len());
    text[caret..]
        .chars()
        .next()
        .map(|ch| caret + ch.len_utf8())
        .unwrap_or(caret)
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

/// Platform wheel -> logical pixels (`y > 0` scrolls down).
fn wheel_pixels(delta: MouseScrollDelta, scale: f32) -> f32 {
    match delta {
        MouseScrollDelta::LineDelta(_, lines) => -lines * WHEEL_LINE_HEIGHT,
        MouseScrollDelta::PixelDelta(position) => {
            -(position.y as f32) / if scale > 0.0 { scale } else { 1.0 }
        }
    }
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

fn pointer_button(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Right => PointerButton::Right,
        MouseButton::Middle => PointerButton::Middle,
        _ => PointerButton::Left,
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

/// The status line after adding games: how many were copied, how many folders
/// joined the scan, and the first reason any were skipped.
fn import_status(report: &ImportReport, dirs_added: usize) -> String {
    let copied = report.copied_count();
    let skipped = report.skipped_count();
    let mut parts = Vec::new();
    if copied > 0 {
        parts.push(format!("已添加 {copied} 个游戏到游戏库"));
    }
    if dirs_added > 0 {
        parts.push(format!("已添加 {dirs_added} 个游戏目录"));
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

/// The save-state hotkey for a key, matching the old front end's layout: `F5`
/// quick-saves, `F6` quick-loads, `F1`–`F3` save slots 1–3, and
/// `Shift`+`F1`–`F3` loads them.
fn state_shortcut(key: &WinitKey, shift: bool) -> Option<Action> {
    match key {
        WinitKey::Named(NamedKey::F11) => Some(Action::ToggleFullscreen),
        WinitKey::Named(NamedKey::F5) => Some(Action::SaveState(0)),
        WinitKey::Named(NamedKey::F6) => Some(Action::LoadState(0)),
        WinitKey::Named(NamedKey::F1) if shift => Some(Action::LoadState(1)),
        WinitKey::Named(NamedKey::F2) if shift => Some(Action::LoadState(2)),
        WinitKey::Named(NamedKey::F3) if shift => Some(Action::LoadState(3)),
        WinitKey::Named(NamedKey::F1) => Some(Action::SaveState(1)),
        WinitKey::Named(NamedKey::F2) => Some(Action::SaveState(2)),
        WinitKey::Named(NamedKey::F3) => Some(Action::SaveState(3)),
        _ => None,
    }
}

fn map_key(key: &WinitKey) -> Option<Key> {
    match key {
        WinitKey::Named(NamedKey::Enter) => Some(Key::Enter),
        WinitKey::Named(NamedKey::Escape) => Some(Key::Escape),
        WinitKey::Named(NamedKey::Backspace) => Some(Key::Backspace),
        WinitKey::Named(NamedKey::Delete) => Some(Key::Delete),
        WinitKey::Named(NamedKey::Tab) => Some(Key::Tab),
        WinitKey::Named(NamedKey::Space) => Some(Key::Space),
        WinitKey::Named(NamedKey::Home) => Some(Key::Home),
        WinitKey::Named(NamedKey::End) => Some(Key::End),
        WinitKey::Named(NamedKey::ArrowUp) => Some(Key::ArrowUp),
        WinitKey::Named(NamedKey::ArrowDown) => Some(Key::ArrowDown),
        WinitKey::Named(NamedKey::ArrowLeft) => Some(Key::ArrowLeft),
        WinitKey::Named(NamedKey::ArrowRight) => Some(Key::ArrowRight),
        WinitKey::Named(NamedKey::F1) => Some(Key::F1),
        WinitKey::Named(NamedKey::F2) => Some(Key::F2),
        WinitKey::Named(NamedKey::F3) => Some(Key::F3),
        WinitKey::Named(NamedKey::F4) => Some(Key::F4),
        WinitKey::Named(NamedKey::F5) => Some(Key::F5),
        WinitKey::Named(NamedKey::F6) => Some(Key::F6),
        WinitKey::Named(NamedKey::F7) => Some(Key::F7),
        WinitKey::Named(NamedKey::F8) => Some(Key::F8),
        WinitKey::Named(NamedKey::F9) => Some(Key::F9),
        WinitKey::Named(NamedKey::F10) => Some(Key::F10),
        WinitKey::Named(NamedKey::F11) => Some(Key::F11),
        WinitKey::Named(NamedKey::F12) => Some(Key::F12),
        WinitKey::Character(text) => text.chars().next().map(Key::Character),
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
    fn save_state_hotkeys_match_the_old_layout() {
        assert_eq!(
            state_shortcut(&WinitKey::Named(NamedKey::F5), false),
            Some(Action::SaveState(0))
        );
        assert_eq!(
            state_shortcut(&WinitKey::Named(NamedKey::F6), false),
            Some(Action::LoadState(0))
        );
        assert_eq!(
            state_shortcut(&WinitKey::Named(NamedKey::F1), false),
            Some(Action::SaveState(1))
        );
        assert_eq!(
            state_shortcut(&WinitKey::Named(NamedKey::F1), true),
            Some(Action::LoadState(1))
        );
        assert_eq!(state_shortcut(&WinitKey::Named(NamedKey::F4), false), None);
        assert_eq!(
            state_shortcut(&WinitKey::Named(NamedKey::F11), false),
            Some(Action::ToggleFullscreen)
        );
    }

    #[test]
    fn import_status_reports_copies_and_skips() {
        let mut report = ImportReport {
            copied: vec![PathBuf::from("/lib/mario.nes")],
            ..ImportReport::default()
        };
        assert!(import_status(&report, 0).contains("已添加 1 个游戏"));

        report.unknown.push(PathBuf::from("/tmp/notes.txt"));
        let message = import_status(&report, 0);
        assert!(message.contains("已添加 1 个游戏"), "{message}");
        assert!(message.contains("跳过 1 个"), "{message}");

        let already = ImportReport {
            already_inside: vec![PathBuf::from("/lib/mario.nes")],
            ..ImportReport::default()
        };
        assert!(import_status(&already, 0).contains("已经在游戏库里"));

        let unknown = ImportReport {
            unknown: vec![PathBuf::from("/tmp/notes.txt")],
            ..ImportReport::default()
        };
        assert!(import_status(&unknown, 0).contains("跳过不认识的 ROM"));
        assert!(import_status(&ImportReport::default(), 2).contains("已添加 2 个游戏目录"));
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

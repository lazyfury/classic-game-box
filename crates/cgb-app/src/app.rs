//! The `winit` + `wgpu` host: the frame loop and the wiring between the UI,
//! the libretro core, the audio device and the gamepad.
//!
//! quill is event-driven by default; an emulator is not. While a game is
//! running the loop schedules a redraw at the core's frame rate
//! (`ControlFlow::WaitUntil`); with no game, or when paused, it falls back to
//! `Wait` and does no work. See `docs/architecture/quill-native-migration.md`
//! §8.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use draw_backend_wgpu::{wgpu, FontConfig, FontMetrics, FontMode, WgpuBackend};
use draw_core::{FontWeight, InputEvent, Key, PointerButton, Size, Vec2, ViewportSize};
use draw_render::{PaintContext, RenderBackend};
use draw_theme::{default_theme, Mode, Theme};
use draw_ui::TextMeasurer;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

use cgb_input::{Gamepads, InputState, KeyboardBindings};
use cgb_library::{load_cores, scan_dir, seed_dir, Library, Paths, Settings};
use cgb_systems::{choose_core, system_for_path, CoreSpec};
use cgb_ui::{Action, Actions, GameRow, Section, Ui, ViewModel};

use crate::cli::{Args, CoreOverride};
use crate::session::Session;

/// One wheel notch scrolls about three text lines.
const WHEEL_LINE_HEIGHT: f32 = 48.0;

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

    paths: Paths,
    settings: Settings,
    library: Option<Library>,
    input: InputState,
    bindings: KeyboardBindings,
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

    last_frame: Instant,
}

impl App {
    fn new(args: Args) -> Self {
        let rescan = args.rescan;
        let paths = Paths::platform();
        let _ = paths.ensure();
        // Arcade cores need a BIOS. Seed the writable system dir the core
        // actually reads from the bundled assets; a player-supplied file wins.
        let _ = seed_dir(Path::new(BUNDLED_ARCADE_SYSTEM), &paths.system);
        let settings = Settings::load(&paths.settings_json);
        let library = Library::open(&paths.library_db).ok();
        let cores = load_core_manifest(&paths);

        let actions = Actions::default();
        let theme = default_theme(Mode::Dark);
        let model = ViewModel {
            core_name: "—".to_string(),
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
            paths,
            settings,
            library,
            input: InputState::new(),
            bindings: KeyboardBindings::default_bindings(),
            gamepads: Gamepads::new().ok(),
            session: None,
            pending_rom: args.rom,
            core_override: args.core,
            cores,
            modifiers: ModifiersState::empty(),
            last_frame: Instant::now(),
        };
        app.refresh_library(rescan);
        app
    }

    /// Create the window, surface, backend and swap chain on first resume.
    fn init(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Classic Game Box")
            .with_inner_size(LogicalSize::new(1100.0, 760.0));
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

        // The real font metrics can only be installed once the backend exists.
        self.dirty = true;

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
    /// With `rescan`, also reconcile the database against disk: rows whose file
    /// is gone are dropped, so moving or deleting ROMs outside the app stays in
    /// sync. Without it the scan only upserts, which is enough for a normal
    /// start.
    fn refresh_library(&mut self, rescan: bool) {
        // Configured folders plus the built-in ROM folder.
        let mut dirs: Vec<PathBuf> = self
            .settings
            .library_dirs
            .iter()
            .map(PathBuf::from)
            .collect();
        dirs.push(self.paths.roms.clone());

        let mut games = Vec::new();
        for dir in &dirs {
            games.extend(scan_dir(dir));
        }

        if let Some(library) = &self.library {
            if rescan {
                let _ = library.sync(&games);
            } else {
                for game in &games {
                    let _ = library.upsert(game);
                }
            }
        }

        self.model.games = games
            .into_iter()
            .map(|game| GameRow {
                title: game.title,
                system: game.system,
                path: game.path,
            })
            .collect();
        self.dirty = true;
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
        config.width = width;
        config.height = height;
        surface.configure(backend.device(), config);
    }

    /// Route one input event: the UI first, then the emulator bindings.
    fn feed(&mut self, event: &InputEvent) {
        self.ui.route_input(event);
        match event {
            InputEvent::KeyDown { key } => {
                self.bindings.apply(*key, true, &mut self.input, 0);
            }
            InputEvent::KeyUp { key } => {
                self.bindings.apply(*key, false, &mut self.input, 0);
            }
            _ => {}
        }
        self.handle_actions();
    }

    /// Act on whatever the UI recorded this event.
    fn handle_actions(&mut self) {
        for action in self.actions.drain() {
            match action {
                Action::Show(section) => {
                    self.model.section = section;
                    self.dirty = true;
                }
                Action::Play(index) => self.start_game(index),
                Action::TogglePause => {
                    if let Some(session) = self.session.as_mut() {
                        session.toggle_pause();
                    }
                    self.dirty = true;
                }
                Action::Reset => {
                    if let Some(session) = self.session.as_ref() {
                        session.reset();
                    }
                }
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
                Action::OpenRom => {
                    self.model.status = "打开 ROM 对话框尚未接入（Q3）".to_string();
                    self.dirty = true;
                }
            }
        }
    }

    fn start_game(&mut self, index: usize) {
        let Some(game) = self.model.games.get(index).cloned() else {
            return;
        };
        self.model.selected = Some(index);
        self.start_path(Path::new(&game.path));
    }

    /// Start a ROM by path: read it, pick a core, build a [`Session`].
    fn start_path(&mut self, rom_path: &Path) {
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
        // flushes its battery save.
        self.session = None;

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
                // A `--rom` may be outside the scanned folders; match it to a
                // library row when possible so the play page shows its title.
                let selected = self
                    .model
                    .games
                    .iter()
                    .position(|game| Path::new(&game.path) == rom_path);
                self.model.selected = selected;
                self.model.core_name = session.core_name().to_string();
                self.model.playing = true;
                self.model.paused = false;
                self.model.status.clear();
                self.model.section = Section::Play;
                self.session = Some(session);
            }
            Err(error) => {
                self.model.status = error;
                self.model.playing = false;
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
        let dev = Path::new("cores/dist").join(module);
        if dev.is_file() {
            return dev;
        }
        packaged
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
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f64().min(0.25);
        self.last_frame = now;

        self.step_gamepad();

        let masks = [self.input.mask(0), self.input.mask(1)];
        if let (Some(session), Some(backend)) = (self.session.as_mut(), self.backend.as_mut()) {
            if !session.paused() {
                session.advance(dt, backend, masks);
            }
        }

        if let Some(session) = self.session.as_ref() {
            self.model.frame = session.frame();
            self.model.paused = session.paused();
            self.model.playing = true;
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

        self.ui.layout(viewport);
        let mut ctx = PaintContext::new();
        self.ui.paint(&mut ctx);
        let list = ctx.into_draw_list();

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
            let _ = backend.submit(&list);
            let _ = backend.end_frame();
        }
        surface_texture.present();
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
                event_loop.exit();
                return;
            }
            WindowEvent::Resized(size) => self.resize(size.width, size.height),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale_factor = scale_factor;
                if let Some(backend) = self.backend.as_mut() {
                    backend.set_scale_factor(scale_factor as f32);
                }
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
                // Save-state hotkeys are app commands, not joypad bindings, and
                // only fire on press (so a held key does not re-save).
                if event.state == ElementState::Pressed {
                    if let Some(action) =
                        state_shortcut(&event.logical_key, self.modifiers.shift_key())
                    {
                        self.actions.push(action);
                        self.handle_actions();
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

/// Platform wheel -> logical pixels (`y > 0` scrolls down).
fn wheel_pixels(delta: MouseScrollDelta, scale: f32) -> f32 {
    match delta {
        MouseScrollDelta::LineDelta(_, lines) => -lines * WHEEL_LINE_HEIGHT,
        MouseScrollDelta::PixelDelta(position) => {
            -(position.y as f32) / if scale > 0.0 { scale } else { 1.0 }
        }
    }
}

/// The core manifest: the packaged `<app data>/cores/cores.json`, else the dev
/// checkout's `cores/cores.json`. Missing is not an error — the app then just
/// has no core to run, and says so when a game is started.
fn load_core_manifest(paths: &Paths) -> Vec<CoreSpec> {
    let packaged = paths.cores.join("cores.json");
    if packaged.is_file() {
        return load_cores(&packaged);
    }
    load_cores(Path::new("cores/cores.json"))
}

fn pointer_button(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Right => PointerButton::Right,
        MouseButton::Middle => PointerButton::Middle,
        _ => PointerButton::Left,
    }
}

/// The save-state hotkey for a key, matching the old front end's layout: `F5`
/// quick-saves, `F6` quick-loads, `F1`–`F3` save slots 1–3, and
/// `Shift`+`F1`–`F3` loads them.
fn state_shortcut(key: &WinitKey, shift: bool) -> Option<Action> {
    match key {
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

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
use winit::keyboard::{Key as WinitKey, NamedKey};
use winit::window::{Window, WindowId};

use cgb_input::{Gamepads, InputState, KeyboardBindings};
use cgb_library::{scan_dir, Library, Paths, Settings};
use cgb_systems::choose_core;
use cgb_ui::{Action, Actions, GameRow, Section, Ui, ViewModel};

use crate::session::Session;

/// One wheel notch scrolls about three text lines.
const WHEEL_LINE_HEIGHT: f32 = 48.0;

/// Runs the app until the window closes. `rom` is the optional `--rom` path.
pub fn run(rom: Option<PathBuf>) {
    let event_loop = EventLoop::new().expect("create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::new(rom);
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

    last_frame: Instant,
}

impl App {
    fn new(rom: Option<PathBuf>) -> Self {
        let paths = Paths::platform();
        let _ = paths.ensure();
        let settings = Settings::load(&paths.settings_json);
        let library = Library::open(&paths.library_db).ok();

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
            pending_rom: None,
            last_frame: Instant::now(),
        };
        app.refresh_library();
        if let Some(rom) = rom {
            app.pending_rom = Some(rom);
        }
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
    fn refresh_library(&mut self) {
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
            for game in scan_dir(dir) {
                if let Some(library) = &self.library {
                    let _ = library.upsert(&game);
                }
                games.push(GameRow {
                    title: game.title.clone(),
                    system: game.system,
                    path: game.path.clone(),
                });
            }
        }
        self.model.games = games;
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

        let selection = self.settings.core_selection();
        let choice = choose_core(&rom_path.to_string_lossy(), &selection);
        // Packaged app: the core lives in the app data dir. Dev (`cargo run`
        // from the repo): fall back to the build output in `cores/dist`.
        let packaged = self.paths.core_dylib(choice.dylib);
        let core_path = if packaged.is_file() {
            packaged
        } else {
            let dev = Path::new("cores/dist").join(choice.dylib);
            if dev.is_file() {
                dev
            } else {
                packaged
            }
        };

        if !core_path.is_file() {
            self.model.status = format!(
                "找不到核心 {}：先跑 ./scripts/build-cores.sh",
                core_path.display()
            );
            self.model.core_name = choice.name.to_string();
            self.dirty = true;
            return;
        }

        let backend = match self.backend.as_mut() {
            Some(backend) => backend,
            None => return,
        };
        let started = Session::start(
            choice,
            &core_path,
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
            WindowEvent::KeyboardInput { event, .. } => {
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

fn pointer_button(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Right => PointerButton::Right,
        MouseButton::Middle => PointerButton::Middle,
        _ => PointerButton::Left,
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

//! One running game: the core, its audio device, and its framebuffer texture.
//!
//! The app owns a `Session` for as long as a cartridge is in. Loading a
//! different game drops it and builds a new one, which is what forces a *new
//! machine* when the console changes (a GBA cannot become a Game Boy) — the
//! old front end's rule, kept.

use std::path::Path;

use cgb_audio::AudioOutput;
use cgb_libretro::CoreHost;
use cgb_systems::CoreChoice;
use cgb_ui::FrameHandle;
use draw_backend_wgpu::{TextureFilter, WgpuBackend};
use draw_render::TextureId;

/// The single framebuffer texture slot. One game is live at a time, so one id
/// is enough; `update_texture` reuses the GPU texture across frames.
const GAME_TEXTURE: TextureId = TextureId::new(1);

/// A loaded cartridge plus everything that runs it.
pub struct Session {
    core: CoreHost,
    /// `None` when the output device could not be opened; the game still runs.
    audio: Option<AudioOutput>,
    texture: TextureId,
    frame: Option<FrameHandle>,
    paused: bool,
    core_name: String,
    frame_seconds: f64,
    /// Time owed to the emulator, in seconds, so a 59.7fps core on a 60Hz
    /// display does not drift.
    accumulator: f64,
}

impl Session {
    /// Bring up `choice`'s core and load the cartridge.
    pub fn start(
        choice: &CoreChoice,
        core_path: &Path,
        system_dir: &Path,
        save_dir: &Path,
        rom_path: &Path,
        data: &[u8],
        backend: &mut WgpuBackend,
    ) -> Result<Self, String> {
        let mut core =
            CoreHost::new(core_path, system_dir, save_dir).map_err(|error| error.to_string())?;
        core.load_game(rom_path, data)
            .map_err(|error| error.to_string())?;

        // Geometry is only trustworthy *after* load (mGBA especially).
        let av = core.av_info();
        let (width, height) = if av.width > 0 && av.height > 0 {
            (av.width, av.height)
        } else {
            (256, 240)
        };

        // Register a black frame at the machine's size with nearest sampling, so
        // the first painted frame is the right shape and stays pixel-sharp.
        let black = vec![0u8; width as usize * height as usize * 4];
        backend
            .register_texture_with_filter(
                GAME_TEXTURE,
                width,
                height,
                &black,
                TextureFilter::Nearest,
            )
            .map_err(|error| error.to_string())?;

        let audio = AudioOutput::new(av.sample_rate.max(8000.0) as u32).ok();
        let frame_seconds = if av.fps > 0.0 {
            1.0 / av.fps
        } else {
            choice.frame_seconds
        };

        Ok(Self {
            core,
            audio,
            texture: GAME_TEXTURE,
            frame: Some(FrameHandle {
                texture: GAME_TEXTURE,
                width,
                height,
            }),
            paused: false,
            core_name: choice.name.to_string(),
            frame_seconds,
            accumulator: 0.0,
        })
    }

    /// Advance the emulator by `dt` seconds, running whole frames to catch up.
    pub fn advance(&mut self, dt: f64, backend: &mut WgpuBackend, masks: [u16; 2]) {
        self.accumulator += dt.clamp(0.0, 0.25);
        let mut steps = 0;
        while self.accumulator >= self.frame_seconds && steps < 4 {
            self.step(backend, masks);
            self.accumulator -= self.frame_seconds;
            steps += 1;
        }
        // If we fell far behind, drop the debt rather than spiral.
        if self.accumulator > self.frame_seconds * 4.0 {
            self.accumulator = 0.0;
        }
    }

    /// Run exactly one frame and publish its video/audio.
    fn step(&mut self, backend: &mut WgpuBackend, masks: [u16; 2]) {
        self.core.set_buttons(0, masks[0]);
        self.core.set_buttons(1, masks[1]);
        self.core.run_frame();

        if let Some(frame) = self.core.take_frame() {
            let _ = backend.update_texture(self.texture, frame.width, frame.height, &frame.rgba);
            self.frame = Some(FrameHandle {
                texture: self.texture,
                width: frame.width,
                height: frame.height,
            });
        }

        let audio = self.core.take_audio();
        if let Some(output) = &self.audio {
            output.push_interleaved(&audio);
        }
    }

    pub fn reset(&self) {
        self.core.reset();
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn frame(&self) -> Option<FrameHandle> {
        self.frame
    }

    pub fn core_name(&self) -> &str {
        &self.core_name
    }

    /// One emulated frame, in seconds.
    pub fn frame_seconds(&self) -> f64 {
        self.frame_seconds
    }
}

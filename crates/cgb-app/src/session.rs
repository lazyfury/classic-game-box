//! One running game: the core, its audio device, and its framebuffer texture.
//!
//! The app owns a `Session` for as long as a cartridge is in. Loading a
//! different game drops it and builds a new one, which is what forces a *new
//! machine* when the console changes (a GBA cannot become a Game Boy) — the
//! old front end's rule, kept.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use cgb_audio::AudioOutput;
use cgb_input::InputState;
use cgb_library::{
    battery_save_path, encode_png, exists, list_slots, read, remove, save_state_path,
    save_state_thumb_path, write, StateSlot,
};
use cgb_libretro::CoreHost;
use cgb_systems::{CoreSpec, SystemId, RETRO_DEVICE_JOYPAD};
use cgb_ui::FrameHandle;
use igui::igui_backend_wgpu::{TextureEffect, TextureFilter, WgpuBackend};
use igui::igui_render::TextureId;

/// The single framebuffer texture slot. One game is live at a time, so one id
/// is enough; `update_texture` reuses the GPU texture across frames.
const GAME_TEXTURE: TextureId = TextureId::new(1);

/// Rewind: snapshot every `REWIND_STRIDE` frames, keeping `REWIND_SECONDS`
/// worth. Serializing is not free, so the stride trades smoothness for cost.
const REWIND_STRIDE: u32 = 2;
const REWIND_SECONDS: f64 = 10.0;

/// How many snapshots fit in [`REWIND_SECONDS`], with a hard cap so a very low
/// frame rate cannot ask for unbounded memory.
fn rewind_capacity(frame_seconds: f64) -> usize {
    if frame_seconds <= 0.0 {
        return 0;
    }
    let frames = REWIND_SECONDS / frame_seconds;
    ((frames / REWIND_STRIDE as f64).ceil() as usize).clamp(1, 4096)
}

/// Whether rewind keeps snapshots for this console.
///
/// Rewind holds [`REWIND_SECONDS`] of serialized states in RAM. That is cheap
/// for the 8/16-bit cores but not for the N64, whose state is tens of
/// megabytes: a held rewind would balloon memory. Disable it there; the UI just
/// has nothing to rewind.
fn rewind_allowed(system: SystemId) -> bool {
    !matches!(system, SystemId::N64)
}

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
    /// Where battery saves and save states for this cartridge live.
    save_dir: PathBuf,
    /// The cartridge path, used to name its `.srm` / `.stateN` files.
    rom_path: PathBuf,
    /// The core's manifest key; save states are namespaced by it because they
    /// are not portable between cores.
    core_key: String,
    /// Wall-clock seconds played while unpaused; drained to the library.
    played_seconds: f64,
    /// The most recent frame's RGBA8 pixels, kept for a screenshot. Moved here
    /// from the frame callback (not copied), so it costs nothing per frame.
    last_pixels: Option<(u32, u32, Vec<u8>)>,
    /// Rewind: recent serialized states, oldest first. One is taken every
    /// `rewind_stride` frames, up to `rewind_capacity` of them.
    rewind: VecDeque<Vec<u8>>,
    rewind_stride: u32,
    rewind_capacity: usize,
    frame_index: u32,
}

impl Session {
    /// Bring up `spec`'s core and load the cartridge.
    pub fn start(
        spec: &CoreSpec,
        system_dir: &Path,
        save_dir: &Path,
        rom_path: &Path,
        data: &[u8],
        backend: &mut WgpuBackend,
    ) -> Result<Self, String> {
        let mut core =
            CoreHost::new(&spec.module, system_dir, save_dir).map_err(|error| error.to_string())?;
        core.load_game(rom_path, data)
            .map_err(|error| error.to_string())?;
        // Both ports are joypads; some cores expect this before input.
        core.set_controller_port_device(0, RETRO_DEVICE_JOYPAD);
        core.set_controller_port_device(1, RETRO_DEVICE_JOYPAD);

        // A battery save (`.srm`) must be written back into the freshly loaded
        // machine *before* the first frame, or the game boots without its save.
        let battery = battery_save_path(save_dir, rom_path);
        if let Some(bytes) = read(&battery) {
            core.write_save_ram(&bytes);
        }

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
            spec.frame_seconds
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
            core_name: spec.name.clone(),
            frame_seconds,
            accumulator: 0.0,
            save_dir: save_dir.to_path_buf(),
            rom_path: rom_path.to_path_buf(),
            core_key: spec.key.clone(),
            played_seconds: 0.0,
            last_pixels: None,
            rewind: VecDeque::new(),
            rewind_stride: REWIND_STRIDE,
            rewind_capacity: if rewind_allowed(spec.system) {
                rewind_capacity(frame_seconds)
            } else {
                0
            },
            frame_index: 0,
        })
    }

    /// Advance the emulator by `dt` seconds, running whole frames to catch up.
    pub fn advance(&mut self, dt: f64, backend: &mut WgpuBackend, input: &InputState) {
        // Play time is wall clock while unpaused: the app only calls `advance`
        // for a running, unpaused session, so a pause or a minimised window
        // adds nothing.
        self.played_seconds += dt.clamp(0.0, 0.25);
        self.accumulator += dt.clamp(0.0, 0.25);
        let mut steps = 0;
        while self.accumulator >= self.frame_seconds && steps < 4 {
            self.step(backend, input);
            self.accumulator -= self.frame_seconds;
            steps += 1;
        }
        // If we fell far behind, drop the debt rather than spiral.
        if self.accumulator > self.frame_seconds * 4.0 {
            self.accumulator = 0.0;
        }
    }

    /// Run exactly one frame and publish its video/audio.
    fn step(&mut self, backend: &mut WgpuBackend, input: &InputState) {
        // The core queries what it wants: buttons as a bitmask, sticks as raw
        // axes (so an arcade core gets a real stick, not a fake D-pad).
        for port in 0..2 {
            self.core.set_buttons(port, input.mask(port));
            for stick in 0..2 {
                for axis in 0..2 {
                    self.core
                        .set_analog(port, stick, axis, input.analog(port, stick, axis));
                }
            }
        }
        self.core.run_frame();

        if let Some(frame) = self.core.take_frame() {
            let _ = backend.update_texture(self.texture, frame.width, frame.height, &frame.rgba);
            self.frame = Some(FrameHandle {
                texture: self.texture,
                width: frame.width,
                height: frame.height,
            });
            // Keep the pixels for a screenshot; moving the buffer costs nothing.
            self.last_pixels = Some((frame.width, frame.height, frame.rgba));
        }

        let audio = self.core.take_audio();
        if let Some(output) = &self.audio {
            output.push_interleaved(&audio);
        }

        // A rewind snapshot every few frames, so a held rewind key steps back
        // through the last few seconds.
        self.frame_index = self.frame_index.wrapping_add(1);
        if self.rewind_stride > 0 && self.frame_index % self.rewind_stride == 0 {
            self.push_rewind();
        }
    }

    /// Serialize the current machine into the rewind ring, dropping the oldest
    /// snapshot when it is full.
    fn push_rewind(&mut self) {
        if self.rewind_capacity == 0 {
            return;
        }
        let bytes = self.core.serialize();
        if bytes.is_empty() {
            return;
        }
        if self.rewind.len() >= self.rewind_capacity {
            self.rewind.pop_front();
        }
        self.rewind.push_back(bytes);
    }

    /// Step back one snapshot. Returns false when there is nothing to rewind to.
    pub fn rewind_step(&mut self) -> bool {
        match self.rewind.pop_back() {
            Some(bytes) => self.core.unserialize(&bytes),
            None => false,
        }
    }

    /// Whether there is anything to rewind to.
    pub fn can_rewind(&self) -> bool {
        !self.rewind.is_empty()
    }

    pub fn reset(&self) {
        self.core.reset();
    }

    /// Write a save state for `slot` to disk (`0` is the quick slot), with a
    /// thumbnail of the last frame beside it.
    pub fn save_state(&self, slot: u8) -> Result<(), String> {
        let bytes = self.core.serialize();
        if bytes.is_empty() {
            return Err("核心不支持即时存档".to_string());
        }
        let path = save_state_path(&self.save_dir, &self.rom_path, &self.core_key, slot);
        write(&path, &bytes).map_err(|error| error.to_string())?;
        if let Some((width, height, pixels)) = self.last_pixels.as_ref() {
            if let Ok(png) = encode_png(*width, *height, pixels) {
                let thumb =
                    save_state_thumb_path(&self.save_dir, &self.rom_path, &self.core_key, slot);
                let _ = write(&thumb, &png);
            }
        }
        Ok(())
    }

    /// Restore the save state in `slot`, if one exists.
    pub fn load_state(&self, slot: u8) -> Result<(), String> {
        let path = save_state_path(&self.save_dir, &self.rom_path, &self.core_key, slot);
        let Some(bytes) = read(&path) else {
            return Err(format!("槽位 {slot} 还没有存档"));
        };
        if self.core.unserialize(&bytes) {
            Ok(())
        } else {
            Err("核心拒绝了这份存档".to_string())
        }
    }

    /// Copy the core's battery RAM to `<saves>/<rom>.srm`. Called when the
    /// session ends; a no-op for cartridges without battery RAM.
    fn persist_battery(&self) {
        let Some(bytes) = self.core.save_ram() else {
            return;
        };
        if bytes.is_empty() {
            return;
        }
        let path = battery_save_path(&self.save_dir, &self.rom_path);
        // A battery-less cartridge can still report a zeroed region. Don't
        // litter the saves dir with empty `.srm` files — but do overwrite an
        // existing one, so an in-game erase is recorded.
        if bytes.iter().all(|byte| *byte == 0) && !exists(&path) {
            return;
        }
        if let Err(error) = write(&path, &bytes) {
            eprintln!("cgb: 写入电池存档 {} 失败：{error}", path.display());
        }
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        if self.paused {
            // A pause is a natural checkpoint; write the battery save out.
            self.persist_battery();
        }
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn frame(&self) -> Option<FrameHandle> {
        self.frame
    }

    /// The cartridge path this session loaded.
    pub fn rom_path(&self) -> &Path {
        &self.rom_path
    }

    /// The most recent frame's size and RGBA8 pixels, for a screenshot.
    pub fn last_pixels(&self) -> Option<(u32, u32, &[u8])> {
        self.last_pixels
            .as_ref()
            .map(|(width, height, pixels)| (*width, *height, pixels.as_slice()))
    }

    /// Take the whole seconds played since the last call, keeping the
    /// fractional remainder so repeated flushes do not lose time.
    pub fn take_played_seconds(&mut self) -> i64 {
        let whole = self.played_seconds.floor();
        self.played_seconds -= whole;
        if whole > 0.0 {
            whole as i64
        } else {
            0
        }
    }

    pub fn core_name(&self) -> &str {
        &self.core_name
    }

    /// The input descriptors the core declared, for the settings page.
    pub fn input_descriptors(&self) -> Vec<cgb_libretro::InputDescriptor> {
        self.core.input_descriptors()
    }

    /// Whether the core can serialize at all (some cannot; the saves UI is
    /// disabled for them).
    pub fn save_supported(&self) -> bool {
        self.core.serialize_size() > 0
    }

    /// This game's save slots for the running core.
    pub fn save_slots(&self) -> Vec<StateSlot> {
        list_slots(&self.save_dir, &self.rom_path, &self.core_key)
    }

    /// A slot's thumbnail path, for the app to decode and upload.
    pub fn save_thumbnail_path(&self, slot: u8) -> PathBuf {
        save_state_thumb_path(&self.save_dir, &self.rom_path, &self.core_key, slot)
    }

    /// Delete a slot's state and thumbnail, ignoring missing files.
    pub fn delete_save(&self, slot: u8) {
        remove(&save_state_path(
            &self.save_dir,
            &self.rom_path,
            &self.core_key,
            slot,
        ));
        let _ = std::fs::remove_file(save_state_thumb_path(
            &self.save_dir,
            &self.rom_path,
            &self.core_key,
            slot,
        ));
    }

    /// Disable every cheat, then apply the enabled ones.
    ///
    /// The `enabled` flag is passed but Mesen and mGBA ignore it (they add
    /// every code they are given), so a disabled cheat is simply not sent.
    pub fn apply_cheats(&self, cheats: &[cgb_library::Cheat]) {
        self.core.reset_cheats();
        for (index, cheat) in cheats.iter().enumerate() {
            if cheat.enabled {
                self.core.set_cheat(index, true, &cheat.code);
            }
        }
    }

    /// Set the post-process applied to this session's framebuffer.
    pub fn set_effect(&self, backend: &mut WgpuBackend, effect: TextureEffect) {
        backend.set_texture_effect(self.texture, effect);
    }

    /// The core's manifest key (save states are namespaced by it).
    pub fn core_key(&self) -> &str {
        &self.core_key
    }

    /// The core's options, for the settings UI.
    pub fn core_options(&self) -> Vec<cgb_libretro::CoreOption> {
        self.core.core_options()
    }

    /// Set a core option value on the running core.
    pub fn set_core_option(&self, key: &str, value: &str) {
        self.core.set_core_option(key, value);
    }

    /// Take the last message the core pushed, if any.
    pub fn take_message(&self) -> Option<String> {
        self.core.take_message()
    }

    /// How many frames the core has produced since load (wraps at `u32`). The
    /// host takes the delta over wall time for the on-screen FPS.
    pub fn frame_index(&self) -> u32 {
        self.frame_index
    }

    /// The core's nominal frame rate, for the initial readout before the first
    /// measured window.
    pub fn target_fps(&self) -> f32 {
        if self.frame_seconds > 0.0 {
            (1.0 / self.frame_seconds) as f32
        } else {
            60.0
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Leaving a game (or the app) is the last chance to flush SRAM.
        self.persist_battery();
    }
}

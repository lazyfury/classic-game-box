//! Manual, human-run smoke test for the N64 core and its OpenGL path.
//!
//! Ignored by default so `cargo test` stays green without ROMs on disk; N64
//! games are also copyright, so no synthetic ROM can stand in (Mupen needs a
//! real IPL3 boot block). Run:
//!
//! ```text
//! CGB_MANUAL_ROM=mario.z64 \
//!     cargo test -p cgb-libretro --test _manual_n64 -- --nocapture --ignored
//! ```
//!
//! `CGB_MANUAL_ROM` (a file name) defaults to `mario.z64`; `CGB_MANUAL_CORE`
//! defaults to `parallel_n64_libretro.dylib`. Both are read from
//! `CGB_MANUAL_DIR`, defaulting to `$HOME/Downloads`.
//!
//! What this proves: the core's `SET_HW_RENDER` was accepted, the offscreen GL
//! context and FBO were created, the core rendered into it, and `take_frame`
//! read it back as RGBA — i.e. the whole GL path, end to end.
//!
//! Diagnostics (all optional):
//!   * `CGB_MANUAL_FRAMES=<n>` — frames to run (default 600).
//!   * `CGB_MANUAL_GFX=gliden64|angrylion|...` — override the GFX plugin.
//!   * `CGB_MANUAL_RSP=auto|hle|cxd4` — override the RSP plugin.
//!   * `CGB_MANUAL_PPM=<path>` — write the last frame as a binary PPM.

use std::path::PathBuf;

use cgb_libretro::CoreHost;

fn manual_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CGB_MANUAL_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").expect("HOME is set");
    PathBuf::from(home).join("Downloads")
}

/// Set a core option before `load_game` reads it (the core registers options
/// during `retro_init`), logging what was requested.
fn override_option(host: &CoreHost, env: &str, key: &str) {
    if let Ok(value) = std::env::var(env) {
        host.set_core_option(key, &value);
        eprintln!("STEP {key}={value}");
    }
}

#[test]
#[ignore = "needs a real N64 ROM; run with --ignored and CGB_MANUAL_ROM"]
fn an_n64_rom_renders_through_opengl() {
    let dir = manual_dir();
    let dist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cores/dist");
    let core_name = std::env::var("CGB_MANUAL_CORE")
        .unwrap_or_else(|_| "parallel_n64_libretro.dylib".to_string());
    let core = dist.join(&core_name);
    let rom = std::env::var("CGB_MANUAL_ROM")
        .map(|name| dir.join(name))
        .unwrap_or_else(|_| dir.join("mario.z64"));
    eprintln!("STEP core {}", core.display());
    eprintln!("STEP rom {}", rom.display());

    let data = std::fs::read(&rom).expect("read rom");
    let mut host = CoreHost::new(&core, &dir, &dir).expect("open core");
    let info = host.system_info();
    eprintln!(
        "STEP core is {} ({:?})",
        info.library_name, info.valid_extensions
    );
    override_option(&host, "CGB_MANUAL_GFX", "parallel-n64-gfxplugin");
    override_option(&host, "CGB_MANUAL_RSP", "parallel-n64-rspplugin");

    eprintln!("STEP load_game (this is where SET_HW_RENDER fires)");
    host.load_game(&rom, &data).expect("load game");
    eprintln!("STEP load_game OK — GL context was accepted");

    let av = host.av_info();
    eprintln!(
        "STEP av {}x{} {:.2}fps {:.0}Hz",
        av.width, av.height, av.fps, av.sample_rate
    );

    // The interpreter is slow-ish; run until the picture appears (or a cap),
    // since the N64 boots for a second or two before it draws anything.
    let max_frames: u32 = std::env::var("CGB_MANUAL_FRAMES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600);
    let mut first_drawn = None;
    let mut last = None;
    let mut audio_samples = 0usize;
    for i in 0..max_frames {
        let start = std::time::Instant::now();
        host.run_frame();
        audio_samples += host.take_audio().len();
        if let Some(frame) = host.take_frame() {
            let non_black = frame
                .rgba
                .chunks_exact(4)
                .filter(|px| px[0] | px[1] | px[2] != 0)
                .count();
            if non_black > 0 && first_drawn.is_none() {
                first_drawn = Some(i);
                eprintln!("STEP first drawn frame: {i}, non-black {non_black}");
            }
            if i % 60 == 0 {
                eprintln!(
                    "STEP frame {i}: {}x{} in {:?}, non-black {non_black}",
                    frame.width,
                    frame.height,
                    start.elapsed()
                );
            }
            last = Some(frame);
        } else {
            eprintln!("STEP frame {i}: no frame in {:?}", start.elapsed());
        }
    }

    let frame = last.expect("at least one frame");
    eprintln!("STEP audio samples over run: {audio_samples}");
    if let Some(path) = std::env::var_os("CGB_MANUAL_PPM") {
        let mut out = format!("P6\n{} {}\n255\n", frame.width, frame.height).into_bytes();
        for px in frame.rgba.chunks_exact(4) {
            out.extend_from_slice(&px[0..3]);
        }
        std::fs::write(&path, out).expect("write ppm");
        eprintln!("STEP wrote {}", path.to_string_lossy());
    }

    let non_black = frame
        .rgba
        .chunks_exact(4)
        .filter(|px| px[0] | px[1] | px[2] != 0)
        .count();
    match first_drawn {
        Some(i) => eprintln!(
            "RESULT OK first drawn at frame {i}, {}x{}, non-black {non_black}",
            frame.width, frame.height
        ),
        None => eprintln!(
            "RESULT all-black {}x{} over {max_frames} frames",
            frame.width, frame.height
        ),
    }
}

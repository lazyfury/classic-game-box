//! Manual, human-run smoke test for an arcade core against a real romset.
//!
//! Ignored by default so `cargo test` stays green without ROMs on disk. Run:
//!
//! ```text
//! CGB_MANUAL_CORE=fbneo_libretro.dylib CGB_MANUAL_ROM=mslug.zip \
//!     cargo test -p cgb-libretro --test _manual_arcade -- --nocapture --ignored
//! ```
//!
//! `CGB_MANUAL_CORE` (a dylib name in `cores/dist`) and `CGB_MANUAL_ROM` (a
//! `.zip` name) are both optional; the defaults are the FBNeo core and
//! `mslug.zip`. The romset and its BIOS (`neogeo.zip`) are read from
//! `CGB_MANUAL_DIR`, defaulting to `$HOME/Downloads`.

use std::path::PathBuf;

use cgb_libretro::CoreHost;

fn manual_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CGB_MANUAL_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").expect("HOME is set");
    PathBuf::from(home).join("Downloads")
}

#[test]
#[ignore = "needs a real romset; run with --ignored and CGB_MANUAL_ROM"]
fn a_real_romset_runs() {
    let dir = manual_dir();
    let dist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cores/dist");
    let core_name = std::env::var("CGB_MANUAL_CORE")
        .unwrap_or_else(|_| "fbneo_libretro.dylib".to_string());
    let core = dist.join(&core_name);
    let rom = std::env::var("CGB_MANUAL_ROM")
        .map(|name| dir.join(name))
        .unwrap_or_else(|_| dir.join("mslug.zip"));
    eprintln!("STEP core {}", core.display());
    eprintln!("STEP rom {}", rom.display());

    let data = std::fs::read(&rom).expect("read rom");
    eprintln!("STEP open");
    let mut host = CoreHost::new(&core, &dir, &dir).expect("open core");
    let info = host.system_info();
    eprintln!(
        "STEP core is {} ({:?})",
        info.library_name, info.valid_extensions
    );
    eprintln!("STEP load_game");
    host.load_game(&rom, &data).expect("load game");
    eprintln!("STEP load_game OK");

    let av = host.av_info();
    eprintln!(
        "STEP av {}x{} {:.2}fps {:.0}Hz",
        av.width, av.height, av.fps, av.sample_rate
    );
    for i in 0..180 {
        host.run_frame();
        if i % 60 == 0 {
            eprintln!("STEP frame {i}");
        }
    }
    eprintln!("STEP done frames");
    let f = host.take_frame().expect("frame");
    let nz = f
        .rgba
        .chunks_exact(4)
        .filter(|p| p[0] | p[1] | p[2] != 0)
        .count();
    eprintln!("RESULT OK {}x{} non-black {}", f.width, f.height, nz);
}

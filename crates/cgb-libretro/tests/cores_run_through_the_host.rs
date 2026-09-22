//! Real cores through the real host: the end-to-end smoke test for everything
//! declared in `cores/cores.json` and `cores/custom/cores.json`.
//!
//! Gated on the gitignored `cores/dist/*.dylib` build products: a core that
//! has not been built is skipped, so a fresh checkout is green without the
//! network. Build them with `./scripts/build-cores.sh`.
//!
//! **One `#[test]`, deliberately.** [`CoreHost`] publishes itself in a
//! process-wide single slot, so two cores cannot be live at once; separate
//! tests would run in parallel and stomp each other. The loops keep exactly
//! one host alive at a time.

use std::path::{Path, PathBuf};

use cgb_libretro::{AvInfo, CoreHost};

/// A minimal NROM: 16KB PRG + 8KB CHR, reset vector jumping to a loop.
fn nrom() -> Vec<u8> {
    let mut rom = vec![0u8; 16 + 16 * 1024 + 8 * 1024];
    rom[0..4].copy_from_slice(b"NES\x1a");
    rom[4] = 1;
    rom[5] = 1;
    let prg = &mut rom[16..16 + 16 * 1024];
    prg[0] = 0x4C; // JMP $8000
    prg[1] = 0x00;
    prg[2] = 0x80;
    prg[0x3FFC] = 0x00;
    prg[0x3FFD] = 0x80;
    rom
}

/// A minimal GBA ROM: an ARM `b #-8` loop at the entry point and the two bytes
/// `GBAIsROM` checks (the ARM branch opcode at 3 and the fixed `0x96` at 0xB2).
fn gba_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0..4].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes());
    rom[0xB2] = 0x96;
    rom
}

fn dist_core(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../cores/dist")
        .join(name)
}

/// Load `rom` into `core`, run a few frames and return the host plus the
/// geometry the core reported. Panics if the core cannot do its job.
///
/// The ROM is written to a temp file first and its real path is passed:
/// some cores (Mesen) declare `need_fullpath` and read the file, ignoring the
/// in-memory pointer. The app always passes the real ROM path for the same
/// reason.
fn run(core: &Path, rom_name: &str, rom: &[u8]) -> (CoreHost, AvInfo) {
    let dir = std::env::temp_dir().join(format!("cgb-core-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let rom_path = dir.join(rom_name);
    std::fs::write(&rom_path, rom).expect("write rom");

    let mut host = CoreHost::new(core, &dir, &dir).expect("open core");
    host.load_game(&rom_path, rom).expect("load game");

    let av = host.av_info();
    assert!(av.fps > 59.0 && av.fps < 61.0, "fps was {}", av.fps);
    assert!(av.sample_rate >= 8000.0);

    for _ in 0..3 {
        host.run_frame();
    }
    let frame = host.take_frame().expect("a frame after run");
    assert_eq!(frame.rgba.len(), av.width as usize * av.height as usize * 4);
    (host, av)
}

/// NES cores and the sample rate each reports. `mesen` is built-in; the other
/// two come from `cores/custom/cores.json`.
const NES_CORES: &[(&str, f64)] = &[
    ("mesen_libretro.dylib", 48_000.0),
    ("nestopia_libretro.dylib", 48_000.0),
    ("custom_nes_core_libretro.dylib", 44_100.0),
];

#[test]
fn cores_run_through_the_host() {
    let mut tested = 0;

    for (name, expected_rate) in NES_CORES {
        let core = dist_core(name);
        if !core.is_file() {
            eprintln!("skip: {} not built", core.display());
            continue;
        }
        let (host, av) = run(&core, "test.nes", &nrom());
        assert_eq!((av.width, av.height), (256, 240), "{name}");
        assert!(
            (av.sample_rate - expected_rate).abs() < 1.0,
            "{name} reported {}Hz, manifest says {expected_rate}",
            av.sample_rate
        );
        eprintln!("{name}: {}x{} @ {:.3}fps", av.width, av.height, av.fps);
        // Drop before the next core: the host slot is a single global.
        drop(host);
        tested += 1;
    }

    // mGBA asks for RGB565; this also exercises the host's format conversion.
    let mgba = dist_core("mgba_libretro.dylib");
    if mgba.is_file() {
        let (host, av) = run(&mgba, "test.gba", &gba_rom());
        assert_eq!((av.width, av.height), (240, 160));
        assert!(
            (av.sample_rate - 65_536.0).abs() < 1.0,
            "{}",
            av.sample_rate
        );
        eprintln!(
            "mgba_libretro.dylib: {}x{} @ {:.3}fps",
            av.width, av.height, av.fps
        );
        drop(host);
        tested += 1;
    } else {
        eprintln!("skip: {} not built", mgba.display());
    }

    if tested == 0 {
        eprintln!("skip: no cores built (run ./scripts/build-cores.sh)");
    }
}

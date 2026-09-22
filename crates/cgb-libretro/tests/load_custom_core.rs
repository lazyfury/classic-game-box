//! Real cores from `cores/custom/cores.json` through the real host: the
//! custom-core flow's end-to-end check (see `cores/custom/README.md`).
//!
//! Gated on the gitignored `cores/dist/*.dylib` build products: a core that
//! has not been built is skipped, so a fresh checkout is green without the
//! network.
//!
//! **One `#[test]`, deliberately.** [`CoreHost`] publishes itself in a
//! process-wide single slot, so two cores cannot be live at once; separate
//! tests would run in parallel and stomp each other. The loop keeps exactly
//! one host alive at a time.

use std::path::{Path, PathBuf};

use cgb_libretro::{AvInfo, CoreHost};

/// A minimal NROM: 16KB PRG + 8KB CHR, reset vector jumping to a loop. Enough
/// for any NES core to load and render without a copyrighted ROM.
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

fn dist_core(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../cores/dist")
        .join(name)
}

/// Load a synthetic NROM into `core`, run a few frames and return the host
/// plus the geometry the core reported. Panics if the core cannot do its job.
fn run_synthetic_nrom(core: &Path) -> (CoreHost, AvInfo) {
    let dir = std::env::temp_dir();
    let mut host = CoreHost::new(core, &dir, &dir).expect("open core");
    host.load_game("test.nes", &nrom()).expect("load game");

    let av = host.av_info();
    assert_eq!((av.width, av.height), (256, 240));
    assert!(av.fps > 59.0 && av.fps < 61.0, "fps was {}", av.fps);
    assert!(av.sample_rate >= 8000.0);

    for _ in 0..3 {
        host.run_frame();
    }
    let frame = host.take_frame().expect("a frame after run");
    assert_eq!(frame.rgba.len(), 256 * 240 * 4);

    (host, av)
}

/// `(module, sample rate)`, matching the manifest hints.
const CASES: &[(&str, f64)] = &[
    ("nestopia_libretro.dylib", 48_000.0),
    ("custom_nes_core_libretro.dylib", 44_100.0),
];

#[test]
fn custom_cores_run_through_the_host() {
    let mut tested = 0;
    for (name, expected_rate) in CASES {
        let core = dist_core(name);
        if !core.is_file() {
            eprintln!("skip: {} not built", core.display());
            continue;
        }

        let (host, av) = run_synthetic_nrom(&core);
        eprintln!(
            "{name}: {}x{} @ {:.3}fps, {:.0}Hz",
            av.width, av.height, av.fps, av.sample_rate
        );
        assert!(
            (av.sample_rate - expected_rate).abs() < 1.0,
            "{name} reported {}Hz, manifest says {expected_rate}",
            av.sample_rate
        );
        // Drop the host before building the next one: the host slot is a
        // single global, so only one core may be live at a time.
        drop(host);
        tested += 1;
    }

    if tested == 0 {
        eprintln!("skip: no custom cores built (run ./scripts/build-cores.sh)");
    }
}

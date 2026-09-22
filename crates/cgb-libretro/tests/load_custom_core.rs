//! A real third-party core through the real host: the custom-core flow's
//! end-to-end check (see `cores/custom/README.md`).
//!
//! Gated on `cores/dist/nestopia_libretro.dylib` existing, which is a
//! gitignored build product — the test skips (does not fail) when the core has
//! not been built, so a fresh checkout is green without the network.

use std::path::Path;

use cgb_libretro::CoreHost;

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

#[test]
fn a_custom_core_runs_through_the_host() {
    let core =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cores/dist/nestopia_libretro.dylib");
    if !core.is_file() {
        eprintln!("skip: {} not built", core.display());
        return;
    }
    let dir = std::env::temp_dir();
    let mut host = CoreHost::new(&core, &dir, &dir).expect("open core");
    host.load_game("test.nes", &nrom()).expect("load game");

    let av = host.av_info();
    assert_eq!((av.width, av.height), (256, 240));
    assert!(av.fps > 59.0 && av.fps < 61.0);

    for _ in 0..3 {
        host.run_frame();
    }
    let frame = host.take_frame().expect("a frame after run");
    assert_eq!(frame.rgba.len(), 256 * 240 * 4);

    // Battery-free NROM: no save RAM is expected.
    assert!(host.save_ram().is_none());
    eprintln!(
        "nestopia: {}x{} @ {:.3}fps, {:.0}Hz",
        av.width, av.height, av.fps, av.sample_rate
    );
}

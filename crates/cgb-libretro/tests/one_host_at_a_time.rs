//! The core host is a process-wide singleton, and switching games must respect
//! that.
//!
//! The callbacks reach one host through a global slot and a core is a single
//! loaded instance, so a second live host would `retro_init` the same dylib
//! again and then `retro_deinit` the new machine when the old host drops — a
//! segfault. The app drops the old session before starting a new one; this
//! test pins the host-side guard that turns a future mistake into an error.
//!
//! Gated on the gitignored `cores/dist` build products: skipped when the core
//! has not been built.

use std::path::{Path, PathBuf};

use cgb_libretro::{CoreHost, LibretroError};

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

fn open(dir: &Path, core: &Path, name: &str, rom: &[u8]) -> CoreHost {
    let path = dir.join(name);
    std::fs::write(&path, rom).expect("write rom");
    let mut host = CoreHost::new(core, dir, dir).expect("open core");
    host.load_game(&path, rom).expect("load game");
    host
}

#[test]
fn a_second_host_is_refused_and_the_next_one_works() {
    let core = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cores/dist/mesen_libretro.dylib");
    if !core.is_file() {
        eprintln!("skip: {} not built", core.display());
        return;
    }
    let dir: PathBuf = std::env::temp_dir().join(format!("cgb-singleton-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let rom = nrom();

    let first = open(&dir, &core, "a.nes", &rom);

    // Starting a second machine while the first is live must fail cleanly
    // instead of corrupting the core.
    match CoreHost::new(&core, &dir, &dir) {
        Err(LibretroError::HostBusy) => {}
        Err(other) => panic!("expected HostBusy, got {other}"),
        Ok(_) => panic!("a second host was allowed while one was live"),
    }

    // Once the first is gone, the next machine starts fine — the app's switch
    // path.
    drop(first);
    let second = open(&dir, &core, "b.nes", &rom);
    for _ in 0..3 {
        second.run_frame();
    }
    assert!(second.take_frame().is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

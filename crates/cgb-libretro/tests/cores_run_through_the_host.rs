//! Real cores through the real host: the end-to-end smoke test for everything
//! declared in `cores/cores.json`.
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

/// NES cores and the sample rate each reports, as declared in
/// `cores/cores.json`.
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

    // FBNeo has no synthetic ROM: prove it opens through the loader and
    // declares the arcade content it wants. Loading a real `.zip` romset is the
    // manual check (`need_fullpath` is false — it reads the archive itself).
    let fbneo = dist_core("fbneo_libretro.dylib");
    if fbneo.is_file() {
        let dir = std::env::temp_dir();
        let host = CoreHost::new(&fbneo, &dir, &dir).expect("open fbneo");
        let info = host.system_info();
        assert!(
            info.library_name.contains("FinalBurn Neo"),
            "{}",
            info.library_name
        );
        assert!(info.valid_extensions.iter().any(|ext| ext == "zip"));
        eprintln!(
            "fbneo_libretro.dylib: {} ({:?})",
            info.library_name, info.valid_extensions
        );
        drop(host);
        tested += 1;
    } else {
        eprintln!("skip: {} not built", fbneo.display());
    }

    // PicoDrive has no synthetic ROM either: prove it opens and declares the
    // Sega content it handles, including the consoles it shares with Genesis
    // Plus GX. It declares `need_fullpath`, which the app already honours.
    let picodrive = dist_core("picodrive_libretro.dylib");
    if picodrive.is_file() {
        let dir = std::env::temp_dir();
        let host = CoreHost::new(&picodrive, &dir, &dir).expect("open picodrive");
        let info = host.system_info();
        assert!(
            info.library_name.contains("PicoDrive"),
            "{}",
            info.library_name
        );
        assert!(info.need_fullpath, "picodrive reads the file itself");
        for ext in ["md", "gen", "smd", "bin", "sms", "gg", "sg"] {
            assert!(
                info.valid_extensions.iter().any(|e| e == ext),
                "picodrive does not declare .{ext}: {:?}",
                info.valid_extensions
            );
        }
        eprintln!(
            "picodrive_libretro.dylib: {} ({:?})",
            info.library_name, info.valid_extensions
        );
        drop(host);
        tested += 1;
    } else {
        eprintln!("skip: {} not built", picodrive.display());
    }

    // ParaLLEl-N64 is hardware-rendered, so it cannot be driven with a
    // synthetic ROM (it needs a real IPL3 boot block and an OpenGL frame).
    // Prove it opens through the loader and declares the N64 content it wants;
    // the GL path itself is the manual check in
    // docs/architecture/n64-gl-hw-render-plan.md.
    let n64 = dist_core("parallel_n64_libretro.dylib");
    if n64.is_file() {
        let dir = std::env::temp_dir();
        let host = CoreHost::new(&n64, &dir, &dir).expect("open parallel_n64");
        let info = host.system_info();
        assert!(
            info.library_name.contains("ParaLLEl"),
            "{}",
            info.library_name
        );
        for ext in ["z64", "n64", "v64"] {
            assert!(
                info.valid_extensions.iter().any(|e| e == ext),
                "parallel_n64 does not declare .{ext}: {:?}",
                info.valid_extensions
            );
        }
        eprintln!(
            "parallel_n64_libretro.dylib: {} ({:?})",
            info.library_name, info.valid_extensions
        );
        drop(host);
        tested += 1;
    } else {
        eprintln!("skip: {} not built", n64.display());
    }

    // FreeJ2ME-Plus runs the game in a child Java VM, so there is no synthetic
    // game to load. Prove the core opens, declares the J2ME content it wants,
    // and — the reason the host speaks core options v2 — parses its options.
    // FreeJ2ME-Plus only gets this right at v2: at v1 it hands a v2 array to
    // `SET_CORE_OPTIONS`, whose layout differs, so the resolution default reads
    // back as 0 and the JVM starts with no screen. Opening the core spawns the
    // VM, so only do it when the jar and a bundled runtime are both present.
    let j2me = dist_core("freej2me_plus_libretro.dylib");
    let j2me_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cores/dist/freej2me_plus");
    let jar = j2me_dir.join("freej2me_plus-lr.jar");
    let runtime_bin = j2me_dir.join("runtime/bin");
    if j2me.is_file() && jar.is_file() && runtime_bin.join("java").is_file() {
        let mut paths = vec![std::fs::canonicalize(&runtime_bin).expect("runtime bin")];
        if let Some(existing) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&existing));
        }
        std::env::set_var("PATH", std::env::join_paths(paths).expect("PATH"));

        let dir = std::env::temp_dir().join(format!("cgb-j2me-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::copy(&jar, dir.join("freej2me_plus-lr.jar")).expect("seed jar");

        let host = CoreHost::new(&j2me, &dir, &dir).expect("open freej2me_plus");
        let info = host.system_info();
        assert!(
            info.library_name.contains("FreeJ2ME"),
            "{}",
            info.library_name
        );
        for ext in ["jar", "kjx"] {
            assert!(
                info.valid_extensions.iter().any(|e| e == ext),
                "freej2me_plus does not declare .{ext}: {:?}",
                info.valid_extensions
            );
        }
        assert!(info.need_fullpath, "freej2me reads the jar itself");
        let options = host.core_options();
        let resolution = options
            .iter()
            .find(|option| option.key == "freej2me_resolution")
            .expect("v2 core options parsed");
        assert_eq!(resolution.value, "240x320");
        assert!(!host.input_descriptors().is_empty(), "input descriptors");
        eprintln!(
            "freej2me_plus_libretro.dylib: {} ({} options)",
            info.library_name,
            options.len()
        );
        // Drop before the next core: the host slot is a single global. This
        // also kills the child JVM.
        drop(host);
        let _ = std::fs::remove_dir_all(&dir);
        tested += 1;
    } else {
        eprintln!("skip: freej2me_plus not built (./cores/freej2me_plus/build.sh)");
    }

    if tested == 0 {
        eprintln!("skip: no cores built (run ./scripts/build-cores.sh)");
    }
}

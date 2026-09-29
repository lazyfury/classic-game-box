//! Build-time wiring for the native core host.
//!
//! * Links the OpenGL framework so the offscreen CGL context in `src/gl.rs` can
//!   be created. macOS only; every other target uses the stub `gl` module.
//! * Compiles the C log shim. The libretro log callback is variadic
//!   (`void (*)(int level, const char *fmt, ...)`) and Rust cannot define
//!   C-variadic functions on stable, so the printf formatting lives in
//!   `src/log_shim.c` and calls back into `cgb_log_emit`.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-lib=framework=OpenGL");
    }

    cc::Build::new()
        .file("src/log_shim.c")
        .warnings(true)
        .compile("cgb_log_shim");
    println!("cargo:rerun-if-changed=src/log_shim.c");
}

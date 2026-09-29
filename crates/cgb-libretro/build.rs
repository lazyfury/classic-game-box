// Link the OpenGL framework so the offscreen CGL context in `src/gl.rs` can be
// created. macOS only; every other target uses the stub `gl` module.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-lib=framework=OpenGL");
    }
}

//! Classic Game Box — native Rust front end.
//!
//! ```text
//! cargo run -p cgb-app                 # open the library UI
//! cargo run -p cgb-app -- --rom mario.nes
//! ```
//!
//! The window host lives in [`app`]; one running game is a [`session`].

mod app;
mod session;

fn main() {
    let rom = std::env::args()
        .skip(1)
        .find(|arg| !arg.starts_with('-'))
        .map(std::path::PathBuf::from);
    app::run(rom);
}

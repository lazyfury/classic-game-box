//! Classic Game Box — native Rust front end.
//!
//! ```text
//! cargo run -p cgb-app                          # open the library UI
//! cargo run -p cgb-app -- mario.nes             # load a ROM at startup
//! cargo run -p cgb-app -- mario.nes --core mesen
//! cargo run -p cgb-app -- --core ./custom_libretro.dylib
//! ```
//!
//! The window host lives in [`app`]; one running game is a [`session`].

mod app;
mod cli;
mod session;

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("{}", cli::USAGE);
        return;
    }
    match cli::Args::parse(raw) {
        Ok(args) => app::run(args),
        Err(error) => {
            eprintln!("{error}\n\n{}", cli::USAGE);
            std::process::exit(2);
        }
    }
}

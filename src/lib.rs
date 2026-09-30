//! Classic Game Box — native Rust front end.
//!
//! This is the app library surface. The binary ([`main.rs`](../main.rs)) is a
//! thin CLI over it; keeping the modules public lets the bench target and the
//! headless self-check drive the same code as the window.
//!
//! ```text
//! cargo run -p cgb-app                          # open the library UI
//! cargo run -p cgb-app -- mario.nes             # load a ROM at startup
//! cargo run -p cgb-app -- mario.nes --core mesen
//! cargo run -p cgb-app -- --core ./custom_libretro.dylib
//! ```
//!
//! The window host lives in [`app`]; one running game is a [`session`].

pub mod app;
pub mod cli;
pub mod cores_cli;
pub mod selfcheck;
pub mod session;
pub mod ui;

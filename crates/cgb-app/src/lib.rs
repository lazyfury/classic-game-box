//! Classic Game Box — native Rust front end.
//!
//! This is the app library surface embedded by the native macOS host.
//!
//! The window host lives in [`app`]; one running game is a [`session`].

pub mod app;
pub mod cli;
pub mod cores_cli;
pub mod selfcheck;
pub mod session;
pub mod ui;

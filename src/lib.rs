//! Classic Game Box — the app library.
//!
//! This is the app surface the native macOS host embeds. The emulator
//! boundary ([`cgb_libretro`]) stays a separate crate; everything
//! app-shaped lives here as flat modules or one-concern directories:
//!
//! | module | what |
//! |---|---|
//! | [`ui`] | the igui views, built from a pure [`ViewModel`] |
//! | [`app`] | app state, frame loop, feature wiring, host traits usage |
//! | [`session`] | one running game (libretro session + audio) |
//! | [`library`] | the SQLite game library, saves, screenshots, cheats |
//! | [`paths`] | file layout ([`Paths`]) and [`Settings`] |
//! | [`cores`] | `cores.json` manifest, buildbot catalog, downloader |
//! | [`audio`] | cpal output + ring buffer |
//! | [`host`] | the `HostWindow` / `GamepadSource` contract |
//! | [`mac`] | the Swift/macOS host FFI (`cgb_mac_*` C ABI) |
//! | [`cli`] / [`cores_cli`] / [`selfcheck`] | headless surfaces |

pub mod app;
pub mod audio;
pub mod cli;
pub mod cores;
pub mod cores_cli;
pub mod host;
pub mod library;
pub mod mac;
pub mod paths;
pub mod selfcheck;
pub mod session;
pub mod ui;

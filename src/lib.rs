//! Classic Game Box — the app library.
//!
//! This is the app surface the native shells embed. The emulator boundary
//! ([`cgb_libretro`]) stays a separate crate; everything app-shaped lives here
//! as flat modules or one-concern directories:
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
//! | [`native`] | the embedded host + `cgb_host_*` C ABI (Swift/macOS + C++/Win32) |
//! | [`cli`] / [`cores_cli`] / [`selfcheck`] | headless surfaces |

pub mod app;
pub mod audio;
pub mod cli;
pub mod cores;
pub mod cores_cli;
pub mod host;
pub mod library;
// The unified embedded host. It compiles on every target: the macOS surface
// branch is `#[cfg(target_os = "macos")]`, and the Windows one uses
// `raw-window-handle` (cross-platform), so the macOS dev gate type-checks both.
pub mod native;
pub mod paths;
pub mod selfcheck;
pub mod session;
pub mod ui;

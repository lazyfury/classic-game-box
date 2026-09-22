//! Pure view data for the UI. No quill, no platform — just what the screen
//! needs to know, so the view builder can be tested headlessly.

use cgb_systems::SystemId;
use draw_render::TextureId;

/// Which page the middle column is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Library,
    Play,
    Settings,
}

impl Section {
    /// The pages, in the order the rail lists them.
    pub const ALL: [Section; 3] = [Section::Library, Section::Play, Section::Settings];

    /// What the rail says.
    pub fn label(self) -> &'static str {
        match self {
            Section::Library => "游戏库",
            Section::Play => "游玩",
            Section::Settings => "设置",
        }
    }
}

/// One row in the library list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameRow {
    pub title: String,
    pub system: SystemId,
    pub path: String,
}

/// A core the settings page can pick for a console.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreRow {
    pub key: String,
    pub name: String,
    pub system: SystemId,
    /// Whether this is the core that would run the console now.
    pub selected: bool,
}

/// One joypad button and the keys bound to it, for the settings page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingRow {
    pub button: String,
    pub keys: String,
}

/// A registered framebuffer texture and its size.
///
/// The texture is registered in the wgpu backend by the app; the view only
/// carries the handle. Drawing it is the one thing the current UI stack cannot
/// do yet — see `frame.rs`.
#[derive(Clone, Copy, Debug)]
pub struct FrameHandle {
    pub texture: TextureId,
    pub width: u32,
    pub height: u32,
}

/// Everything the UI renders from, rebuilt from app state each frame.
#[derive(Clone, Debug)]
pub struct ViewModel {
    pub section: Section,
    pub games: Vec<GameRow>,
    pub selected: Option<usize>,
    pub playing: bool,
    pub paused: bool,
    pub core_name: String,
    pub frame: Option<FrameHandle>,
    pub status: String,
    /// Every core in the manifest, for the settings picker.
    pub cores: Vec<CoreRow>,
    /// Folders the library scans.
    pub library_dirs: Vec<String>,
    /// Keyboard bindings, for the settings page.
    pub bindings: Vec<BindingRow>,
}

impl Default for ViewModel {
    fn default() -> Self {
        Self {
            section: Section::Library,
            games: Vec::new(),
            selected: None,
            playing: false,
            paused: false,
            core_name: String::new(),
            frame: None,
            status: String::new(),
            cores: Vec::new(),
            library_dirs: Vec::new(),
            bindings: Vec::new(),
        }
    }
}

/// What the user asked for, drained by the app after routing input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Show a page.
    Show(Section),
    /// Start the game at this library index.
    Play(usize),
    TogglePause,
    Reset,
    /// Write a save state to a slot (`0` is the quick slot).
    SaveState(u8),
    /// Restore a save state from a slot.
    LoadState(u8),
    /// Open the native folder picker and add the chosen folder to the library.
    OpenRom,
    /// Make the core at this `cores` index the pick for its console.
    SelectCore(usize),
    /// Stop scanning the library folder at this `library_dirs` index.
    RemoveLibraryDir(usize),
}

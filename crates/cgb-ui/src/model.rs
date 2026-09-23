//! Pure view data for the UI. No quill, no platform — just what the screen
//! needs to know, so the view builder can be tested headlessly.

use cgb_systems::SystemId;
use draw_render::TextureId;

/// Which page the middle column is showing. The console is the right column
/// and is always there, so it is not a section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Library,
    Screenshots,
    Saves,
    Cheats,
    Settings,
}

impl Section {
    /// The pages, in the order the rail lists them.
    pub const ALL: [Section; 5] = [
        Section::Library,
        Section::Screenshots,
        Section::Saves,
        Section::Cheats,
        Section::Settings,
    ];

    /// What the rail says.
    pub fn label(self) -> &'static str {
        match self {
            Section::Library => "游戏库",
            Section::Screenshots => "截图",
            Section::Saves => "存档",
            Section::Cheats => "金手指",
            Section::Settings => "设置",
        }
    }
}

/// Insets the UI must leave for platform chrome, in logical pixels. On macOS
/// with a full-size content view the content runs under the title bar, so the
/// header has to clear the title bar (`top`) and the traffic lights (`left`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SafeArea {
    pub top: f32,
    pub left: f32,
}

impl SafeArea {
    /// No chrome to avoid (the default on every platform but macOS).
    pub const ZERO: Self = Self {
        top: 0.0,
        left: 0.0,
    };
}

/// What an in-progress text edit is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    /// The game's display name.
    Name,
    /// The game's tags, comma-separated in the field.
    Tags,
    /// The library search query.
    Search,
}

/// An in-progress text edit. The app owns the keyboard while this is set and
/// commits or cancels it; the view draws the field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditState {
    pub game_id: i64,
    pub kind: EditKind,
    pub text: String,
    /// Caret position, a byte index into `text` on a char boundary.
    pub caret: usize,
}

/// A post-process preset for the game picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaderKind {
    Off,
    Scanlines,
    Crt,
    Lcd,
    Sharpen,
}

impl ShaderKind {
    /// Every preset, in the order the settings page lists them.
    pub const ALL: [ShaderKind; 5] = [
        ShaderKind::Off,
        ShaderKind::Scanlines,
        ShaderKind::Crt,
        ShaderKind::Lcd,
        ShaderKind::Sharpen,
    ];

    /// What the button says.
    pub fn label(self) -> &'static str {
        match self {
            ShaderKind::Off => "关闭",
            ShaderKind::Scanlines => "扫描线",
            ShaderKind::Crt => "CRT",
            ShaderKind::Lcd => "LCD 网格",
            ShaderKind::Sharpen => "锐化",
        }
    }

    /// The stable key stored in settings.
    pub fn key(self) -> &'static str {
        match self {
            ShaderKind::Off => "off",
            ShaderKind::Scanlines => "scanlines",
            ShaderKind::Crt => "crt",
            ShaderKind::Lcd => "lcd",
            ShaderKind::Sharpen => "sharpen",
        }
    }

    /// Parse a stored key; anything unknown is `Off`.
    pub fn from_key(key: &str) -> Self {
        match key {
            "scanlines" => ShaderKind::Scanlines,
            "crt" => ShaderKind::Crt,
            "lcd" => ShaderKind::Lcd,
            "sharpen" => ShaderKind::Sharpen,
            _ => ShaderKind::Off,
        }
    }
}

/// A destructive action waiting for the player to confirm it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confirm {
    /// Delete the game with this id (file, screenshots and row).
    DeleteGame(i64),
    /// Delete the screenshot with this id.
    DeleteScreenshot(i64),
}

impl Confirm {
    /// The question the confirmation bar asks.
    pub fn message(self) -> &'static str {
        match self {
            Confirm::DeleteGame(_) => "删除这个游戏？ROM 文件和它的截图都会被删除。",
            Confirm::DeleteScreenshot(_) => "删除这张截图？",
        }
    }
}

/// One row in the library list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameRow {
    /// Database id; screenshots reference it.
    pub id: i64,
    /// The display name (editable; from the file stem until renamed).
    pub name: String,
    /// The actual file name (`mario.nes`).
    pub file_name: String,
    pub system: SystemId,
    pub path: String,
    pub size: u64,
    /// Whether the game is pinned to the top of the library.
    pub pinned: bool,
    /// How many times the game has been run.
    pub play_count: i64,
    /// Total time played, in seconds.
    pub play_seconds: i64,
    /// When it was last run, in epoch milliseconds (0 = never).
    pub last_played_at: i64,
    /// The player's labels, alphabetical and without duplicates.
    pub tags: Vec<String>,
    /// How many screenshots have been taken of it.
    pub screenshots: i64,
    /// The game's cover texture, when it has a screenshot set as cover. The
    /// app registers it; the view draws it behind the card controls.
    pub cover: Option<FrameHandle>,
}

/// One cheat in the cheats section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheatRow {
    pub desc: String,
    pub code: String,
    pub enabled: bool,
}

/// One save-state slot in the saves section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveSlotRow {
    pub slot: u8,
    pub exists: bool,
    /// When it was written, epoch milliseconds (0 = never).
    pub modified_ms: i64,
    /// The registered thumbnail texture, when the slot has one.
    pub thumb: Option<FrameHandle>,
}

/// One screenshot in the screenshots section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenshotRow {
    pub id: i64,
    pub game_id: i64,
    /// The game's display name, for the caption.
    pub game: String,
    pub created_at: i64,
    /// Whether this picture is its game's cover.
    pub is_cover: bool,
    /// The registered thumbnail texture, when it has been uploaded.
    pub thumb: Option<FrameHandle>,
}

/// How the library is ordered. Pinned games always come first, whatever the
/// key; this only decides the order within the pinned and unpinned groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Size,
    LastPlayed,
    Playtime,
    Added,
}

impl SortKey {
    /// The sort modes, in the order the library bar lists them.
    pub const ALL: [SortKey; 5] = [
        SortKey::Name,
        SortKey::Size,
        SortKey::LastPlayed,
        SortKey::Playtime,
        SortKey::Added,
    ];

    /// What the sort button says.
    pub fn label(self) -> &'static str {
        match self {
            SortKey::Name => "名称",
            SortKey::Size => "大小",
            SortKey::LastPlayed => "最近",
            SortKey::Playtime => "时长",
            SortKey::Added => "加入",
        }
    }

    /// The stable key stored in settings.
    pub fn key(self) -> &'static str {
        match self {
            SortKey::Name => "name",
            SortKey::Size => "size",
            SortKey::LastPlayed => "last_played",
            SortKey::Playtime => "playtime",
            SortKey::Added => "added",
        }
    }

    /// Parse a stored key; anything unknown (including empty) is the name
    /// order, so a setting written by an older build still loads.
    pub fn from_key(key: &str) -> Self {
        match key {
            "size" => SortKey::Size,
            "last_played" => SortKey::LastPlayed,
            "playtime" => SortKey::Playtime,
            "added" => SortKey::Added,
            _ => SortKey::Name,
        }
    }

    /// The direction this key starts in when it becomes active: names read
    /// A–Z, everything else starts with the largest or most recent first.
    pub fn default_desc(self) -> bool {
        !matches!(self, SortKey::Name)
    }
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

/// One input descriptor a core declared (`SET_INPUT_DESCRIPTORS`), shown in the
/// settings page so a core's own buttons (mGBA's shoulders, an arcade stick)
/// are visible.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputDescriptorRow {
    pub port: u32,
    pub device: u32,
    pub index: u32,
    pub id: u32,
    pub description: String,
}

/// One core option, for the settings page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreOptionRow {
    pub key: String,
    pub label: String,
    /// The `(value, label)` choices.
    pub values: Vec<(String, String)>,
    pub value: String,
}

/// A registered framebuffer texture and its size.
///
/// The texture is registered in the wgpu backend by the app; the view only
/// carries the handle. Drawing it is the one thing the current UI stack cannot
/// do yet — see `frame.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameHandle {
    pub texture: TextureId,
    pub width: u32,
    pub height: u32,
}

/// Everything the UI renders from, rebuilt from app state each frame.
#[derive(Clone, Debug)]
pub struct ViewModel {
    pub section: Section,
    /// Platform chrome the header must clear (macOS title bar / traffic lights).
    pub safe_area: SafeArea,
    pub games: Vec<GameRow>,
    pub selected: Option<usize>,
    /// The active library sort key, and whether it is descending.
    pub sort: SortKey,
    pub sort_desc: bool,
    pub playing: bool,
    pub paused: bool,
    pub core_name: String,
    pub frame: Option<FrameHandle>,
    pub status: String,
    /// Every screenshot, newest first. The section filters it by game.
    pub screenshots: Vec<ScreenshotRow>,
    /// The game whose screenshots the section shows, by game id. `None` falls
    /// back to the playing/selected game.
    pub screenshot_game: Option<i64>,
    /// The running game's save slots. Empty when nothing is running; the
    /// section shows a note instead.
    pub saves: Vec<SaveSlotRow>,
    /// Whether the running core supports save states at all.
    pub saves_supported: bool,
    /// The running game's cheats.
    pub cheats: Vec<CheatRow>,
    /// The middle column's scrollable grid (library or screenshots): its
    /// scroll offset and viewport height. The view mounts only the rows they
    /// cover, so a long grid does not lay out and re-measure every cell on
    /// each scroll step.
    pub grid_offset: f32,
    pub grid_viewport: f32,
    /// The library search query; only games whose name contains it are shown.
    pub search: String,
    /// The screenshot being previewed in the play column, by id.
    pub preview: Option<i64>,
    /// A destructive action waiting for confirmation, shown in a bar.
    pub confirm: Option<Confirm>,
    /// An in-progress rename or tag edit, shown in a bar above the grid.
    pub editing: Option<EditState>,
    /// Every core in the manifest, for the settings picker.
    pub cores: Vec<CoreRow>,
    /// Folders the library scans.
    pub library_dirs: Vec<String>,
    /// Keyboard bindings, for the settings page.
    pub bindings: Vec<BindingRow>,
    /// The console the bindings belong to (they are per console).
    pub bindings_system: String,
    /// The input descriptors the running core declared, if any.
    pub core_inputs: Vec<InputDescriptorRow>,
    /// The post-process preset for the game picture.
    pub shader: ShaderKind,
    /// The running core's options, for the settings page.
    pub core_options: Vec<CoreOptionRow>,
}

impl Default for ViewModel {
    fn default() -> Self {
        Self {
            section: Section::Library,
            safe_area: SafeArea::ZERO,
            games: Vec::new(),
            selected: None,
            sort: SortKey::Name,
            sort_desc: false,
            playing: false,
            paused: false,
            core_name: String::new(),
            frame: None,
            status: String::new(),
            screenshots: Vec::new(),
            screenshot_game: None,
            saves: Vec::new(),
            saves_supported: false,
            cheats: Vec::new(),
            grid_offset: 0.0,
            grid_viewport: 0.0,
            search: String::new(),
            preview: None,
            confirm: None,
            editing: None,
            cores: Vec::new(),
            library_dirs: Vec::new(),
            bindings: Vec::new(),
            bindings_system: String::new(),
            core_inputs: Vec::new(),
            shader: ShaderKind::Off,
            core_options: Vec::new(),
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
    /// Step the running game back one rewind snapshot.
    Rewind,
    /// Pick the post-process preset for the game picture.
    SetShader(ShaderKind),
    /// Cycle the core option at this index by `+1` / `-1`.
    CycleCoreOption(usize, i32),
    /// Write a save state to a slot (`0` is the quick slot).
    SaveState(u8),
    /// Restore a save state from a slot.
    LoadState(u8),
    /// Open the native file picker and add the chosen ROM files to the library.
    AddGames,
    /// Open the native folder picker and add the chosen folder to the library.
    OpenRom,
    /// Make the core at this `cores` index the pick for its console.
    SelectCore(usize),
    /// Stop scanning the library folder at this `library_dirs` index.
    RemoveLibraryDir(usize),
    /// Pin or unpin the game at this library index.
    TogglePin(usize),
    /// Ask to delete something destructive; the app shows a confirmation bar.
    RequestDelete(Confirm),
    /// Confirm the pending destructive action.
    ConfirmDelete,
    /// Dismiss the pending destructive action.
    CancelDelete,
    /// Start editing the game's display name.
    StartRename(i64),
    /// Start editing the game's tags.
    StartTagEdit(i64),
    /// Start typing a library search query.
    StartSearch,
    /// Clear the library search query.
    ClearSearch,
    /// Commit the pending text edit.
    CommitEdit,
    /// Discard the pending text edit.
    CancelEdit,
    /// Sort the library by this key.
    Sort(SortKey),
    /// Flip the library between ascending and descending.
    ToggleSortOrder,
    /// Take a screenshot of the running game and add it to the library.
    Screenshot,
    /// Take a screenshot and make it the running game's cover.
    ScreenshotCover,
    /// Show the screenshots section for the game with this id.
    ShowScreenshots(i64),
    /// Preview the screenshot with this id in the play column.
    PreviewScreenshot(i64),
    /// Close the screenshot preview.
    ClosePreview,
    /// Step the preview by `+1` (next) or `-1` (previous).
    StepPreview(i32),
    /// Make the screenshot with this id its game's cover.
    SetCover(i64),
    /// Write a save state to this slot.
    SaveToSlot(u8),
    /// Load the save state in this slot.
    LoadFromSlot(u8),
    /// Delete the save state in this slot.
    DeleteSlot(u8),
    /// Import a `.cht` cheat file for the running game.
    ImportCheats,
    /// Enable or disable the cheat at this index.
    ToggleCheat(usize),
    /// Reveal the screenshot with this id in the file browser.
    RevealScreenshot(i64),
    /// Open the screenshots directory in the file browser.
    OpenScreenshotsFolder,
}

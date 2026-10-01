//! Pure view data for the UI. No igui, no platform — just what the screen
//! needs to know, so the view builder can be tested headlessly.

use cgb_libretro::SystemId;
use igui::igui_core::{NodeId, Vec2};
use igui::igui_render::TextureId;

use crate::ui::theme::ThemeChoice;

/// Which page the middle column is showing. The console is the right column
/// and is usually there, so it is not a section; the settings page takes the
/// right column over for its group detail (like the screenshot preview does).
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

/// The functional groups the settings page is split into. The middle column
/// lists them and the right column shows the selected group's cards, so a long
/// settings page never becomes one endless scroll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsGroup {
    /// The one game library folder, and how to switch it.
    Library,
    /// Theme family and the light / dark look.
    Appearance,
    /// The picture post-process and the geometry anti-aliasing.
    Display,
    /// Per-console core picks and the running core's options.
    Cores,
    /// Keyboard bindings and the running core's input descriptors.
    Input,
    /// Searching and downloading cores from the catalog.
    Download,
}

impl SettingsGroup {
    /// Every group, in the order the middle column lists them.
    pub const ALL: [SettingsGroup; 6] = [
        SettingsGroup::Library,
        SettingsGroup::Appearance,
        SettingsGroup::Display,
        SettingsGroup::Cores,
        SettingsGroup::Input,
        SettingsGroup::Download,
    ];

    /// The group's name, shown in the nav and as the detail title.
    pub fn label(self) -> &'static str {
        match self {
            SettingsGroup::Library => "游戏库",
            SettingsGroup::Appearance => "外观",
            SettingsGroup::Display => "画面",
            SettingsGroup::Cores => "模拟器核心",
            SettingsGroup::Input => "按键与输入",
            SettingsGroup::Download => "下载核心",
        }
    }

    /// A one-line note under the nav label.
    pub fn description(self) -> &'static str {
        match self {
            SettingsGroup::Library => "库文件夹与备份",
            SettingsGroup::Appearance => "主题与明暗",
            SettingsGroup::Display => "后处理与抗锯齿",
            SettingsGroup::Cores => "按机种选核与核心选项",
            SettingsGroup::Input => "键盘绑定与手柄",
            SettingsGroup::Download => "从下载源获取更多核心",
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

/// What an in-progress text edit is for. The game variants carry the game id,
/// so a search (which has no game) can never be mistaken for a rename.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditTarget {
    /// The display name of the game with this id.
    GameName(i64),
    /// The tags of the game with this id, comma-separated in the field.
    GameTags(i64),
    /// The library search query.
    LibrarySearch,
    /// The downloadable-core catalog search query.
    CatalogSearch,
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

/// How much geometry anti-aliasing (MSAA) the renderer uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MsaaKind {
    /// 4x while idle, dropped while a game runs (the live image dominates).
    Auto,
    /// No anti-aliasing; a single-sample pass.
    Off,
    /// 2x MSAA.
    Two,
    /// 4x MSAA.
    Four,
}

impl MsaaKind {
    /// Every mode, in the order the settings page lists them.
    pub const ALL: [MsaaKind; 4] = [MsaaKind::Auto, MsaaKind::Off, MsaaKind::Two, MsaaKind::Four];

    /// What the button says.
    pub fn label(self) -> &'static str {
        match self {
            MsaaKind::Auto => "自动",
            MsaaKind::Off => "关闭",
            MsaaKind::Two => "2×",
            MsaaKind::Four => "4×",
        }
    }

    /// The stable key stored in settings.
    pub fn key(self) -> &'static str {
        match self {
            MsaaKind::Auto => "auto",
            MsaaKind::Off => "off",
            MsaaKind::Two => "2x",
            MsaaKind::Four => "4x",
        }
    }

    /// Parse a stored key; anything unknown means [`Auto`](MsaaKind::Auto).
    pub fn from_key(key: &str) -> Self {
        match key {
            "off" => MsaaKind::Off,
            "2x" => MsaaKind::Two,
            "4x" => MsaaKind::Four,
            _ => MsaaKind::Auto,
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
    /// The dialog's title.
    pub fn title(self) -> &'static str {
        match self {
            Confirm::DeleteGame(_) => "删除游戏？",
            Confirm::DeleteScreenshot(_) => "删除截图？",
        }
    }

    /// The dialog's body: what the action does.
    pub fn message(self) -> &'static str {
        match self {
            Confirm::DeleteGame(_) => "ROM 文件和它的截图都会被删除，此操作无法撤销。",
            Confirm::DeleteScreenshot(_) => "这张截图会被删除，此操作无法撤销。",
        }
    }
}

/// How serious a status message is, so the status line can colour it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatusKind {
    /// Neutral information (the default).
    #[default]
    Info,
    /// The action succeeded.
    Success,
    /// The action failed; the message says what went wrong.
    Error,
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
    /// The core this game runs, by manifest key, or `None` to follow the
    /// console's pick.
    pub core: Option<String>,
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
    pub cover: Option<TextureHandle>,
}

/// One console's tally in the library, for the filter row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemCount {
    pub system: SystemId,
    /// How many games the library holds for it.
    pub count: usize,
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
    pub thumb: Option<TextureHandle>,
}

/// What to call a save-state slot. Slot `0` is the quick slot; the numbered
/// slots read 1-based (`槽 1` … `槽 9`) so the label matches the `F1`–`F3`
/// hotkeys.
pub fn save_slot_label(slot: u8) -> String {
    if slot == 0 {
        "快速".to_string()
    } else {
        format!("槽 {slot}")
    }
}

/// One screenshot in the screenshots section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenshotRow {
    pub id: i64,
    pub game_id: i64,
    /// The game's display name, for the caption.
    pub game_name: String,
    pub created_at: i64,
    /// Whether this picture is its game's cover.
    pub is_cover: bool,
    /// The registered thumbnail texture, when it has been uploaded.
    pub thumb: Option<TextureHandle>,
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

/// One core in the settings page's "download a core" list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogRow {
    /// The buildbot name (`mame`, `snes9x`, …).
    pub name: String,
    /// The display name from `libretro-core-info`.
    pub display_name: String,
    /// The core-info `systemid` (`super_nes`, `mame`, …), as a string key
    /// rather than a [`SystemId`] (the core may not be supported yet).
    pub system_key: String,
    /// Whether a module with this name is already in the cores directory.
    pub downloaded: bool,
    /// Whether this app models the core's console. An unsupported core can be
    /// downloaded but never registered or run, so the list says so instead of
    /// offering a button.
    pub supported: bool,
}

/// A console the library has games for, but no available core serves. The
/// library page offers to download `core` for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissingCoreRow {
    pub system: SystemId,
    /// The downloadable core's catalog name (what the app would fetch).
    pub core: String,
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
pub struct TextureHandle {
    pub texture: TextureId,
    pub width: u32,
    pub height: u32,
}

/// Everything the UI renders from, rebuilt from app state each frame.
#[derive(Clone, Debug)]
pub struct ViewModel {
    pub section: Section,
    /// Which settings group the settings page's right column shows.
    pub settings_group: SettingsGroup,
    /// Platform chrome the header must clear (macOS title bar / traffic lights).
    pub safe_area: SafeArea,
    pub games: Vec<GameRow>,
    pub selected: Option<usize>,
    /// The active library sort key, and whether it is descending.
    pub sort: SortKey,
    pub sort_desc: bool,
    pub has_session: bool,
    pub paused: bool,
    /// Immersive play: only the game picture (plus a slim overlay bar) is
    /// mounted, and the window is asked to go borderless-fullscreen. The rest
    /// of the shell is hidden.
    pub fullscreen: bool,
    pub core_name: String,
    pub frame: Option<TextureHandle>,
    pub status: String,
    /// The severity of [`status`](Self::status), so the line can colour an
    /// error differently from a success.
    pub status_kind: StatusKind,
    /// Every screenshot, newest first. The section filters it by game.
    pub screenshots: Vec<ScreenshotRow>,
    /// The game whose screenshots the section shows, by game id. `None` falls
    /// back to the playing/selected game.
    pub screenshot_game: Option<i64>,
    /// The running game's save slots. Empty when nothing is running; the
    /// section shows a note instead.
    pub save_states: Vec<SaveSlotRow>,
    /// Whether the running core supports save states at all.
    pub save_states_supported: bool,
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
    /// The play column's live info line (FPS / resolution / core). The host
    /// updates the mounted text node in place, so it does not rebuild the tree.
    pub info: String,
    /// During a fullscreen transition, mount nothing (a blank surface) while the
    /// OS animates the window; the target UI is mounted once it settles.
    pub ui_hidden: bool,
    /// Whether the screenshots page is in multi-select mode.
    pub screenshot_select: bool,
    /// The screenshots ticked for a batch delete.
    pub selected_screenshots: Vec<i64>,
    /// An in-progress rename or tag edit, shown in a bar above the grid.
    pub editing: Option<EditTarget>,
    /// How many games the library holds, before any search or system filter.
    pub total_games: usize,
    /// The console the library is filtered to, or `None` for all of them.
    pub system_filter: Option<SystemId>,
    /// Per-console tallies, in `SYSTEMS` order, for consoles that have games.
    pub system_counts: Vec<SystemCount>,
    /// Whether the "missing core" card is folded away.
    pub missing_cores_collapsed: bool,
    /// Every core in the manifest, for the settings picker.
    pub cores: Vec<CoreRow>,
    /// The game library folder, shown on the settings page.
    pub library_root: Option<String>,
    /// Keyboard bindings, for the settings page.
    pub bindings: Vec<BindingRow>,
    /// The console the bindings belong to (they are per console).
    pub bindings_system: String,
    /// The input descriptors the running core declared, if any.
    pub core_inputs: Vec<InputDescriptorRow>,
    /// The post-process preset for the game picture.
    pub shader: ShaderKind,
    /// Geometry anti-aliasing (MSAA) mode.
    pub msaa: MsaaKind,
    /// The UI theme family, and whether the light appearance is used.
    pub theme_choice: ThemeChoice,
    pub light: bool,
    /// The running core's options, for the settings page.
    pub core_options: Vec<CoreOptionRow>,
    /// The middle column's initial width in logical pixels. The live width is
    /// owned by the UI (the resize handle's shared cell); this seeds it at
    /// construction.
    pub content_width: f32,
    /// The library / screenshots grid column count. The app steps it 2 / 3 / 4
    /// from the middle width (see [`library_columns`](crate::ui::library_columns)).
    pub grid_columns: usize,
    /// The downloadable-core catalog, filtered by [`catalog_query`](Self::catalog_query).
    pub catalog: Vec<CatalogRow>,
    /// The catalog search query.
    pub catalog_query: String,
    /// How many cores the whole catalog holds, before filtering.
    pub catalog_total: usize,
    /// The core currently downloading, if any.
    pub catalog_downloading: Option<String>,
    /// A human line about the catalog / download state.
    pub catalog_status: String,
    /// Download progress in `0.0..=1.0`; `None` when the total is unknown.
    pub catalog_progress: Option<f32>,
    /// Consoles the library has games for but no available core serves, each
    /// with the core the app would download. Drives the library page's
    /// "missing core" card; empty when nothing is missing.
    pub missing_cores: Vec<MissingCoreRow>,
}

impl ViewModel {
    /// Set the status line's text and severity together, so its colour cannot
    /// go stale.
    pub fn set_status(&mut self, text: impl Into<String>, kind: StatusKind) {
        self.status = text.into();
        self.status_kind = kind;
    }

    /// The game the shell is focused on: the selected library row, if any.
    /// Returns `None` when nothing is selected (e.g. a game launched straight
    /// from `--rom` that is not in the library list).
    pub fn selected_game(&self) -> Option<&GameRow> {
        self.selected.and_then(|index| self.games.get(index))
    }
}

impl Default for ViewModel {
    fn default() -> Self {
        Self {
            section: Section::Library,
            settings_group: SettingsGroup::Library,
            safe_area: SafeArea::ZERO,
            games: Vec::new(),
            selected: None,
            sort: SortKey::Name,
            sort_desc: false,
            has_session: false,
            paused: false,
            fullscreen: false,
            core_name: String::new(),
            frame: None,
            status: String::new(),
            status_kind: StatusKind::Info,
            screenshots: Vec::new(),
            screenshot_game: None,
            save_states: Vec::new(),
            save_states_supported: false,
            cheats: Vec::new(),
            grid_offset: 0.0,
            grid_viewport: 0.0,
            search: String::new(),
            preview: None,
            info: String::new(),
            ui_hidden: false,
            screenshot_select: false,
            selected_screenshots: Vec::new(),
            editing: None,
            total_games: 0,
            system_filter: None,
            system_counts: Vec::new(),
            missing_cores_collapsed: true,
            cores: Vec::new(),
            library_root: None,
            bindings: Vec::new(),
            bindings_system: String::new(),
            core_inputs: Vec::new(),
            shader: ShaderKind::Off,
            msaa: MsaaKind::Auto,
            theme_choice: ThemeChoice::default(),
            light: false,
            core_options: Vec::new(),
            content_width: crate::ui::view::CONTENT_DEFAULT_WIDTH,
            grid_columns: 2,
            catalog: Vec::new(),
            catalog_query: String::new(),
            catalog_total: 0,
            catalog_downloading: None,
            catalog_status: String::new(),
            catalog_progress: None,
            missing_cores: Vec::new(),
        }
    }
}

/// What the user asked for, drained by the app after routing input.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Show a page.
    Show(Section),
    /// Show one settings group in the settings page's right column.
    ShowSettingsGroup(SettingsGroup),
    /// Start the game at this library index.
    Play(usize),
    TogglePause,
    /// Enter or leave the immersive fullscreen play view.
    ToggleFullscreen,
    Reset,
    /// Step the running game back one rewind snapshot.
    Rewind,
    /// Pick the post-process preset for the game picture.
    SetShader(ShaderKind),
    /// Pick the geometry anti-aliasing (MSAA) mode.
    SetMsaa(MsaaKind),
    /// Pick the UI theme family.
    SetThemeChoice(ThemeChoice),
    /// Switch the light / dark appearance.
    SetLight(bool),
    /// Cycle the core option at this index by `+1` / `-1`.
    CycleCoreOption(usize, i32),
    /// Open the native file picker and add the chosen ROM files to the library.
    AddGames,
    /// Open the native folder picker and switch to the chosen game library.
    SwitchLibrary,
    /// Filter the library to one console, or `None` for all of them.
    FilterSystem(Option<SystemId>),
    /// Fold or unfold the "missing core" card.
    ToggleMissingCores,
    /// Make the core at this `cores` index the pick for its console.
    SelectCore(usize),
    /// Begin typing a downloadable-core catalog search.
    StartCatalogSearch,
    /// Clear the downloadable-core catalog search query.
    ClearCatalogSearch,
    /// Refresh the downloadable-core catalog from the network.
    RefreshCatalog,
    /// Download the catalog core at this `catalog` index.
    DownloadCore(usize),
    /// Download the core recommended for this console (the library page's
    /// "missing core" card).
    DownloadRecommendedCore(SystemId),
    /// Download every core the "missing core" prompt listed.
    DownloadMissingCores,
    /// Pin or unpin the game at this library index.
    TogglePin(usize),
    /// Ask to delete something destructive; the app opens a confirmation dialog.
    RequestDelete(Confirm),
    /// Confirm the pending destructive action.
    ConfirmDelete,
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
    /// Write a save state to this slot (`0` is the quick slot).
    SaveToSlot(u8),
    /// Load the save state in this slot (`0` is the quick slot).
    LoadFromSlot(u8),
    /// Delete the save state in this slot.
    DeleteSlot(u8),
    /// Import a `.cht` cheat file for the running game.
    ImportCheats,
    /// Enable or disable the cheat at this index.
    ToggleCheat(usize),
    /// Reveal the screenshot with this id in the file browser.
    RevealScreenshot(i64),
    /// Toggle the screenshots page's multi-select mode.
    ToggleScreenshotSelect,
    /// Tick or untick the screenshot with this id.
    ToggleScreenshotSelected(i64),
    /// Delete every ticked screenshot.
    DeleteSelectedScreenshots,
    /// Open the screenshots directory in the file browser.
    OpenScreenshotsFolder,
    /// Open a game card's context menu at `position` (a right click).
    GameContextMenu {
        index: usize,
        position: Vec2,
    },
    /// Open a console's core picker menu, anchored to the `Select` trigger.
    OpenCoreMenu {
        system: SystemId,
        anchor: NodeId,
    },
    /// Open the per-game console picker at `position` (from a card's menu).
    OpenSystemMenu {
        id: i64,
        position: Vec2,
    },
    /// Run this game as `system`, overriding the system its file extension
    /// suggests. Persisted in the library.
    SetGameSystem {
        id: i64,
        system: SystemId,
    },
    /// Open the per-game core picker at `position` (from a card's menu).
    OpenGameCoreMenu {
        id: i64,
        position: Vec2,
    },
    /// Run this game with `core` (a manifest key), overriding the console's
    /// pick. `None` clears the override. Persisted in the library.
    SetGameCore {
        id: i64,
        core: Option<String>,
    },
    /// A single click on a card. The host turns two in quick succession into a
    /// "play" (a card needs a double click; the play button uses [`Action::Play`]).
    CardActivate(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_status_sets_text_and_severity_together() {
        let mut model = ViewModel::default();
        model.set_status("boom", StatusKind::Error);
        assert_eq!(model.status, "boom");
        assert_eq!(model.status_kind, StatusKind::Error);
    }

    #[test]
    fn msaa_kind_keys_round_trip() {
        for mode in MsaaKind::ALL {
            assert_eq!(MsaaKind::from_key(mode.key()), mode);
        }
        // A removed or unknown key falls back to auto, not to a fixed count.
        assert_eq!(MsaaKind::from_key("nonsense"), MsaaKind::Auto);
    }
}

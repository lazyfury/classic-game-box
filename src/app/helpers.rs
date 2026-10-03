//! Free helpers: paths, core manifest, icons/shader, filesystem.

use super::*;

/// Wall-clock milliseconds since the Unix epoch.
pub(crate) fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// Reveal a file in the platform file browser (Finder on macOS).
pub(crate) fn reveal_path(path: &Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn();
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(parent) = path.parent() {
            open_path(parent);
        }
    }
}

/// Open a directory in the platform file browser.
pub(crate) fn open_path(path: &Path) {
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(not(target_os = "macos"))]
    let opener = "xdg-open";
    let _ = std::process::Command::new(opener).arg(path).spawn();
}

/// The core manifest: the packaged `<app data>/cores/cores.json`, else the
/// bundle's `Resources/cores/cores.json`, else the dev checkout's
/// `cores/cores.json`. Missing is not an error — the app then just has no core
/// to run, and says so when a game is started.
pub(crate) fn load_core_manifest(paths: &Paths) -> Vec<CoreSpec> {
    let mut cores = load_shipped_cores(paths);
    // Cores fetched by `--download-core` are registered beside the catalog;
    // merge them in so the core picker offers them. A shipped row with the same
    // `(system, key)` wins the slot, but the downloaded module still shadows the
    // packaged one because `find_module` looks in app data first.
    let downloaded = paths.cores.join("downloaded.json");
    if downloaded.is_file() {
        for spec in load_cores(&downloaded) {
            if !cores
                .iter()
                .any(|core| core.system == spec.system && core.key == spec.key)
            {
                cores.push(spec);
            }
        }
    }
    // Only offer a core whose module is actually present: a checkout that built
    // `--minimal` still has the full `cores.json`, and a packaged app may ship a
    // subset. This resolves through the same search order `find_module` uses, so
    // anything kept here loads. Blocked cores are dropped even when a module for
    // them exists (e.g. one registered before it was blocked).
    cores.retain(|core| !is_blocked(&core.key) && resolve_module(paths, &core.module).is_file());
    cores
}

/// Resolve a manifest `module` (a bare file name or a path) the way the app
/// loads it: an absolute or already-existing path is used as given, else the
/// file is searched in `<app data>/cores`, the bundle's `Resources/cores`, then
/// the checkout's `cores/dist`. Returns the first hit, or the packaged path
/// when none exists (so callers can test `is_file`).
pub(crate) fn resolve_module(paths: &Paths, module: &Path) -> PathBuf {
    if module.is_absolute() || module.is_file() {
        return module.to_path_buf();
    }
    let packaged = paths.cores.join(module);
    if packaged.is_file() {
        return packaged;
    }
    if let Some(resources) = resource_dir() {
        let bundled = resources.join("cores").join(module);
        if bundled.is_file() {
            return bundled;
        }
    }
    let dev = Path::new("cores/dist").join(module);
    if dev.is_file() {
        return dev;
    }
    packaged
}

/// The downloadable core to recommend for a console: the shipped manifest's
/// preferred (first) core that the catalog can fetch, else the first catalog
/// core that serves the console. `None` when nothing downloadable serves it
/// (e.g. a console only the bundled / self-built cores cover, like J2ME).
pub(crate) fn recommend_core(
    shipped: &[CoreSpec],
    catalog: &Catalog,
    system: SystemId,
) -> Option<String> {
    if let Some(key) = shipped
        .iter()
        .filter(|core| core.system == system)
        .map(|core| core.key.as_str())
        .find(|key| !is_blocked(key) && catalog.get(key).is_some())
    {
        return Some(key.to_string());
    }
    catalog
        .cores
        .iter()
        .find(|entry| {
            !is_blocked(&entry.name) && SystemId::parse_key(&entry.system) == Some(system)
        })
        .map(|entry| entry.name.clone())
}

/// The consoles a set of games needs but no available core serves, in
/// first-seen order, each with the core to download. A console with no
/// downloadable core is left out (there is nothing to offer).
pub(crate) fn missing_core_rows(
    games: &[Game],
    available: &[CoreSpec],
    shipped: &[CoreSpec],
    catalog: &Catalog,
) -> Vec<MissingCoreRow> {
    let mut systems: Vec<SystemId> = Vec::new();
    for game in games {
        if !systems.contains(&game.system) {
            systems.push(game.system);
        }
    }
    systems
        .into_iter()
        .filter(|system| !available.iter().any(|core| core.system == *system))
        .filter_map(|system| {
            recommend_core(shipped, catalog, system).map(|core| MissingCoreRow { system, core })
        })
        .collect()
}

/// The cores shipped with the app (packaged app-data copy, bundled Resources,
/// or the checkout's `cores/cores.json`).
pub(crate) fn load_shipped_cores(paths: &Paths) -> Vec<CoreSpec> {
    let packaged = paths.cores.join("cores.json");
    if packaged.is_file() {
        return load_cores(&packaged);
    }
    if let Some(resources) = resource_dir() {
        let bundled = resources.join("cores/cores.json");
        if bundled.is_file() {
            return load_cores(&bundled);
        }
    }
    load_cores(Path::new("cores/cores.json"))
}

/// The bundled FreeJ2ME-Plus jar and JRE: the packaged app's Resources, or the
/// build output in a checkout. `None` when it was never built, so the app falls
/// back to a `java` already on `PATH`.
pub(crate) fn j2me_dir() -> Option<PathBuf> {
    if let Some(resources) = resource_dir() {
        let bundled = resources.join(BUNDLED_J2ME);
        if bundled.is_dir() {
            return Some(bundled);
        }
    }
    let dev = PathBuf::from("cores/dist").join(BUNDLED_J2ME);
    dev.is_dir().then_some(dev)
}

/// The bundled PPSSPP assets: the packaged app's Resources, or the build output
/// in a checkout. `None` when they were never fetched, so a user who installed
/// them into the system directory by hand is left alone.
pub(crate) fn ppsspp_assets_dir() -> Option<PathBuf> {
    if let Some(resources) = resource_dir() {
        let bundled = resources.join(BUNDLED_PPSSPP);
        if bundled.join("compat.ini").is_file() {
            return Some(bundled);
        }
    }
    let dev = PathBuf::from("cores/dist").join(BUNDLED_PPSSPP);
    dev.join("compat.ini").is_file().then_some(dev)
}

/// Put `dir` first on `PATH`. Process-global, called once at startup before the
/// window and any worker threads exist. The FreeJ2ME core `chdir`s to the
/// system directory before it `exec`s `java`, so the entry must be absolute
/// (the caller canonicalises it).
pub(crate) fn prepend_path(dir: &Path) {
    let mut paths = vec![dir.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    if let Ok(joined) = std::env::join_paths(paths) {
        std::env::set_var("PATH", joined);
    }
}

/// Make the FreeJ2ME Java worker run headless.
///
/// The libretro path only uses AWT offscreen (fonts, `BufferedImage`,
/// `Graphics2D`), but on macOS initialising the Cocoa AWT toolkit turns the
/// `java` process into the foreground app and takes key focus from our window,
/// so keyboard input stops reaching the game. Headless keeps every AWT class
/// the core needs without a toolkit. `JAVA_TOOL_OPTIONS` is read by the bundled
/// `java` launcher and inherited across the core's `fork/exec`; a user's own
/// options are preserved by appending rather than replacing.
pub(crate) fn ensure_headless_java() {
    const HEADLESS: &str = "-Djava.awt.headless=true";
    let existing = std::env::var("JAVA_TOOL_OPTIONS").unwrap_or_default();
    if existing.split_whitespace().any(|option| option == HEADLESS) {
        return;
    }
    let joined = match existing.trim() {
        "" => HEADLESS.to_string(),
        options => format!("{options} {HEADLESS}"),
    };
    std::env::set_var("JAVA_TOOL_OPTIONS", joined);
}

/// Map the UI preset to the backend's effect.
pub(crate) fn texture_effect(kind: ShaderKind) -> TextureEffect {
    match kind {
        ShaderKind::Off => TextureEffect::None,
        ShaderKind::Scanlines => TextureEffect::Scanlines,
        ShaderKind::Crt => TextureEffect::Crt,
        ShaderKind::Lcd => TextureEffect::Lcd,
        ShaderKind::Sharpen => TextureEffect::Sharpen,
    }
}

/// True when `path` is `root` itself or inside it. Both sides are canonicalized
/// first, so a `..` segment cannot make an outside path look like it is inside.
pub(crate) fn inside_library(root: &Path, path: &Path) -> bool {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    path.starts_with(&root)
}

#[cfg(test)]
mod tests {
    use super::ensure_headless_java;

    #[test]
    fn headless_java_is_added_once_and_keeps_existing_options() {
        let saved = std::env::var_os("JAVA_TOOL_OPTIONS");
        std::env::set_var("JAVA_TOOL_OPTIONS", "-Xmx64m");

        ensure_headless_java();
        let first = std::env::var("JAVA_TOOL_OPTIONS").unwrap_or_default();
        assert!(first.contains("-Xmx64m"), "keeps the user's options");
        assert!(first
            .split_whitespace()
            .any(|option| option == "-Djava.awt.headless=true"));

        // Idempotent: a second call must not stack the flag.
        ensure_headless_java();
        assert_eq!(
            first,
            std::env::var("JAVA_TOOL_OPTIONS").unwrap_or_default()
        );

        match saved {
            Some(value) => std::env::set_var("JAVA_TOOL_OPTIONS", value),
            None => std::env::remove_var("JAVA_TOOL_OPTIONS"),
        }
    }
}

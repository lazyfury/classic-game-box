//! The core-management CLI: `--force-update`, `--search-core` and
//! `--download-core`. It runs before the UI so a core can be fetched and
//! proven loadable headlessly.

use std::path::Path;

use cgb_cores::{
    cache_path, download_core, is_blocked, register_downloaded, registry_path, update_catalog,
    write_catalog, Catalog, CatalogEntry, Platform, DEFAULT_SOURCE,
};
use cgb_libretro::CoreHost;
use cgb_paths::Paths;

use crate::cli::Args;

/// Handle the core flags. Returns the process exit code.
pub fn run(args: &Args) -> i32 {
    let paths = Paths::platform();
    if let Err(error) = paths.ensure() {
        eprintln!("无法创建数据目录：{error}");
        return 1;
    }
    let base = args.core_base_url.as_deref().unwrap_or(DEFAULT_SOURCE);
    let cache = cache_path(&paths.cores);

    let mut code = 0;
    if args.force_update {
        code |= force_update(base, &cache);
    }
    if let Some(query) = &args.search_cores {
        code |= search(query, &cache);
    }
    if let Some(name) = &args.download_core {
        code |= download(
            name,
            base,
            &cache,
            &paths.cores,
            &paths.system,
            &paths.saves,
        );
    }
    code
}

/// Refresh the catalog cache from the network.
fn force_update(base: &str, cache: &Path) -> i32 {
    let Some(platform) = Platform::current() else {
        eprintln!("无法确定本机平台，buildbot 没有对应目录");
        return 1;
    };
    match update_catalog(base, platform) {
        Ok(catalog) => match write_catalog(cache, &catalog) {
            Ok(()) => {
                println!(
                    "已更新下载源：{} 个核心（{}/{}/latest）-> {}",
                    catalog.cores.len(),
                    platform.os,
                    platform.arch,
                    cache.display()
                );
                0
            }
            Err(error) => {
                eprintln!("写入目录缓存失败：{error}");
                1
            }
        },
        Err(error) => {
            eprintln!("更新下载源失败：{error}");
            1
        }
    }
}

/// Search the cached catalog.
fn search(query: &str, cache: &Path) -> i32 {
    let catalog = Catalog::load(cache);
    let hits = catalog.search(query);
    for entry in &hits {
        let system = if entry.system.is_empty() {
            "-"
        } else {
            entry.system.as_str()
        };
        println!("{:<24} {:<52} {}", entry.name, entry.display_name, system);
    }
    let source = if catalog.source.is_empty() {
        DEFAULT_SOURCE
    } else {
        catalog.source.as_str()
    };
    println!(
        "共 {} 个匹配 / 目录 {} 个（来源 {}）",
        hits.len(),
        catalog.cores.len(),
        source
    );
    0
}

/// Download one core, then open it to prove it loads.
fn download(
    name: &str,
    base: &str,
    cache: &Path,
    cores: &Path,
    system: &Path,
    saves: &Path,
) -> i32 {
    let Some(platform) = Platform::current() else {
        eprintln!("无法确定本机平台，buildbot 没有对应目录");
        return 1;
    };
    if is_blocked(name) {
        eprintln!("核心 {name} 已屏蔽（本应用无法运行它）");
        return 1;
    }
    let catalog = Catalog::load(cache);
    let entry = catalog.get(name).cloned().unwrap_or_else(|| CatalogEntry {
        name: name.to_string(),
        display_name: name.to_string(),
        system: String::new(),
        extensions: String::new(),
    });

    match download_core(&entry, base, platform, cores) {
        Ok(path) => {
            println!("已下载：{}", path.display());
            match register_downloaded(&registry_path(cores), &entry, platform) {
                Ok(true) => println!("已登记到核心清单（可在“每个机种选核”里选到）"),
                Ok(false) => println!(
                    "机种 `{}` 本应用暂不支持，只能用 --core <路径> 加载",
                    entry.system
                ),
                Err(error) => eprintln!("登记失败：{error}"),
            }
            match CoreHost::new(&path, system, saves) {
                Ok(host) => {
                    let info = host.system_info();
                    println!(
                        "可加载：{} {}  扩展名 [{}]  need_fullpath={}",
                        info.library_name,
                        info.library_version,
                        info.valid_extensions.join("|"),
                        info.need_fullpath
                    );
                    0
                }
                Err(error) => {
                    eprintln!("已下载但加载失败：{error}");
                    1
                }
            }
        }
        Err(error) => {
            eprintln!("下载失败：{error}");
            1
        }
    }
}

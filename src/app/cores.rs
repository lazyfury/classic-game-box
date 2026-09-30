//! Core manifest, selection and downloads — methods on [`super::App`].

use super::*;

impl super::App {
    /// Rebuild the settings page's downloadable-core rows from the query.
    pub(super) fn rebuild_catalog(&mut self) {
        self.model.catalog_total = self.catalog.cores.len();
        let platform = Platform::current();
        let cores_dir = self.paths.cores.clone();
        let query = self.model.catalog_query.trim().to_string();
        // An empty query lists nothing: the catalog is hundreds of cores, and
        // the search box is the way in.
        self.model.catalog = if query.is_empty() {
            Vec::new()
        } else {
            self.catalog
                .search(&query)
                .into_iter()
                .take(CATALOG_LIST_LIMIT)
                .map(|entry| CatalogRow {
                    name: entry.name.clone(),
                    display_name: entry.display_name.clone(),
                    system_key: entry.system.clone(),
                    downloaded: platform.is_some_and(|platform| {
                        cores_dir.join(entry.module_file(platform)).is_file()
                    }),
                    supported: SystemId::parse_key(&entry.system).is_some(),
                })
                .collect()
        };
    }

    /// Clear the catalog search and rebuild the list.
    pub(super) fn clear_catalog_search(&mut self) {
        self.model.catalog_query.clear();
        self.model.editing = None;
        self.actions.clear_edit();
        self.rebuild_catalog();
        self.dirty = true;
    }

    /// Refresh the downloadable-core catalog on a background thread;
    /// `poll_downloads` applies the result.
    pub(super) fn refresh_catalog(&mut self) {
        if self.download_rx.is_some() {
            return;
        }
        let Some(platform) = Platform::current() else {
            self.model
                .set_status("无法确定本机平台，无法刷新下载源", StatusKind::Error);
            self.dirty = true;
            return;
        };
        let cache = cache_path(&self.paths.cores);
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let result = update_catalog(DEFAULT_SOURCE, platform)
                .and_then(|catalog| write_catalog(&cache, &catalog).map(|()| catalog));
            let event = match result {
                Ok(catalog) => DownloadEvent::CatalogRefreshed(Box::new(catalog)),
                Err(error) => DownloadEvent::Failed {
                    name: "下载源".to_string(),
                    error: error.to_string(),
                },
            };
            let _ = tx.send(event);
        });
        self.download_rx = Some(rx);
        self.model.catalog_status = "正在刷新下载源…".to_string();
        self.model.catalog_progress = None;
        self.model.set_status("正在刷新下载源…", StatusKind::Info);
        self.dirty = true;
    }

    /// Download one core on a background thread; `poll_downloads` applies the
    /// result and re-reads the manifest.
    pub(super) fn download_core(&mut self, name: &str) {
        if self.download_rx.is_some() {
            return;
        }
        let Some(platform) = Platform::current() else {
            self.model
                .set_status("无法确定本机平台，无法下载核心", StatusKind::Error);
            self.dirty = true;
            return;
        };
        let entry = self
            .catalog
            .get(name)
            .cloned()
            .unwrap_or_else(|| cgb_cores::CatalogEntry {
                name: name.to_string(),
                display_name: name.to_string(),
                system: String::new(),
                extensions: String::new(),
            });
        let cores_dir = self.paths.cores.clone();
        let registry = registry_path(&cores_dir);
        let (tx, rx) = channel();
        let progress = tx.clone();
        let entry_for_thread = entry.clone();
        std::thread::spawn(move || {
            let result = download_core_with_progress(
                &entry_for_thread,
                DEFAULT_SOURCE,
                platform,
                &cores_dir,
                move |received, total| {
                    let _ = progress.send(DownloadEvent::Progress { received, total });
                },
            );
            let event = match result {
                Ok(_path) => {
                    let registered = register_downloaded(&registry, &entry_for_thread, platform)
                        .unwrap_or(false);
                    DownloadEvent::Done {
                        name: entry_for_thread.name.clone(),
                        registered,
                    }
                }
                Err(error) => DownloadEvent::Failed {
                    name: entry_for_thread.name.clone(),
                    error: error.to_string(),
                },
            };
            let _ = tx.send(event);
        });
        self.download_rx = Some(rx);
        self.model.catalog_downloading = Some(entry.name.clone());
        self.model.catalog_status = format!("正在下载 {}…", entry.name);
        self.model.catalog_progress = Some(0.0);
        self.model
            .set_status(format!("正在下载 {}…", entry.name), StatusKind::Info);
        self.dirty = true;
    }

    /// Drain the background download's events. Called once per frame.
    pub(super) fn poll_downloads(&mut self) {
        let Some(rx) = self.download_rx.take() else {
            return;
        };
        let mut events = Vec::new();
        let mut disconnected = false;
        loop {
            match rx.try_recv() {
                Ok(event) => events.push(event),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if !disconnected {
            self.download_rx = Some(rx);
        }
        for event in events {
            self.on_download_event(event);
        }
    }

    /// Apply one background download event.
    pub(super) fn on_download_event(&mut self, event: DownloadEvent) {
        match event {
            DownloadEvent::Progress { received, total } => {
                let progress = total
                    .filter(|total| *total > 0)
                    .map(|total| (received as f32 / total as f32).clamp(0.0, 1.0));
                self.model.catalog_progress = progress;
                // Mirror the progress into the footer, so it is visible even
                // when the settings card has scrolled away.
                if let Some(name) = self.model.catalog_downloading.clone() {
                    let text = match progress {
                        Some(progress) => format!("正在下载 {name}… {:.0}%", progress * 100.0),
                        None => format!("正在下载 {name}…"),
                    };
                    self.model.set_status(text, StatusKind::Info);
                }
                self.dirty = true;
            }
            DownloadEvent::Done { name, registered } => {
                self.model.catalog_downloading = None;
                self.model.catalog_progress = None;
                self.model.catalog_status = if registered {
                    format!("已下载并登记 {name}")
                } else {
                    format!("已下载 {name}（机种暂不支持，用 --core <路径> 加载）")
                };
                self.cores = load_core_manifest(&self.paths);
                self.rebuild_settings_view();
                self.model
                    .set_status(format!("核心 {name} 已就绪"), StatusKind::Success);
                self.dirty = true;
            }
            DownloadEvent::Failed { name, error } => {
                self.model.catalog_downloading = None;
                self.model.catalog_progress = None;
                self.model.catalog_status = format!("{name} 失败：{error}");
                self.model
                    .set_status(format!("{name} 失败：{error}"), StatusKind::Error);
                self.dirty = true;
            }
            DownloadEvent::CatalogRefreshed(catalog) => {
                self.catalog = *catalog;
                self.model.catalog_status =
                    format!("下载源已更新（{} 个核心）", self.catalog.cores.len());
                self.rebuild_catalog();
                self.model.set_status("下载源已更新", StatusKind::Success);
                self.dirty = true;
            }
        }
    }

    /// Remember a core pick for its console.
    pub(super) fn select_core(&mut self, index: usize) {
        let Some(row) = self.model.cores.get(index) else {
            return;
        };
        let (system, key) = (row.system, row.key.clone());
        self.settings.set_core_key(system, Some(&key));
        let _ = self.settings.save(&self.paths.settings_json);
        self.model.set_status(
            format!("{} 的核心已切换为 {key}", system.short()),
            StatusKind::Success,
        );
        self.rebuild_settings_view();
    }

    /// Resolve which core runs this ROM. `--core` wins over the saved pick,
    /// which wins over the manifest's first core for the console. A key is
    /// looked up in the merged manifests; a `--core <path>` module is used as
    /// given.
    pub(super) fn resolve_core(&self, system: SystemId) -> Result<CoreSpec, String> {
        let spec = match &self.core_override {
            Some(CoreOverride::Key(key)) => choose_core(&self.cores, system, Some(key))
                .ok_or_else(|| {
                    format!("没有核心 `{key}` 支持 {}（见 cores.json）", system.short())
                })?,
            Some(CoreOverride::Module(module)) => {
                return Ok(CoreSpec::custom(module.clone(), system));
            }
            None => choose_core(&self.cores, system, self.settings.core_key(system))
                .ok_or_else(|| format!("{} 没有可用核心（见 cores.json）", system.short()))?,
        };
        let mut spec = spec.clone();
        spec.module = self.find_module(&spec.module);
        Ok(spec)
    }

    /// Locate a module: an absolute or existing path is used as given; a bare
    /// file name is searched in the packaged `cores/` dir, then the dev build
    /// output in `cores/dist` (see `cores/README.md`).
    pub(super) fn find_module(&self, module: &Path) -> PathBuf {
        if module.is_absolute() || module.is_file() {
            return module.to_path_buf();
        }
        let packaged = self.paths.cores.join(module);
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
}

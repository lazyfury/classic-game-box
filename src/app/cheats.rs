//! Cheat list — methods on [`super::App`].

use super::*;

impl super::App {
    /// Project the running game's cheats into the view.
    pub(super) fn populate_cheats(&mut self) {
        self.model.cheats = self
            .cheats
            .iter()
            .map(|cheat| crate::ui::CheatRow {
                desc: cheat.desc.clone(),
                code: cheat.code.clone(),
                enabled: cheat.enabled,
            })
            .collect();
        self.dirty = true;
    }

    /// Import a RetroArch `.cht` for the running game, replacing its list.
    pub(super) fn import_cheats(&mut self) {
        let Some(path) = self.cheat_path.clone() else {
            self.model
                .set_status("没有正在运行的游戏".to_string(), StatusKind::Info);
            self.dirty = true;
            return;
        };
        let Some(file) = rfd::FileDialog::new()
            .set_title("导入金手指 (.cht)")
            .add_filter("Cheat", &["cht"])
            .pick_file()
        else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(&file) else {
            self.model
                .set_status("读取金手指文件失败".to_string(), StatusKind::Error);
            self.dirty = true;
            return;
        };
        let cheats = crate::library::parse_cht(&text);
        if cheats.is_empty() {
            self.model
                .set_status("文件里没有可用的金手指".to_string(), StatusKind::Error);
            self.dirty = true;
            return;
        }
        let count = cheats.len();
        self.cheats = cheats;
        let _ = crate::library::save_cheats(&path, &self.cheats);
        if let Some(session) = self.session.as_ref() {
            session.apply_cheats(&self.cheats);
        }
        self.populate_cheats();
        self.model
            .set_status(format!("已导入 {count} 条金手指"), StatusKind::Success);
    }

    /// Enable or disable one cheat on the running core, and persist the list.
    pub(super) fn toggle_cheat(&mut self, index: usize) {
        let Some(cheat) = self.cheats.get_mut(index) else {
            return;
        };
        cheat.enabled = !cheat.enabled;
        let enabled = cheat.enabled;
        let desc = cheat.desc.clone();
        if let Some(path) = self.cheat_path.clone() {
            let _ = crate::library::save_cheats(&path, &self.cheats);
        }
        // Re-apply the whole list: the cores ignore the enabled flag, so a
        // disabled cheat has to be left out rather than sent as disabled.
        if let Some(session) = self.session.as_ref() {
            session.apply_cheats(&self.cheats);
        }
        self.populate_cheats();
        self.model.set_status(
            format!("{}：{desc}", if enabled { "已开启" } else { "已关闭" }),
            StatusKind::Success,
        );
    }
}

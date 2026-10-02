//! Settings: shader, MSAA, theme, core options — methods on [`super::App`].

use super::*;

impl super::App {
    /// Pick the picture post-process, persist it, and apply it.
    pub(super) fn set_shader(&mut self, kind: ShaderKind) {
        self.shader = kind;
        self.settings.shader = kind.key().to_string();
        let _ = self.settings.save(&self.paths.settings_json);
        self.apply_shader();
        self.rebuild_settings_view();
        self.model
            .set_status(format!("画面效果：{}", kind.label()), StatusKind::Info);
    }

    /// Pick the anti-aliasing mode, persist it and apply it.
    pub(super) fn set_msaa(&mut self, mode: MsaaKind) {
        self.msaa = mode;
        self.settings.msaa = mode.key().to_string();
        let _ = self.settings.save(&self.paths.settings_json);
        self.sync_msaa();
        self.rebuild_settings_view();
        self.model
            .set_status(format!("抗锯齿：{}", mode.label()), StatusKind::Info);
    }

    /// Switch the UI theme / appearance, persist it and rebuild so the change
    /// shows immediately (a theme switch also rebuilds the overlays).
    pub(super) fn set_theme(&mut self, choice: ThemeChoice, light: bool) {
        self.theme_choice = choice;
        self.light = light;
        self.settings.theme = Some(choice.key().to_string());
        self.settings.light = light;
        let _ = self.settings.save(&self.paths.settings_json);
        self.theme = choice.theme(if light { Mode::Light } else { Mode::Dark });
        if let Some(backend) = self.backend.clone() {
            backend
                .borrow_mut()
                .set_clear_color(self.theme.background());
        }
        self.rebuild_settings_view();
        self.dirty = true;
        let label = if light { "浅色" } else { "深色" };
        self.model.set_status(
            format!("外观：{} · {label}", choice.label()),
            StatusKind::Info,
        );
    }

    /// Apply the current preset to the running game's texture.
    pub(super) fn apply_shader(&mut self) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let Some(backend) = self.backend.clone() else {
            return;
        };
        let mut backend = backend.borrow_mut();
        session.set_effect(&mut backend, texture_effect(self.shader));
    }

    /// Read the running core's options, apply any remembered values, and
    /// project them into the settings page.
    pub(super) fn reload_core_options(&mut self) {
        let Some(session) = self.session.as_ref() else {
            self.core_options.clear();
            self.rebuild_settings_view();
            return;
        };
        let core_key = session.core_key().to_string();
        // A saved choice is a live override of the manifest default applied at
        // load (see `Session::start`).
        for option in session.core_options() {
            let key = format!("{core_key}:{}", option.key);
            if let Some(value) = self.settings.core_options.get(&key) {
                session.set_core_option(&option.key, value);
            }
        }
        self.core_options = session.core_options();
        self.rebuild_settings_view();
    }

    /// Cycle one core option to its previous/next value and persist it.
    pub(super) fn cycle_core_option(&mut self, index: usize, delta: i32) {
        let Some(option) = self.core_options.get(index) else {
            return;
        };
        if option.values.is_empty() {
            return;
        }
        let current = option
            .values
            .iter()
            .position(|(value, _)| *value == option.value)
            .unwrap_or(0) as i32;
        let next = (current + delta).rem_euclid(option.values.len() as i32) as usize;
        let value = option.values[next].0.clone();
        let display = option.values[next].1.clone();
        let option_key = option.key.clone();
        let core_key = self
            .session
            .as_ref()
            .map(|session| session.core_key().to_string());
        if let Some(session) = self.session.as_ref() {
            session.set_core_option(&option_key, &value);
        }
        if let Some(core_key) = core_key {
            self.settings
                .core_options
                .insert(format!("{core_key}:{option_key}"), value.clone());
            let _ = self.settings.save(&self.paths.settings_json);
        }
        if let Some(option) = self.core_options.get_mut(index) {
            option.value = value;
        }
        self.rebuild_settings_view();
        self.model
            .set_status(format!("{option_key} = {display}"), StatusKind::Info);
    }

    /// Project the cores, folders and keyboard bindings into the settings page.
    pub(super) fn rebuild_settings_view(&mut self) {
        self.model.cores = self
            .cores
            .iter()
            .map(|core| CoreRow {
                key: core.key.clone(),
                name: core.name.clone(),
                system: core.system,
                // The core that would run this console now: the saved pick, else
                // the manifest's first core for it.
                selected: choose_core(
                    &self.cores,
                    core.system,
                    self.settings.core_key(core.system),
                )
                .map(|chosen| chosen.key == core.key)
                .unwrap_or(false),
            })
            .collect();
        self.model.library_root = self.settings.library_root.clone();
        self.model.bindings = self
            .bindings
            .get(&self.active_system)
            .map(binding_rows)
            .unwrap_or_default();
        self.model.bindings_system = self.active_system.name().to_string();
        self.model.keyboard_mode = self.keyboard_mode;
        self.model.shader = self.shader;
        self.model.msaa = self.msaa;
        self.model.theme_choice = self.theme_choice;
        self.model.light = self.light;
        self.model.core_options = self
            .core_options
            .iter()
            .map(|option| CoreOptionRow {
                key: option.key.clone(),
                label: option.label.clone(),
                values: option.values.clone(),
                value: option.value.clone(),
            })
            .collect();
        // The running core's own input descriptors, when a game is loaded.
        self.model.core_inputs = self
            .session
            .as_ref()
            .map(|session| session.input_descriptors())
            .unwrap_or_default()
            .into_iter()
            .map(|descriptor| InputDescriptorRow {
                port: descriptor.port,
                device: descriptor.device,
                index: descriptor.index,
                id: descriptor.id,
                description: descriptor.description,
            })
            .collect();
        self.rebuild_catalog();
        self.dirty = true;
    }

    /// Pick how the one keyboard is shared, and remember it.
    pub(super) fn set_keyboard_mode(&mut self, mode: KeyboardMode) {
        self.keyboard_mode = mode;
        self.settings.keyboard_mode = mode.key().to_string();
        let _ = self.settings.save(&self.paths.settings_json);
        self.model.keyboard_mode = mode;
        self.dirty = true;
    }

    /// Drop geometry MSAA while a game is running.
    ///
    /// The core image dominates the frame and is nearest-scaled, so 4x MSAA on
    /// the static chrome is wasted GPU fill; it is restored when the game pauses
    /// or exits. The backend setter is a no-op when the count is unchanged, so
    /// this is safe to call every frame.
    pub(super) fn sync_msaa(&mut self) {
        let samples = self.target_msaa_samples();
        if let Some(backend) = self.backend.clone() {
            backend.borrow_mut().set_msaa_samples(samples);
        }
    }

    /// The sample count the current anti-aliasing mode wants on this frame.
    pub(super) fn target_msaa_samples(&self) -> u32 {
        match self.msaa {
            MsaaKind::Off => 1,
            MsaaKind::Two => 2,
            MsaaKind::Four => 4,
            // Auto: 4x while idle, single-sample while a game runs (its live
            // image dominates the frame, so 4x fill is wasted).
            MsaaKind::Auto => {
                if self
                    .session
                    .as_ref()
                    .is_some_and(|session| !session.paused())
                {
                    1
                } else {
                    4
                }
            }
        }
    }
}

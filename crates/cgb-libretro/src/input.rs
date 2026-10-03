//! Input: keyboard bindings and host-provided gamepad state, reduced to the same
//! per-port button bitmask the libretro host expects.
//!
//! Keyboard and gamepad are independent sources that are OR-ed together: a
//! press on either sets the bit, a release only clears it when neither source
//! still holds it. The old Electron front end learned this the hard way — if
//! the two sources overwrite each other, releasing a pad button while a key is
//! held drops the input.
//!
//! `cgb-libretro` reads the mask through [`InputState::mask`]; the app sets it
//! through [`KeyboardBindings::apply`] and [`GamepadSnapshot`].

use std::collections::HashMap;

use crate::{JoypadButton, SystemId};

/// The most controller ports the front end tracks, aligned with libretro's
/// 8-player convention (RetroArch's `MAX_PLAYERS`). A core may support fewer;
/// it reports its own maximum through `RETRO_ENVIRONMENT_GET_INPUT_MAX_USERS`.
pub const MAX_PORTS: usize = 8;

/// A key the keyboard bindings can bind.
///
/// Deliberately not `igui_core::Key`: this crate owns only the keys it can
/// bind, so it stays free of the UI (dependency rule). The app maps the UI's
/// key events into this type before calling [`KeyboardBindings::apply`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// A printable character (`w`, `z`, `1`, …).
    Character(char),
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Enter,
    Space,
    Tab,
}

/// The pressed-button bitmask for both controller ports.
///
/// Keyboard and gamepad are tracked separately and OR-ed by [`mask`], so
/// releasing a pad button while a key is held (or vice versa) does not drop the
/// input.
///
/// [`mask`]: InputState::mask
#[derive(Clone, Copy, Debug, Default)]
pub struct InputState {
    /// Buttons held by the keyboard bindings.
    keyboard: [u16; MAX_PORTS],
    /// Buttons held by a gamepad.
    gamepad: [u16; MAX_PORTS],
    /// Analog sticks: `[port][stick][axis]` in `-32768..=32767`, libretro's
    /// convention (Y positive is down). Only a gamepad produces these.
    analog: [[[i16; 2]; 2]; MAX_PORTS],
}

impl InputState {
    /// A fresh state with nothing pressed.
    pub fn new() -> Self {
        Self::default()
    }

    /// The bitmask for `port` (`0` or `1`), both sources OR-ed.
    pub fn mask(&self, port: usize) -> u16 {
        let keyboard = self.keyboard.get(port).copied().unwrap_or(0);
        let gamepad = self.gamepad.get(port).copied().unwrap_or(0);
        keyboard | gamepad
    }

    /// An analog axis: `stick` 0 = left, 1 = right; `axis` 0 = X, 1 = Y.
    pub fn analog(&self, port: usize, stick: usize, axis: usize) -> i16 {
        self.analog
            .get(port)
            .and_then(|port| port.get(stick))
            .and_then(|stick| stick.get(axis))
            .copied()
            .unwrap_or(0)
    }

    /// Set an analog axis (gamepad only; the keyboard has no analog source).
    pub fn set_analog(&mut self, port: usize, stick: usize, axis: usize, value: i16) {
        if let Some(slot) = self
            .analog
            .get_mut(port)
            .and_then(|port| port.get_mut(stick))
            .and_then(|stick| stick.get_mut(axis))
        {
            *slot = value;
        }
    }

    /// Whether `button` is currently held on `port` by either source.
    pub fn is_down(&self, port: usize, button: JoypadButton) -> bool {
        (self.mask(port) >> button.id()) & 1 != 0
    }

    /// Press or release one button from the **keyboard** on one port.
    pub fn set(&mut self, port: usize, button: JoypadButton, down: bool) {
        set_bit(&mut self.keyboard, port, button, down);
    }

    /// Press or release one button from a **gamepad** on one port.
    pub fn set_gamepad(&mut self, port: usize, button: JoypadButton, down: bool) {
        set_bit(&mut self.gamepad, port, button, down);
    }

    /// Replace a port's whole gamepad button mask at once.
    pub fn set_gamepad_mask(&mut self, port: usize, mask: u16) {
        if let Some(slot) = self.gamepad.get_mut(port) {
            *slot = mask;
        }
    }

    /// Release everything on a port (focus loss, core switch).
    pub fn clear(&mut self, port: usize) {
        if let Some(mask) = self.keyboard.get_mut(port) {
            *mask = 0;
        }
        if let Some(mask) = self.gamepad.get_mut(port) {
            *mask = 0;
        }
        if let Some(analog) = self.analog.get_mut(port) {
            *analog = [[0; 2]; 2];
        }
    }

    /// Release the **gamepad** half of every port (buttons and sticks), leaving
    /// the keyboard half alone.
    ///
    /// A gamepad source calls this at the start of each poll and then writes
    /// every connected pad, so the gamepad half is recomputed from scratch each
    /// frame: a port whose pad disconnected, was unassigned or was moved can
    /// never keep last frame's buttons or a stuck stick. Clearing only the
    /// gamepad half keeps the keyboard and gamepad OR-ed independently.
    pub fn clear_gamepad(&mut self) {
        self.gamepad = [0; MAX_PORTS];
        self.analog = [[[0; 2]; 2]; MAX_PORTS];
    }
}

/// Set or clear one bit in a per-port mask.
fn set_bit(masks: &mut [u16], port: usize, button: JoypadButton, down: bool) {
    let Some(mask) = masks.get_mut(port) else {
        return;
    };
    let bit = 1u16 << button.id();
    if down {
        *mask |= bit;
    } else {
        *mask &= !bit;
    }
}

/// A host-provided gamepad state: per-port button masks and sticks.
///
/// A platform host that owns its own gamepad API (Swift's `GameController`)
/// fills this in and hands it to the app, which applies it to the shared
/// [`InputState`] once per frame. It is the snapshot consumed by the host.
#[derive(Clone, Copy, Debug, Default)]
pub struct GamepadSnapshot {
    /// Buttons held per port, as a libretro joypad bitmask.
    pub buttons: [u16; MAX_PORTS],
    /// Analog sticks: `[port][stick][axis]`, libretro's convention.
    pub analog: [[[i16; 2]; 2]; MAX_PORTS],
    /// Whether a pad is connected on each port (informational).
    pub connected: [bool; MAX_PORTS],
}

impl GamepadSnapshot {
    /// Press or release one button.
    pub fn set_button(&mut self, port: usize, button: JoypadButton, down: bool) {
        let Some(mask) = self.buttons.get_mut(port) else {
            return;
        };
        let bit = 1u16 << button.id();
        if down {
            *mask |= bit;
        } else {
            *mask &= !bit;
        }
    }

    /// Set one analog axis.
    pub fn set_analog(&mut self, port: usize, stick: usize, axis: usize, value: i16) {
        if let Some(slot) = self
            .analog
            .get_mut(port)
            .and_then(|port| port.get_mut(stick))
            .and_then(|stick| stick.get_mut(axis))
        {
            *slot = value;
        }
    }

    /// Mark a port connected or disconnected; disconnecting clears it.
    pub fn connect(&mut self, port: usize, connected: bool) {
        if let Some(slot) = self.connected.get_mut(port) {
            *slot = connected;
        }
        if !connected {
            self.clear(port);
        }
    }

    /// Clear a port's buttons and sticks.
    pub fn clear(&mut self, port: usize) {
        if let Some(mask) = self.buttons.get_mut(port) {
            *mask = 0;
        }
        if let Some(analog) = self.analog.get_mut(port) {
            *analog = [[0; 2]; 2];
        }
    }

    /// Write this snapshot into the gamepad half of `state` (the keyboard half
    /// is left alone, so the two sources still OR).
    pub fn apply(&self, state: &mut InputState) {
        for port in 0..MAX_PORTS {
            state.set_gamepad_mask(port, self.buttons[port]);
            for stick in 0..2 {
                for axis in 0..2 {
                    state.set_analog(port, stick, axis, self.analog[port][stick][axis]);
                }
            }
        }
    }
}

/// How the one keyboard is shared between players.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum KeyboardMode {
    /// Both WASD and the arrow keys drive player one.
    #[default]
    Single,
    /// WASD drives player one, the arrow keys player two.
    TwoPlayer,
}

impl KeyboardMode {
    pub const ALL: [Self; 2] = [Self::Single, Self::TwoPlayer];

    /// The stable settings key.
    pub fn key(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::TwoPlayer => "two_player",
        }
    }

    /// Parse the settings key, defaulting to [`Self::Single`].
    pub fn from_key(key: &str) -> Self {
        match key {
            "two_player" => Self::TwoPlayer,
            _ => Self::Single,
        }
    }

    /// A human label for the settings UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::Single => "单人（WASD + 方向键）",
            Self::TwoPlayer => "双人（WASD / 方向键）",
        }
    }
}

/// A key → button table. More than one key may map to the same button (the
/// default binds both `Z` and `J` to B).
#[derive(Clone, Debug)]
pub struct KeyboardBindings {
    bindings: Vec<(Key, JoypadButton)>,
}

impl KeyboardBindings {
    /// The default layout from the README: arrows/WASD, Z/J = B, X/K = A,
    /// Enter/Space = Start, Tab = Select.
    pub fn default_bindings() -> Self {
        use JoypadButton::*;
        let mut bindings = Vec::new();
        let mut bind = |keys: &[Key], button: JoypadButton| {
            for key in keys {
                bindings.push((*key, button));
            }
        };
        bind(
            &[Key::ArrowUp, Key::Character('w'), Key::Character('W')],
            Up,
        );
        bind(
            &[Key::ArrowDown, Key::Character('s'), Key::Character('S')],
            Down,
        );
        bind(
            &[Key::ArrowLeft, Key::Character('a'), Key::Character('A')],
            Left,
        );
        bind(
            &[Key::ArrowRight, Key::Character('d'), Key::Character('D')],
            Right,
        );
        bind(&[Key::Character('z'), Key::Character('j')], B);
        bind(&[Key::Character('x'), Key::Character('k')], A);
        bind(&[Key::Enter, Key::Space], Start);
        bind(&[Key::Tab], Select);
        Self { bindings }
    }

    /// The default keyboard layout for `system`.
    ///
    /// The base layout is the README's: arrows/WASD, `Z`/`J` = B, `X`/`K` = A,
    /// Enter/Space = Start, Tab = Select. The Sega Mega Drive adds its third
    /// face button (Genesis C) and the 6-button extras, so a Genesis game is
    /// playable without rebinding.
    pub fn default_bindings_for(system: SystemId) -> Self {
        let mut bindings = Self::default_bindings();
        if system == SystemId::Genesis {
            // Genesis Plus GX's libretro ids: B = A, A = B, Y = C, X = X,
            // L = Y, R = Z, R2 = Mode.
            bindings.bind(Key::Character('c'), JoypadButton::Y); // Genesis C
            bindings.bind(Key::Character('v'), JoypadButton::X); // Genesis X
            bindings.bind(Key::Character('b'), JoypadButton::L); // Genesis Y
            bindings.bind(Key::Character('n'), JoypadButton::R); // Genesis Z
            bindings.bind(Key::Character('m'), JoypadButton::R2); // Mode
        } else if system == SystemId::N64 {
            // Mupen64Plus-Next's libretro ids (see its input descriptors):
            // N64 A = JOYPAD_B, N64 B = JOYPAD_Y, Z = L2, R shoulder = R2,
            // L shoulder = Select, and the C buttons = X / A / L / R. The base
            // layout already covers A (`z`/`j`), the D-pad and Start; add the
            // missing buttons. Binding `x` to Y replaces its base A mapping.
            bindings.bind(Key::Character('x'), JoypadButton::Y); // N64 B
            bindings.bind(Key::Character('c'), JoypadButton::L2); // Z trigger
            bindings.bind(Key::Character('v'), JoypadButton::R2); // R shoulder
            bindings.bind(Key::Character('i'), JoypadButton::X); // C-Up
            bindings.bind(Key::Character('j'), JoypadButton::L); // C-Left
            bindings.bind(Key::Character('l'), JoypadButton::R); // C-Right
        } else if system == SystemId::Psp {
            // PPSSPP's libretro ids: Cross = B, Circle = A, Square = Y,
            // Triangle = X, L/R = the shoulders. The base layout already
            // covers Cross (`z`/`j`), Circle (`x`/`k`), the D-pad, Start and
            // Select; add the two remaining face buttons and the shoulders.
            bindings.bind(Key::Character('c'), JoypadButton::Y); // Square
            bindings.bind(Key::Character('v'), JoypadButton::X); // Triangle
            bindings.bind(Key::Character('q'), JoypadButton::L);
            bindings.bind(Key::Character('e'), JoypadButton::R);
        } else if system == SystemId::PlayStation {
            // Beetle PSX's libretro ids: Cross = B, Circle = A, Square = Y,
            // Triangle = X, L1/R1 = L/R, L2/R2 = L2/R2. The base covers Cross
            // (`z`/`j`), Circle (`x`/`k`), the D-pad, Start and Select.
            bindings.bind(Key::Character('c'), JoypadButton::Y); // Square
            bindings.bind(Key::Character('v'), JoypadButton::X); // Triangle
            bindings.bind(Key::Character('q'), JoypadButton::L); // L1
            bindings.bind(Key::Character('e'), JoypadButton::R); // R1
            bindings.bind(Key::Character('r'), JoypadButton::L2); // L2
            bindings.bind(Key::Character('f'), JoypadButton::R2); // R2
        } else if system == SystemId::Snes {
            // Snes9x's libretro ids: B = B, A = A, Y = Y, X = X, L/R = the
            // shoulders. The base layout already covers B (`z`/`j`), A
            // (`x`/`k`), the D-pad, Start and Select; add the rest.
            bindings.bind(Key::Character('c'), JoypadButton::Y); // SNES Y
            bindings.bind(Key::Character('v'), JoypadButton::X); // SNES X
            bindings.bind(Key::Character('q'), JoypadButton::L);
            bindings.bind(Key::Character('e'), JoypadButton::R);
        } else if system == SystemId::J2me {
            // FreeJ2ME's libretro buttons, straight from its input descriptors:
            // Y = "OK/Fire", SELECT = "Left Softkey", START = "Right Softkey",
            // A/B = Num 9/7, L/R = Num 1/3, X = Num 0, L3 = Num 5, R3 = CLR.
            //
            // The game's own confirm is **OK/Fire**, not SELECT: SELECT/START
            // are the LCDUI softkeys. A Canvas game that checks `FIRE` (or
            // `KEY_NUM5`) therefore ignores the Select binding, which is why
            // "some games don't respond to select". Put confirm on Enter/Space
            // (it was Start) and the softkeys on q/e, so a keyboard has an
            // obvious OK key as well as both softkeys.
            bindings.bind(Key::Enter, JoypadButton::Y); // OK/Fire
            bindings.bind(Key::Space, JoypadButton::Y); // OK/Fire
            bindings.bind(Key::Character('q'), JoypadButton::Select); // left softkey
            bindings.bind(Key::Character('e'), JoypadButton::Start); // right softkey
            bindings.bind(Key::Character('1'), JoypadButton::L); // Num 1
            bindings.bind(Key::Character('3'), JoypadButton::R); // Num 3
            bindings.bind(Key::Character('5'), JoypadButton::L3); // Num 5
            bindings.bind(Key::Character('7'), JoypadButton::B); // Num 7
            bindings.bind(Key::Character('9'), JoypadButton::A); // Num 9
            bindings.bind(Key::Character('0'), JoypadButton::R3); // CLR
        }
        bindings
    }

    /// The two-player table for `system`'s **player one**: the single-player
    /// layout without the arrow keys (which move to player two).
    pub fn default_p1_for(system: SystemId) -> Self {
        Self::default_bindings_for(system).without_arrows()
    }

    /// The default **player two** table: the arrow keys drive the D-pad, and
    /// nearby punctuation drives the face buttons.
    pub fn default_p2_for(_system: SystemId) -> Self {
        use JoypadButton::*;
        let mut bindings = Vec::new();
        let mut bind = |keys: &[Key], button: JoypadButton| {
            for key in keys {
                bindings.push((*key, button));
            }
        };
        bind(&[Key::ArrowUp], Up);
        bind(&[Key::ArrowDown], Down);
        bind(&[Key::ArrowLeft], Left);
        bind(&[Key::ArrowRight], Right);
        bind(&[Key::Character(','), Key::Character('<')], B);
        bind(&[Key::Character('.'), Key::Character('>')], A);
        bind(&[Key::Character(';'), Key::Character(':')], Start);
        bind(&[Key::Character('\''), Key::Character('"')], Select);
        Self { bindings }
    }

    /// Drop every arrow-key binding (used to split the single-player layout).
    fn without_arrows(mut self) -> Self {
        self.bindings.retain(|(key, _)| {
            !matches!(
                key,
                Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight
            )
        });
        self
    }
    /// Rebind a key to a button, replacing any existing mapping for that key.
    pub fn bind(&mut self, key: Key, button: JoypadButton) {
        self.bindings.retain(|(existing, _)| *existing != key);
        self.bindings.push((key, button));
    }

    /// The button a key is bound to, if any. Used by the bindings UI.
    pub fn button_for(&self, key: Key) -> Option<JoypadButton> {
        self.bindings
            .iter()
            .find(|(existing, _)| *existing == key)
            .map(|(_, button)| *button)
    }

    /// The whole key → button table, for the settings page.
    pub fn entries(&self) -> &[(Key, JoypadButton)] {
        &self.bindings
    }

    /// Apply a key event to `state` on `port`.
    pub fn apply(&self, key: Key, down: bool, state: &mut InputState, port: usize) {
        for (bound, button) in &self.bindings {
            if *bound == key {
                state.set(port, *button, down);
            }
        }
    }
}

impl Default for KeyboardBindings {
    fn default() -> Self {
        Self::default_bindings()
    }
}

/// A small helper for the bindings UI: every button with its default key, in
/// listing order.
pub fn default_key_hints() -> HashMap<JoypadButton, Key> {
    let bindings = KeyboardBindings::default_bindings();
    let mut hints = HashMap::new();
    for button in JoypadButton::ALL {
        if let Some((key, _)) = bindings
            .bindings
            .iter()
            .find(|(_, candidate)| *candidate == button)
        {
            hints.insert(button, *key);
        }
    }
    hints
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_sets_and_a_release_clears() {
        let mut state = InputState::new();
        let bindings = KeyboardBindings::default_bindings();
        bindings.apply(Key::Character('z'), true, &mut state, 0);
        assert!(state.is_down(0, JoypadButton::B));
        bindings.apply(Key::Character('z'), false, &mut state, 0);
        assert!(!state.is_down(0, JoypadButton::B));
    }

    #[test]
    fn the_two_aliases_both_reach_b() {
        let mut state = InputState::new();
        let bindings = KeyboardBindings::default_bindings();
        bindings.apply(Key::Character('j'), true, &mut state, 0);
        assert!(state.is_down(0, JoypadButton::B));
    }

    #[test]
    fn port_one_is_separate_from_port_zero() {
        let mut state = InputState::new();
        let bindings = KeyboardBindings::default_bindings();
        bindings.apply(Key::ArrowUp, true, &mut state, 1);
        assert!(!state.is_down(0, JoypadButton::Up));
        assert!(state.is_down(1, JoypadButton::Up));
    }

    #[test]
    fn rebinding_replaces_the_old_key() {
        let mut bindings = KeyboardBindings::default_bindings();
        bindings.bind(Key::Character('q'), JoypadButton::A);
        assert_eq!(
            bindings.button_for(Key::Character('q')),
            Some(JoypadButton::A)
        );
        // The old 'x' mapping is untouched; 'q' is simply added.
        assert_eq!(
            bindings.button_for(Key::Character('x')),
            Some(JoypadButton::A)
        );
    }

    #[test]
    fn only_the_genesis_layout_reaches_the_third_face_button() {
        // Genesis Plus GX exposes Genesis C on libretro Y; `c` reaches it.
        let mut genesis = InputState::new();
        KeyboardBindings::default_bindings_for(SystemId::Genesis).apply(
            Key::Character('c'),
            true,
            &mut genesis,
            0,
        );
        assert!(genesis.is_down(0, JoypadButton::Y));
        // Other consoles keep the plain two-face-button layout.
        let mut nes = InputState::new();
        KeyboardBindings::default_bindings_for(SystemId::Nes).apply(
            Key::Character('c'),
            true,
            &mut nes,
            0,
        );
        assert!(!nes.is_down(0, JoypadButton::Y));
    }

    #[test]
    fn the_n64_layout_adds_the_z_trigger_and_c_buttons() {
        let bindings = KeyboardBindings::default_bindings_for(SystemId::N64);
        let mut state = InputState::new();
        for (key, button) in [
            ('x', JoypadButton::Y),  // N64 B
            ('c', JoypadButton::L2), // Z trigger
            ('v', JoypadButton::R2), // R shoulder
            ('i', JoypadButton::X),  // C-Up
            ('j', JoypadButton::L),  // C-Left
            ('l', JoypadButton::R),  // C-Right
            ('z', JoypadButton::B),  // N64 A
        ] {
            bindings.apply(Key::Character(key), true, &mut state, 0);
            assert!(state.is_down(0, button), "{key:?} -> {button:?}");
        }
        // A plain console keeps `x` on A and leaves the C buttons alone.
        let mut nes = InputState::new();
        KeyboardBindings::default_bindings_for(SystemId::Nes).apply(
            Key::Character('c'),
            true,
            &mut nes,
            0,
        );
        assert!(!nes.is_down(0, JoypadButton::L2));
    }

    #[test]
    fn the_psp_layout_adds_square_triangle_and_shoulders() {
        let bindings = KeyboardBindings::default_bindings_for(SystemId::Psp);
        let mut state = InputState::new();
        for (key, button) in [
            ('z', JoypadButton::B), // Cross
            ('x', JoypadButton::A), // Circle
            ('c', JoypadButton::Y), // Square
            ('v', JoypadButton::X), // Triangle
            ('q', JoypadButton::L),
            ('e', JoypadButton::R),
        ] {
            bindings.apply(Key::Character(key), true, &mut state, 0);
            assert!(state.is_down(0, button), "{key:?} -> {button:?}");
        }
    }

    #[test]
    fn the_ps1_layout_adds_square_triangle_and_shoulders() {
        let bindings = KeyboardBindings::default_bindings_for(SystemId::PlayStation);
        let mut state = InputState::new();
        for (key, button) in [
            ('z', JoypadButton::B),  // Cross
            ('x', JoypadButton::A),  // Circle
            ('c', JoypadButton::Y),  // Square
            ('v', JoypadButton::X),  // Triangle
            ('q', JoypadButton::L),  // L1
            ('e', JoypadButton::R),  // R1
            ('r', JoypadButton::L2), // L2
            ('f', JoypadButton::R2), // R2
        ] {
            bindings.apply(Key::Character(key), true, &mut state, 0);
            assert!(state.is_down(0, button), "{key:?} -> {button:?}");
        }
    }

    #[test]
    fn the_snes_layout_adds_x_y_and_shoulders() {
        let bindings = KeyboardBindings::default_bindings_for(SystemId::Snes);
        let mut state = InputState::new();
        for (key, button) in [
            ('z', JoypadButton::B), // SNES B
            ('x', JoypadButton::A), // SNES A
            ('c', JoypadButton::Y), // SNES Y
            ('v', JoypadButton::X), // SNES X
            ('q', JoypadButton::L),
            ('e', JoypadButton::R),
        ] {
            bindings.apply(Key::Character(key), true, &mut state, 0);
            assert!(state.is_down(0, button), "{key:?} -> {button:?}");
        }
    }

    #[test]
    fn the_j2me_layout_puts_confirm_on_enter() {
        let bindings = KeyboardBindings::default_bindings_for(SystemId::J2me);
        let mut state = InputState::new();
        // Confirm is FreeJ2ME's "OK/Fire" (libretro Y), not Start; Select is
        // the left softkey, so Enter must not land on Start.
        bindings.apply(Key::Enter, true, &mut state, 0);
        assert!(state.is_down(0, JoypadButton::Y));
        assert!(!state.is_down(0, JoypadButton::Start));
        bindings.apply(Key::Space, true, &mut state, 0);
        assert!(state.is_down(0, JoypadButton::Y));
        for (key, button) in [
            ('q', JoypadButton::Select), // left softkey
            ('e', JoypadButton::Start),  // right softkey
            ('1', JoypadButton::L),
            ('3', JoypadButton::R),
            ('5', JoypadButton::L3),
            ('7', JoypadButton::B),
            ('9', JoypadButton::A),
            ('0', JoypadButton::R3),
        ] {
            bindings.apply(Key::Character(key), true, &mut state, 0);
            assert!(state.is_down(0, button), "{key:?} -> {button:?}");
        }
    }

    #[test]
    fn keyboard_and_gamepad_are_or_ed() {
        let mut state = InputState::new();
        let bindings = KeyboardBindings::default_bindings();
        bindings.apply(Key::ArrowUp, true, &mut state, 0);
        state.set_gamepad(0, JoypadButton::Up, true);
        // Releasing the pad button must not clear the held key.
        state.set_gamepad(0, JoypadButton::Up, false);
        assert!(state.is_down(0, JoypadButton::Up));
        // And releasing the key must not clear a held pad button.
        state.set(0, JoypadButton::A, true);
        state.set_gamepad(0, JoypadButton::A, true);
        state.set(0, JoypadButton::A, false);
        assert!(state.is_down(0, JoypadButton::A));
    }

    #[test]
    fn two_player_p1_has_no_arrows_and_p2_does() {
        let p1 = KeyboardBindings::default_p1_for(SystemId::Nes);
        assert_eq!(p1.button_for(Key::Character('w')), Some(JoypadButton::Up));
        assert_eq!(p1.button_for(Key::ArrowUp), None, "arrows belong to P2");

        let p2 = KeyboardBindings::default_p2_for(SystemId::Nes);
        assert_eq!(p2.button_for(Key::ArrowUp), Some(JoypadButton::Up));
        assert_eq!(p2.button_for(Key::Character('.')), Some(JoypadButton::A));
    }

    #[test]
    fn keyboard_mode_keys_round_trip() {
        for mode in KeyboardMode::ALL {
            assert_eq!(KeyboardMode::from_key(mode.key()), mode);
        }
        assert_eq!(KeyboardMode::from_key("nonsense"), KeyboardMode::Single);
    }

    #[test]
    fn all_ports_are_independent_and_out_of_range_is_ignored() {
        let mut state = InputState::new();
        let bindings = KeyboardBindings::default_bindings();
        bindings.apply(Key::ArrowUp, true, &mut state, 0);
        assert!(state.is_down(0, JoypadButton::Up));
        assert!(!state.is_down(1, JoypadButton::Up));

        state.set_gamepad_mask(7, 1 << JoypadButton::A.id());
        assert!(state.is_down(7, JoypadButton::A));
        // Out-of-range ports are ignored, not a panic.
        state.set_gamepad_mask(MAX_PORTS, 0xffff);
        assert_eq!(state.mask(MAX_PORTS), 0);
    }

    #[test]
    fn clear_gamepad_drops_pads_but_keeps_keys() {
        let mut state = InputState::new();
        state.set(0, JoypadButton::A, true); // keyboard
        state.set_gamepad_mask(0, 1 << JoypadButton::Left.id());
        state.set_analog(0, 0, 0, 12345);

        state.clear_gamepad();

        // The pad half is gone, including any stuck direction or stick.
        assert!(!state.is_down(0, JoypadButton::Left));
        assert_eq!(state.analog(0, 0, 0), 0);
        // ... but the keyboard is untouched, so a held key still registers.
        assert!(state.is_down(0, JoypadButton::A));
    }
}

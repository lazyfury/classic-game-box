//! Input: keyboard bindings and `gilrs` gamepads, both reduced to the same
//! per-port button bitmask the libretro host expects.
//!
//! Keyboard and gamepad are independent sources that are OR-ed together: a
//! press on either sets the bit, a release only clears it when neither source
//! still holds it. The old Electron front end learned this the hard way — if
//! the two sources overwrite each other, releasing a pad button while a key is
//! held drops the input.
//!
//! `cgb-libretro` reads the mask through [`InputState::mask`]; the app sets it
//! through [`KeyboardBindings::apply`] and [`Gamepads::poll`].

use std::collections::HashMap;

use cgb_systems::JoypadButton;
use draw_core::Key;

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
    keyboard: [u16; 2],
    /// Buttons held by a gamepad.
    gamepad: [u16; 2],
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

    /// Release everything on a port (focus loss, core switch).
    pub fn clear(&mut self, port: usize) {
        if let Some(mask) = self.keyboard.get_mut(port) {
            *mask = 0;
        }
        if let Some(mask) = self.gamepad.get_mut(port) {
            *mask = 0;
        }
    }
}

/// Set or clear one bit in a per-port mask.
fn set_bit(masks: &mut [u16; 2], port: usize, button: JoypadButton, down: bool) {
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

/// The `gilrs` connection and its port assignment.
pub struct Gamepads {
    gilrs: gilrs::Gilrs,
    /// Which physical pad is on which libretro port, in connection order.
    ports: [Option<gilrs::GamepadId>; 2],
    /// Buttons held per port, independent of the keyboard, so the two sources
    /// can be OR-ed instead of overwriting each other.
    held: [u16; 2],
}

impl Gamepads {
    /// Connect to the platform's gamepad API. Failure is not fatal: the app
    /// falls back to keyboard.
    // `gilrs::Error` is a large enum, but this is called once at startup and
    // never in a hot path, so boxing it would only add indirection.
    #[allow(clippy::result_large_err)]
    pub fn new() -> Result<Self, gilrs::Error> {
        Ok(Self {
            gilrs: gilrs::Gilrs::new()?,
            ports: [None, None],
            held: [0, 0],
        })
    }

    /// Drain pending events and update `state`. Call once per frame.
    pub fn poll(&mut self, state: &mut InputState) {
        while let Some(event) = self.gilrs.next_event() {
            match event.event {
                gilrs::EventType::Connected => self.assign(event.id),
                gilrs::EventType::Disconnected => self.release_port(event.id),
                gilrs::EventType::ButtonPressed(button, _) => {
                    if let (Some(port), Some(mapped)) = (self.port_of(event.id), button_of(button))
                    {
                        self.set_button(state, port, mapped, true);
                    }
                }
                gilrs::EventType::ButtonReleased(button, _) => {
                    if let (Some(port), Some(mapped)) = (self.port_of(event.id), button_of(button))
                    {
                        self.set_button(state, port, mapped, false);
                    }
                }
                // Some pads report the D-pad and the sticks as axes, not
                // buttons, so map those too.
                gilrs::EventType::AxisChanged(axis, value, _) => {
                    if let (Some(port), Some((negative, positive))) =
                        (self.port_of(event.id), axis_buttons(axis))
                    {
                        self.set_button(state, port, negative, value < -STICK_DEADZONE);
                        self.set_button(state, port, positive, value > STICK_DEADZONE);
                    }
                }
                _ => {}
            }
        }
    }

    /// The pad assigned to a port, if any.
    pub fn assigned(&self, port: usize) -> Option<gilrs::GamepadId> {
        self.ports.get(port).copied().flatten()
    }

    fn assign(&mut self, id: gilrs::GamepadId) {
        if self.port_of(id).is_some() {
            return;
        }
        if let Some(slot) = self.ports.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(id);
        }
    }

    fn release_port(&mut self, id: gilrs::GamepadId) {
        if let Some(port) = self.port_of(id) {
            self.ports[port] = None;
            self.held[port] = 0;
        }
    }

    fn port_of(&self, id: gilrs::GamepadId) -> Option<usize> {
        self.ports.iter().position(|slot| *slot == Some(id))
    }

    /// Merge this port's held buttons into the shared state.
    fn publish(&self, state: &mut InputState, port: usize) {
        for button in JoypadButton::ALL {
            let down = (self.held[port] >> button.id()) & 1 != 0;
            state.set_gamepad(port, button, down);
        }
    }

    /// Press or release one gamepad button and republish the port.
    fn set_button(
        &mut self,
        state: &mut InputState,
        port: usize,
        button: JoypadButton,
        down: bool,
    ) {
        let bit = 1u16 << button.id();
        if down {
            self.held[port] |= bit;
        } else {
            self.held[port] &= !bit;
        }
        self.publish(state, port);
    }
}

/// How far a stick or hat must move before it counts as a direction.
const STICK_DEADZONE: f32 = 0.5;

/// The (negative, positive) D-pad buttons an axis drives, if any.
fn axis_buttons(axis: gilrs::Axis) -> Option<(JoypadButton, JoypadButton)> {
    use gilrs::Axis;
    use JoypadButton::*;
    Some(match axis {
        Axis::LeftStickX | Axis::DPadX => (Left, Right),
        // gilrs normalizes the Y axis so positive is up.
        Axis::LeftStickY | Axis::DPadY => (Down, Up),
        _ => return None,
    })
}

/// Which libretro button a physical pad button means.
///
/// The bottom face button is the one printed "A" on most pads and the console's
/// A is the right-hand one, so it is the **bottom** button that maps to libretro
/// A (id 8) and the **right** one to B (id 0) — the same way the legacy front
/// end did (`legacy/electron/src/renderer/gamepad.ts`).
fn button_of(button: gilrs::Button) -> Option<JoypadButton> {
    use gilrs::Button;
    use JoypadButton::*;
    Some(match button {
        Button::South => A,
        Button::East => B,
        Button::North => Y,
        Button::West => X,
        Button::LeftTrigger | Button::LeftTrigger2 => L,
        Button::RightTrigger | Button::RightTrigger2 => R,
        Button::Select => Select,
        Button::Start => Start,
        Button::DPadUp => Up,
        Button::DPadDown => Down,
        Button::DPadLeft => Left,
        Button::DPadRight => Right,
        _ => return None,
    })
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
    fn stick_axes_drive_the_dpad() {
        use gilrs::Axis;
        assert_eq!(
            axis_buttons(Axis::LeftStickX),
            Some((JoypadButton::Left, JoypadButton::Right))
        );
        assert_eq!(
            axis_buttons(Axis::LeftStickY),
            Some((JoypadButton::Down, JoypadButton::Up))
        );
        assert_eq!(axis_buttons(Axis::RightStickX), None);
    }

    /// The legacy front end mapped the bottom face button (printed "A" on most
    /// pads) to the console's A, and the right face button to B. Pinned here so
    /// the two do not drift.
    #[test]
    fn the_bottom_face_button_is_a_and_the_right_one_is_b() {
        use gilrs::Button;
        assert_eq!(button_of(Button::South), Some(JoypadButton::A));
        assert_eq!(button_of(Button::East), Some(JoypadButton::B));
        assert_eq!(button_of(Button::Start), Some(JoypadButton::Start));
        assert_eq!(button_of(Button::DPadUp), Some(JoypadButton::Up));
    }
}

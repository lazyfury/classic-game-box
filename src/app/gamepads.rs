//! The default gamepad backend: `gilrs` (Rust).
//!
//! `App::init` uses it whenever the platform host does not publish its own
//! [`GamepadSource`]. The macOS Swift host always does (Apple's `GameController`
//! maps Bluetooth Xbox pads correctly, where `gilrs` mislabels them), so in the
//! product this is the Windows/Linux path — but it also works on macOS if the
//! host is absent.
//!
//! `gilrs` supports more than four pads (Windows' XInput alone caps at four);
//! it assigns the first free port on connect and reads each pad's current state
//! every frame.

use std::collections::HashMap;

use cgb_libretro::{InputState, JoypadButton, MAX_PORTS};
use gilrs::{Axis, Button, EventType, GamepadId, Gilrs};

use crate::host::GamepadSource;

/// gilrs button → libretro joypad button.
///
/// gilrs's `LeftTrigger` is the bumper (LB) and `LeftTrigger2` the analog
/// trigger (LT); likewise on the right.
const BUTTONS: &[(Button, JoypadButton)] = &[
    (Button::South, JoypadButton::B),
    (Button::East, JoypadButton::A),
    (Button::West, JoypadButton::Y),
    (Button::North, JoypadButton::X),
    (Button::LeftTrigger, JoypadButton::L),
    (Button::RightTrigger, JoypadButton::R),
    (Button::LeftTrigger2, JoypadButton::L2),
    (Button::RightTrigger2, JoypadButton::R2),
    (Button::LeftThumb, JoypadButton::L3),
    (Button::RightThumb, JoypadButton::R3),
    (Button::Start, JoypadButton::Start),
    (Button::Select, JoypadButton::Select),
    (Button::DPadUp, JoypadButton::Up),
    (Button::DPadDown, JoypadButton::Down),
    (Button::DPadLeft, JoypadButton::Left),
    (Button::DPadRight, JoypadButton::Right),
];

/// A `gilrs`-backed gamepad source.
pub struct GilrsGamepads {
    gilrs: Gilrs,
    /// Connected pads, in the port each drives.
    ports: HashMap<GamepadId, usize>,
}

impl GilrsGamepads {
    /// Open the backend, or `None` when it cannot start.
    pub fn new() -> Option<Self> {
        Gilrs::new().ok().map(|gilrs| Self {
            gilrs,
            ports: HashMap::new(),
        })
    }

    /// The first port no pad has claimed, if any.
    fn free_port(&self) -> Option<usize> {
        (0..MAX_PORTS).find(|port| !self.ports.values().any(|assigned| assigned == port))
    }
}

impl GamepadSource for GilrsGamepads {
    fn poll(&mut self, state: &mut InputState) {
        // Connection changes first, so a fresh pad is read in this same frame.
        while let Some(event) = self.gilrs.next_event() {
            match event.event {
                EventType::Connected => {
                    if !self.ports.contains_key(&event.id) {
                        if let Some(port) = self.free_port() {
                            self.ports.insert(event.id, port);
                        }
                    }
                }
                EventType::Disconnected => {
                    if let Some(port) = self.ports.remove(&event.id) {
                        state.clear(port);
                    }
                }
                _ => {}
            }
        }

        // Then read the current value of every assigned pad (gilrs tracks the
        // state, so there is no per-frame event bookkeeping).
        for (id, &port) in &self.ports {
            let gamepad = self.gilrs.gamepad(*id);
            let mut mask = 0u16;
            for (button, joypad) in BUTTONS {
                if gamepad.is_pressed(*button) {
                    mask |= 1 << joypad.id();
                }
            }
            state.set_gamepad_mask(port, mask);
            let sticks = [
                (Axis::LeftStickX, Axis::LeftStickY),
                (Axis::RightStickX, Axis::RightStickY),
            ];
            for (stick, (x_axis, y_axis)) in sticks.into_iter().enumerate() {
                state.set_analog(port, stick, 0, to_i16(gamepad.value(x_axis)));
                // gilrs Y is positive up; libretro wants positive down.
                state.set_analog(port, stick, 1, to_i16(-gamepad.value(y_axis)));
            }
        }
    }
}

/// A `-1.0..=1.0` axis as libretro's `i16`.
fn to_i16(value: f32) -> i16 {
    (value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

#[cfg(test)]
mod tests {
    use super::to_i16;

    #[test]
    fn axes_map_to_i16_and_clamp() {
        assert_eq!(to_i16(0.0), 0);
        assert_eq!(to_i16(1.0), i16::MAX);
        assert_eq!(to_i16(-1.0), -i16::MAX);
        assert_eq!(to_i16(2.0), i16::MAX, "clamped high");
        assert_eq!(to_i16(-2.0), -i16::MAX, "clamped low");
    }
}

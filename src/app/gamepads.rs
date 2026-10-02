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
use std::time::{Duration, Instant};

use cgb_libretro::{InputState, JoypadButton, MAX_PORTS};
use gilrs::{Axis, Button, EventType, GamepadId, Gilrs};

use crate::host::{GamepadDevice, GamepadSource};

/// How long the main pad must hold Select to request a full reset.
const RESET_HOLD: Duration = Duration::from_secs(1);

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
    /// A port waiting for the next pad to press a button ("press to claim").
    claim: Option<usize>,
    /// When the main pad's Select started being held, for the reset long-press.
    select_since: Option<Instant>,
    /// Whether the current Select hold already fired a reset request.
    reset_latched: bool,
    /// A pending reset request, taken by the app.
    reset_requested: bool,
}

impl GilrsGamepads {
    /// Open the backend, or `None` when it cannot start.
    pub fn new() -> Option<Self> {
        Gilrs::new().ok().map(|gilrs| Self {
            gilrs,
            ports: HashMap::new(),
            claim: None,
            select_since: None,
            reset_latched: false,
            reset_requested: false,
        })
    }

    /// The first port no pad has claimed, if any.
    fn free_port(&self) -> Option<usize> {
        (0..MAX_PORTS).find(|port| !self.ports.values().any(|assigned| assigned == port))
    }

    /// A long Select press on the main pad (port 0) requests a full reset.
    fn detect_reset(&mut self, now: Instant) {
        let main = self
            .ports
            .iter()
            .find(|(_, port)| **port == 0)
            .map(|(id, _)| *id);
        let held = main.is_some_and(|id| self.gilrs.gamepad(id).is_pressed(Button::Select));
        if !held {
            self.select_since = None;
            self.reset_latched = false;
            return;
        }
        let since = *self.select_since.get_or_insert(now);
        if !self.reset_latched && now.duration_since(since) >= RESET_HOLD {
            self.reset_requested = true;
            self.reset_latched = true;
        }
    }
}

impl GamepadSource for GilrsGamepads {
    fn poll(&mut self, state: &mut InputState) {
        // Connection changes first, so a fresh pad is read in this same frame.
        while let Some(event) = self.gilrs.next_event() {
            match event.event {
                EventType::Connected => {
                    // While a claim is waiting a fresh pad takes no port: the
                    // user claims it explicitly by pressing Start.
                    if self.claim.is_none() && !self.ports.contains_key(&event.id) {
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
                // Only an unassigned pad claims: an assigned pad ignores Start,
                // so a lone pad cannot hop between ports.
                EventType::ButtonPressed(Button::Start, _)
                    if !self.ports.contains_key(&event.id) =>
                {
                    if let Some(port) = self.claim.take() {
                        self.ports.retain(|_, assigned| *assigned != port);
                        self.ports.insert(event.id, port);
                    }
                }
                _ => {}
            }
        }

        self.detect_reset(Instant::now());

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

    fn devices(&self) -> Vec<GamepadDevice> {
        self.gilrs
            .gamepads()
            .map(|(id, gamepad)| GamepadDevice {
                id: usize::from(id).to_string(),
                name: gamepad.name().to_string(),
                port: self.ports.get(&id).copied(),
            })
            .collect()
    }

    fn assign(&mut self, id: &str, port: Option<usize>) {
        let Some(target) = self
            .gilrs
            .gamepads()
            .find(|(gamepad_id, _)| usize::from(*gamepad_id).to_string() == id)
            .map(|(gamepad_id, _)| gamepad_id)
        else {
            return;
        };
        // One pad per port: bump whatever was there off it.
        if let Some(port) = port {
            self.ports.retain(|_, assigned| *assigned != port);
            self.ports.insert(target, port);
        } else {
            self.ports.remove(&target);
        }
    }

    fn claim(&mut self, port: Option<usize>) {
        self.claim = port;
    }

    fn take_reset_request(&mut self) -> bool {
        std::mem::take(&mut self.reset_requested)
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

//! Joypad ids, shared by the input layer and the libretro host.
//!
//! These mirror `RETRO_DEVICE_ID_JOYPAD_*` from `libretro.h`. They live here,
//! in the pure domain crate, so `cgb-input` never has to depend on the core
//! host and `cgb-libretro` never has to depend on the input layer — both just
//! agree on these numbers. The emulator adapter is the only place that turns
//! them into a `retro_input_state` answer.

/// Libretro's joypad device class (`RETRO_DEVICE_JOYPAD`).
pub const RETRO_DEVICE_JOYPAD: u32 = 1;

/// Libretro's "query all buttons at once" id (`RETRO_DEVICE_ID_JOYPAD_MASK`).
pub const RETRO_DEVICE_ID_JOYPAD_MASK: u32 = 256;

/// A digital button on a libretro joypad, in `RETRO_DEVICE_ID_JOYPAD_*` order.
///
/// The order matters: it is the id the core sees. Named after the hardware
/// where the names are unambiguous, and by libretro's letters otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum JoypadButton {
    B,
    Y,
    Select,
    Start,
    Up,
    Down,
    Left,
    Right,
    A,
    X,
    L,
    R,
}

impl JoypadButton {
    /// Every button, in id order.
    pub const ALL: [JoypadButton; 12] = [
        JoypadButton::B,
        JoypadButton::Y,
        JoypadButton::Select,
        JoypadButton::Start,
        JoypadButton::Up,
        JoypadButton::Down,
        JoypadButton::Left,
        JoypadButton::Right,
        JoypadButton::A,
        JoypadButton::X,
        JoypadButton::L,
        JoypadButton::R,
    ];

    /// The libretro id this button answers to.
    pub fn id(self) -> u32 {
        match self {
            JoypadButton::B => 0,
            JoypadButton::Y => 1,
            JoypadButton::Select => 2,
            JoypadButton::Start => 3,
            JoypadButton::Up => 4,
            JoypadButton::Down => 5,
            JoypadButton::Left => 6,
            JoypadButton::Right => 7,
            JoypadButton::A => 8,
            JoypadButton::X => 9,
            JoypadButton::L => 10,
            JoypadButton::R => 11,
        }
    }

    /// What the bindings UI calls it.
    pub fn label(self) -> &'static str {
        match self {
            JoypadButton::B => "B",
            JoypadButton::Y => "Y",
            JoypadButton::Select => "Select",
            JoypadButton::Start => "Start",
            JoypadButton::Up => "Up",
            JoypadButton::Down => "Down",
            JoypadButton::Left => "Left",
            JoypadButton::Right => "Right",
            JoypadButton::A => "A",
            JoypadButton::X => "X",
            JoypadButton::L => "L",
            JoypadButton::R => "R",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_match_libretro_and_are_unique() {
        let mut seen = [false; 12];
        for button in JoypadButton::ALL {
            let id = button.id() as usize;
            assert!(id < 12);
            assert!(!seen[id], "duplicate id for {button:?}");
            seen[id] = true;
        }
    }

    #[test]
    fn the_nintendo_faces_are_where_the_nes_expects_them() {
        // The NES core only reads B and A; they must be ids 0 and 8.
        assert_eq!(JoypadButton::B.id(), 0);
        assert_eq!(JoypadButton::A.id(), 8);
    }
}

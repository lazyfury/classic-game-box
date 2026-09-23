//! Joypad ids, shared by the input layer and the libretro host.
//!
//! These mirror `RETRO_DEVICE_ID_JOYPAD_*` from `libretro.h`. They live here,
//! in the pure domain crate, so `cgb-input` never has to depend on the core
//! host and `cgb-libretro` never has to depend on the input layer — both just
//! agree on these numbers. The emulator adapter is the only place that turns
//! them into a `retro_input_state` answer.

/// Libretro's joypad device class (`RETRO_DEVICE_JOYPAD`).
pub const RETRO_DEVICE_JOYPAD: u32 = 1;

/// Libretro's analog device class (`RETRO_DEVICE_ANALOG`).
pub const RETRO_DEVICE_ANALOG: u32 = 5;

/// Analog stick index: the left stick.
pub const RETRO_DEVICE_INDEX_ANALOG_LEFT: u32 = 0;
/// Analog stick index: the right stick.
pub const RETRO_DEVICE_INDEX_ANALOG_RIGHT: u32 = 1;

/// Analog axis id: the X axis (also the left/right axis of a stick).
pub const RETRO_DEVICE_ID_ANALOG_X: u32 = 0;
/// Analog axis id: the Y axis (also the up/down axis of a stick).
pub const RETRO_DEVICE_ID_ANALOG_Y: u32 = 1;

/// Device-capability bits for `GET_INPUT_DEVICE_CAPABILITIES`
/// (`RETRO_DEVICE_MASK` of the device class).
pub const RETRO_DEVICE_JOYPAD_BIT: u32 = 1 << RETRO_DEVICE_JOYPAD;
pub const RETRO_DEVICE_ANALOG_BIT: u32 = 1 << RETRO_DEVICE_ANALOG;

/// Libretro's "query all buttons at once" id (`RETRO_DEVICE_ID_JOYPAD_MASK`).
pub const RETRO_DEVICE_ID_JOYPAD_MASK: u32 = 256;

/// A digital button on a libretro joypad, in `RETRO_DEVICE_ID_JOYPAD_*` order.
///
/// The order matters: it is the id the core sees. Named after the hardware
/// where the names are unambiguous, and by libretro's letters otherwise. The
/// full sixteen ids are covered, so a core with shoulder/trigger buttons
/// (mGBA's L/R, an arcade stick's extra buttons) is described without a special
/// case.
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
    L2,
    R2,
    L3,
    R3,
}

impl JoypadButton {
    /// Every button, in id order.
    pub const ALL: [JoypadButton; 16] = [
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
        JoypadButton::L2,
        JoypadButton::R2,
        JoypadButton::L3,
        JoypadButton::R3,
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
            JoypadButton::L2 => 12,
            JoypadButton::R2 => 13,
            JoypadButton::L3 => 14,
            JoypadButton::R3 => 15,
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
            JoypadButton::L2 => "L2",
            JoypadButton::R2 => "R2",
            JoypadButton::L3 => "L3",
            JoypadButton::R3 => "R3",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_match_libretro_and_are_unique() {
        let mut seen = [false; 16];
        for button in JoypadButton::ALL {
            let id = button.id() as usize;
            assert!(id < 16);
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

    #[test]
    fn device_capability_bits_match_the_device_classes() {
        assert_eq!(RETRO_DEVICE_JOYPAD_BIT, 1 << 1);
        assert_eq!(RETRO_DEVICE_ANALOG_BIT, 1 << 5);
    }
}

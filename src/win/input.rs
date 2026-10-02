//! Native event translation: Win32 messages (delivered through the C ABI) →
//! `igui_core::InputEvent`s.
//!
//! This is the embedded Windows host's native implementation of pointer,
//! keyboard and IME input. It mirrors `src/mac/input.rs`: the mapping rules are
//! shared with the UI's expected input model, and they stay platform-pure so
//! they compile and unit-test on any host.

use igui::igui_app::{AppBuilder, PlatformEvent, PlatformObserver, Plugin};
use igui::igui_core::{ImeEvent, InputEvent, Key, Modifiers, PointerButton, Vec2};

// --- Win32 virtual-key codes -------------------------------------------------
//
// A virtual key is just a `u32`; naming the ones we translate keeps this module
// free of any `windows.h` dependency.

pub const VK_BACK: u32 = 0x08;
pub const VK_TAB: u32 = 0x09;
pub const VK_RETURN: u32 = 0x0D;
pub const VK_ESCAPE: u32 = 0x1B;
pub const VK_SPACE: u32 = 0x20;
pub const VK_END: u32 = 0x23;
pub const VK_HOME: u32 = 0x24;
pub const VK_LEFT: u32 = 0x25;
pub const VK_UP: u32 = 0x26;
pub const VK_RIGHT: u32 = 0x27;
pub const VK_DOWN: u32 = 0x28;
pub const VK_DELETE: u32 = 0x2E;
pub const VK_F1: u32 = 0x70;
pub const VK_F5: u32 = 0x74;
pub const VK_F12: u32 = 0x7B;

/// One translated native event. The C++ shell builds these through `ffi.rs`;
/// the observer below turns each into core input events.
#[derive(Clone, Debug)]
pub enum WinEvent {
    PointerMove(Vec2),
    PointerDown {
        position: Vec2,
        button: PointerButton,
        click_count: u32,
    },
    PointerUp {
        position: Vec2,
        button: PointerButton,
    },
    PointerLeave,
    Wheel {
        position: Vec2,
        delta: Vec2,
    },
    KeyDown(Key),
    KeyUp(Key),
    Text(String),
    Modifiers(Modifiers),
    Ime(ImeEvent),
}

impl WinEvent {
    /// The core input events this native event produces.
    pub fn to_input(&self) -> Vec<InputEvent> {
        match self {
            Self::PointerMove(position) => vec![InputEvent::PointerMove {
                position: *position,
            }],
            Self::PointerDown {
                position,
                button,
                click_count,
            } => {
                let mut events = vec![InputEvent::PointerDown {
                    position: *position,
                    button: *button,
                }];
                // The shell tracks the double-click interval (Win32 reports
                // `WM_*BUTTONDBLCLK`) and passes 2 here.
                if *click_count >= 2 {
                    events.push(InputEvent::DoubleClick {
                        position: *position,
                    });
                }
                events
            }
            Self::PointerUp { position, button } => vec![InputEvent::PointerUp {
                position: *position,
                button: *button,
            }],
            Self::PointerLeave => vec![InputEvent::PointerLeave],
            Self::Wheel { position, delta } => vec![InputEvent::Wheel {
                position: *position,
                delta: *delta,
            }],
            Self::KeyDown(key) => vec![InputEvent::KeyDown { key: *key }],
            Self::KeyUp(key) => vec![InputEvent::KeyUp { key: *key }],
            Self::Text(text) => vec![InputEvent::TextInput { text: text.clone() }],
            Self::Modifiers(modifiers) => vec![InputEvent::ModifiersChanged(*modifiers)],
            Self::Ime(event) => vec![InputEvent::Ime(event.clone())],
        }
    }
}

/// Translates [`WinEvent`]s into core input events.
#[derive(Default)]
pub struct WinInputPlugin;

impl Plugin for WinInputPlugin {
    fn name(&self) -> &'static str {
        "cgb-win-input"
    }

    fn build(&self, app: &mut AppBuilder) {
        app.add_platform_observer(WinInputObserver);
    }
}

struct WinInputObserver;

impl PlatformObserver for WinInputObserver {
    fn on_platform(&mut self, event: PlatformEvent<'_>, out: &mut Vec<InputEvent>) {
        if let Some(event) = event.downcast_ref::<WinEvent>() {
            out.extend(event.to_input());
        }
    }
}

/// Win32 virtual-key code → core key, falling back to the typed character.
///
/// `characters` is the text the key produced (`ToUnicode` at `WM_KEYDOWN`
/// time, so a shortcut like Ctrl+C still yields `'c'` while the modifier set
/// carries Ctrl).
pub fn key_from_code(code: u32, characters: Option<&str>) -> Option<Key> {
    match code {
        VK_RETURN => Some(Key::Enter),
        VK_ESCAPE => Some(Key::Escape),
        VK_BACK => Some(Key::Backspace),
        VK_DELETE => Some(Key::Delete),
        VK_TAB => Some(Key::Tab),
        VK_SPACE => Some(Key::Space),
        VK_HOME => Some(Key::Home),
        VK_END => Some(Key::End),
        VK_LEFT => Some(Key::ArrowLeft),
        VK_RIGHT => Some(Key::ArrowRight),
        VK_DOWN => Some(Key::ArrowDown),
        VK_UP => Some(Key::ArrowUp),
        VK_F1..=VK_F12 => Some(function_key(code - VK_F1)),
        _ => characters
            .and_then(|text| text.chars().next())
            .map(Key::Character),
    }
}

/// The `F1..F12` key for an offset from `VK_F1`.
fn function_key(offset: u32) -> Key {
    match offset {
        0 => Key::F1,
        1 => Key::F2,
        2 => Key::F3,
        3 => Key::F4,
        4 => Key::F5,
        5 => Key::F6,
        6 => Key::F7,
        7 => Key::F8,
        8 => Key::F9,
        9 => Key::F10,
        10 => Key::F11,
        _ => Key::F12,
    }
}

/// The modifier set from the shell's bit mask (`1` shift, `2` ctrl, `4` alt,
/// `8` meta/Windows). The layout matches `macos/` so both hosts speak the same
/// ABI.
pub fn modifiers_from_bits(bits: u32) -> Modifiers {
    Modifiers {
        shift: bits & 1 != 0,
        ctrl: bits & 2 != 0,
        alt: bits & 4 != 0,
        meta: bits & 8 != 0,
    }
}

/// The pointer button from the shell's tag (`0` left, `1` right, `2` middle).
pub fn pointer_button(tag: u32) -> PointerButton {
    match tag {
        1 => PointerButton::Right,
        2 => PointerButton::Middle,
        _ => PointerButton::Left,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use igui::igui_core::{ImeEvent, InputEvent, Key, PointerButton, Vec2};

    #[test]
    fn named_keys_map_by_code() {
        assert_eq!(key_from_code(VK_RETURN, None), Some(Key::Enter));
        assert_eq!(key_from_code(VK_ESCAPE, None), Some(Key::Escape));
        assert_eq!(key_from_code(VK_BACK, None), Some(Key::Backspace));
        assert_eq!(key_from_code(VK_LEFT, None), Some(Key::ArrowLeft));
        assert_eq!(key_from_code(VK_F5, None), Some(Key::F5));
        assert_eq!(key_from_code(VK_F12, None), Some(Key::F12));
    }

    #[test]
    fn printable_keys_fall_back_to_the_character() {
        assert_eq!(key_from_code(0x41, Some("a")), Some(Key::Character('a')));
        // A shortcut like Ctrl+C still carries its character.
        assert_eq!(key_from_code(0x43, Some("c")), Some(Key::Character('c')));
        assert_eq!(key_from_code(0x41, None), None);
    }

    #[test]
    fn modifier_bits_map() {
        let modifiers = modifiers_from_bits(0b1011);
        assert!(modifiers.shift && modifiers.ctrl && !modifiers.alt && modifiers.meta);
        assert_eq!(modifiers_from_bits(0), Modifiers::NONE);
    }

    #[test]
    fn pointer_buttons_map() {
        assert_eq!(pointer_button(0), PointerButton::Left);
        assert_eq!(pointer_button(1), PointerButton::Right);
        assert_eq!(pointer_button(2), PointerButton::Middle);
        assert_eq!(pointer_button(9), PointerButton::Left);
    }

    #[test]
    fn a_double_click_reports_both_events() {
        let event = WinEvent::PointerDown {
            position: Vec2::new(1.0, 2.0),
            button: PointerButton::Left,
            click_count: 2,
        };
        assert!(matches!(
            event.to_input().as_slice(),
            [
                InputEvent::PointerDown { .. },
                InputEvent::DoubleClick { .. }
            ]
        ));
    }

    #[test]
    fn text_and_ime_translate() {
        assert_eq!(
            WinEvent::Text("a".into()).to_input(),
            vec![InputEvent::TextInput { text: "a".into() }]
        );
        assert_eq!(
            WinEvent::Ime(ImeEvent::Disabled).to_input(),
            vec![InputEvent::Ime(ImeEvent::Disabled)]
        );
    }
}

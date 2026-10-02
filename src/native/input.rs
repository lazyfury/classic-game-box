//! Native event translation, shared by every embedded host.
//!
//! The macOS (AppKit) and Windows (Win32) shells deliver the same event
//! vocabulary through the C ABI — pointer, wheel, key, text and IME — so the
//! translation into `igui_core::InputEvent`s lives here once. The only
//! platform-specific pieces are the virtual-key-code tables: the shell sends
//! its own key code (an AppKit `keyCode` or a Win32 virtual key), and
//! [`key_from_code`] picks the matching table for the running target.

use igui::igui_app::{AppBuilder, PlatformEvent, PlatformObserver, Plugin};
use igui::igui_core::{ImeEvent, InputEvent, Key, Modifiers, PointerButton, Vec2};

/// One translated native event. The shell builds these through `ffi.rs`; the
/// observer below turns each into core input events.
#[derive(Clone, Debug)]
pub enum NativeEvent {
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

impl NativeEvent {
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
                // The shell tracks its platform's double-click window (AppKit
                // `clickCount`, Win32 `WM_*BUTTONDBLCLK`) and passes `2` here.
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

/// Translates [`NativeEvent`]s into core input events.
#[derive(Default)]
pub struct NativeInputPlugin;

impl Plugin for NativeInputPlugin {
    fn name(&self) -> &'static str {
        "cgb-host-input"
    }

    fn build(&self, app: &mut AppBuilder) {
        app.add_platform_observer(NativeInputObserver);
    }
}

struct NativeInputObserver;

impl PlatformObserver for NativeInputObserver {
    fn on_platform(&mut self, event: PlatformEvent<'_>, out: &mut Vec<InputEvent>) {
        if let Some(event) = event.downcast_ref::<NativeEvent>() {
            out.extend(event.to_input());
        }
    }
}

/// The running shell's virtual-key code → core key, falling back to the typed
/// character.
///
/// `characters` is the text the key produced with modifiers ignored (`Cmd`/Ctrl
/// masked out), so a shortcut like Cmd/Ctrl+C still yields `'c'` while the
/// modifier set carries the command/ctrl bit.
#[cfg(target_os = "macos")]
pub fn key_from_code(code: u32, characters: Option<&str>) -> Option<Key> {
    mac_key(code, characters)
}

/// The running shell's virtual-key code → core key. See the macOS variant.
#[cfg(target_os = "windows")]
pub fn key_from_code(code: u32, characters: Option<&str>) -> Option<Key> {
    win_key(code, characters)
}

/// Fallback for targets with no shell; never used by a shipped host.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn key_from_code(code: u32, characters: Option<&str>) -> Option<Key> {
    let _ = code;
    characters
        .and_then(|text| text.chars().next())
        .map(Key::Character)
}

/// AppKit `keyCode` → core key.
///
/// Kept compiled (and tested) on every target so the macOS mapping cannot rot
/// when only Windows is being worked on.
pub fn mac_key(code: u32, characters: Option<&str>) -> Option<Key> {
    match code {
        0x24 | 0x4C => Some(Key::Enter),
        0x35 => Some(Key::Escape),
        0x33 => Some(Key::Backspace),
        0x75 => Some(Key::Delete),
        0x30 => Some(Key::Tab),
        0x31 => Some(Key::Space),
        0x73 => Some(Key::Home),
        0x77 => Some(Key::End),
        0x7B => Some(Key::ArrowLeft),
        0x7C => Some(Key::ArrowRight),
        0x7D => Some(Key::ArrowDown),
        0x7E => Some(Key::ArrowUp),
        0x7A => Some(Key::F1),
        0x78 => Some(Key::F2),
        0x63 => Some(Key::F3),
        0x76 => Some(Key::F4),
        0x60 => Some(Key::F5),
        0x61 => Some(Key::F6),
        0x62 => Some(Key::F7),
        0x64 => Some(Key::F8),
        0x65 => Some(Key::F9),
        0x6D => Some(Key::F10),
        0x67 => Some(Key::F11),
        0x6F => Some(Key::F12),
        _ => characters
            .and_then(|text| text.chars().next())
            .map(Key::Character),
    }
}

// Win32 virtual-key codes. A virtual key is just a `u32`; naming the ones we
// translate keeps this module free of any `windows.h` dependency.
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

/// Win32 virtual-key code → core key.
///
/// Kept compiled (and tested) on every target so the Windows mapping cannot rot
/// when only macOS is being worked on.
pub fn win_key(code: u32, characters: Option<&str>) -> Option<Key> {
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
/// `8` command/meta). Both shells use the same layout.
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
    fn mac_named_keys_map_by_code() {
        assert_eq!(mac_key(0x24, None), Some(Key::Enter));
        assert_eq!(mac_key(0x35, None), Some(Key::Escape));
        assert_eq!(mac_key(0x33, None), Some(Key::Backspace));
        assert_eq!(mac_key(0x7B, None), Some(Key::ArrowLeft));
        assert_eq!(mac_key(0x60, None), Some(Key::F5));
    }

    #[test]
    fn win_named_keys_map_by_code() {
        assert_eq!(win_key(VK_RETURN, None), Some(Key::Enter));
        assert_eq!(win_key(VK_ESCAPE, None), Some(Key::Escape));
        assert_eq!(win_key(VK_BACK, None), Some(Key::Backspace));
        assert_eq!(win_key(VK_LEFT, None), Some(Key::ArrowLeft));
        assert_eq!(win_key(VK_F5, None), Some(Key::F5));
        assert_eq!(win_key(VK_F12, None), Some(Key::F12));
    }

    #[test]
    fn printable_keys_fall_back_to_the_character() {
        assert_eq!(mac_key(0x00, Some("a")), Some(Key::Character('a')));
        // A shortcut like Cmd+C / Ctrl+C still carries its character.
        assert_eq!(mac_key(0x08, Some("c")), Some(Key::Character('c')));
        assert_eq!(win_key(0x41, Some("a")), Some(Key::Character('a')));
        assert_eq!(win_key(0x43, Some("c")), Some(Key::Character('c')));
        assert_eq!(mac_key(0x00, None), None);
        assert_eq!(win_key(0x41, None), None);
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
        let event = NativeEvent::PointerDown {
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
            NativeEvent::Text("a".into()).to_input(),
            vec![InputEvent::TextInput { text: "a".into() }]
        );
        assert_eq!(
            NativeEvent::Ime(ImeEvent::Disabled).to_input(),
            vec![InputEvent::Ime(ImeEvent::Disabled)]
        );
    }
}

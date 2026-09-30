//! Native event translation: AppKit events (delivered through the C ABI) →
//! `igui_core::InputEvent`s.
//!
//! This is the embedded host's replacement for `igui_winit`'s
//! `PointerPlugin` / `KeyboardPlugin` / `ImePlugin`. The mapping rules mirror
//! `igui_winit::input` so the UI behaves the same on both hosts.

use igui::igui_app::{AppBuilder, PlatformEvent, PlatformObserver, Plugin};
use igui::igui_core::{ImeEvent, InputEvent, Key, Modifiers, PointerButton, Vec2};

/// One translated native event. Swift builds these through `ffi.rs`; the
/// observer below turns each into core input events.
#[derive(Clone, Debug)]
pub enum MacEvent {
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

impl MacEvent {
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
                // A second press within the platform's double-click window is
                // reported as its own event (AppKit tracks the timing).
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

/// Translates [`MacEvent`]s into core input events.
#[derive(Default)]
pub struct MacInputPlugin;

impl Plugin for MacInputPlugin {
    fn name(&self) -> &'static str {
        "cgb-mac-input"
    }

    fn build(&self, app: &mut AppBuilder) {
        app.add_platform_observer(MacInputObserver);
    }
}

struct MacInputObserver;

impl PlatformObserver for MacInputObserver {
    fn on_platform(&mut self, event: PlatformEvent<'_>, out: &mut Vec<InputEvent>) {
        if let Some(event) = event.downcast_ref::<MacEvent>() {
            out.extend(event.to_input());
        }
    }
}

/// macOS virtual key code → core key, falling back to the typed character.
///
/// `characters` is the event's `charactersIgnoringModifiers` (so Cmd+C yields
/// `'c'`, letting hotkeys see a character key while the modifier set still
/// carries Command).
pub fn key_from_code(code: u32, characters: Option<&str>) -> Option<Key> {
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

/// The modifier set from Swift's bit mask (`1` shift, `2` ctrl, `4` alt,
/// `8` command).
pub fn modifiers_from_bits(bits: u32) -> Modifiers {
    Modifiers {
        shift: bits & 1 != 0,
        ctrl: bits & 2 != 0,
        alt: bits & 4 != 0,
        meta: bits & 8 != 0,
    }
}

/// The pointer button from Swift's tag (`0` left, `1` right, `2` middle).
pub fn pointer_button(tag: u32) -> PointerButton {
    match tag {
        1 => PointerButton::Right,
        2 => PointerButton::Middle,
        _ => PointerButton::Left,
    }
}

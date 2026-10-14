//! Terminal events to `lem.display.v1` input.
//!
//! The display describes what happened and the relay turns it into what
//! Lem consumes (ADR 0013, 0014): which key, which modifiers, which mouse
//! button at which cell. Lem's key names, its shift rules and click counts
//! are all the relay's. What stays here is reading the terminal right.

use lem_protocols::display::v1::{self, NamedKey, key, mouse};
use termina::Event;
use termina::event::{
    KeyCode, KeyEvent, KeyEventState, Modifiers, MouseButton, MouseEvent, MouseEventKind,
};

/// How the terminal encodes keys: legacy bytes, or the kitty keyboard
/// protocol once negotiation confirmed it (`keyboard.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Legacy,
    Kitty,
}

/// Name the control bytes 0x1C–0x1F by their ASCII keys, `C-\`, `C-]`,
/// `C-^` and `C-_`, for a key read under legacy encoding.
///
/// The parser reports those bytes as Ctrl+4..7, the keys that share them
/// in a legacy terminal. Under legacy encoding a Ctrl+digit 4..7 can only
/// have come from them; under the kitty protocol it is the digit, sent as
/// CSI u. So the encoding decides (ADR 0021).
pub fn legacy_control(key: KeyEvent) -> KeyEvent {
    let name = match key.code {
        KeyCode::Char('4') => '\\',
        KeyCode::Char('5') => ']',
        KeyCode::Char('6') => '^',
        KeyCode::Char('7') => '_',
        _ => return key,
    };
    if key.modifiers.contains(Modifiers::CONTROL) && !key.state.contains(KeyEventState::KEYPAD) {
        KeyEvent {
            code: KeyCode::Char(name),
            ..key
        }
    } else {
        key
    }
}

/// Decode `event` as read under `encoding` (see [`legacy_control`]).
pub fn decode(event: &mut Event, encoding: Encoding) {
    if encoding == Encoding::Legacy
        && let Event::Key(key) = event
    {
        *key = legacy_control(*key);
    }
}

/// Describe a key event, or `None` for a key the protocol has no word for.
///
/// Dropping an unknown key rather than guessing matters: forwarded as
/// something else, it would run some other binding.
pub fn key(event: KeyEvent) -> Option<v1::Key> {
    let code = match event.code {
        // Already decoded for its encoding (`decode`).
        KeyCode::Char(c) => key::Code::Text(c.to_string()),
        KeyCode::Enter => key::Code::Named(NamedKey::Enter as i32),
        KeyCode::Tab | KeyCode::BackTab => key::Code::Named(NamedKey::Tab as i32),
        KeyCode::Backspace => key::Code::Named(NamedKey::Backspace as i32),
        KeyCode::Escape => key::Code::Named(NamedKey::Escape as i32),
        KeyCode::Insert => key::Code::Named(NamedKey::Insert as i32),
        KeyCode::Delete => key::Code::Named(NamedKey::Delete as i32),
        KeyCode::Up => key::Code::Named(NamedKey::Up as i32),
        KeyCode::Down => key::Code::Named(NamedKey::Down as i32),
        KeyCode::Left => key::Code::Named(NamedKey::Left as i32),
        KeyCode::Right => key::Code::Named(NamedKey::Right as i32),
        KeyCode::Home => key::Code::Named(NamedKey::Home as i32),
        KeyCode::End => key::Code::Named(NamedKey::End as i32),
        KeyCode::PageUp => key::Code::Named(NamedKey::PageUp as i32),
        KeyCode::PageDown => key::Code::Named(NamedKey::PageDown as i32),
        KeyCode::Menu => key::Code::Named(NamedKey::ContextMenu as i32),
        KeyCode::Function(n) => key::Code::Function(u32::from(n)),
        _ => return None,
    };

    let mut modifiers = Vec::new();
    for (flag, modifier) in [
        (Modifiers::CONTROL, v1::Modifier::Ctrl),
        (Modifiers::ALT, v1::Modifier::Meta),
        (Modifiers::SHIFT, v1::Modifier::Shift),
        (Modifiers::SUPER, v1::Modifier::Super),
    ] {
        if event.modifiers.contains(flag) {
            modifiers.push(modifier as i32);
        }
    }
    // BackTab is shift-tab whether or not the terminal also sets SHIFT.
    if event.code == KeyCode::BackTab && !event.modifiers.contains(Modifiers::SHIFT) {
        modifiers.push(v1::Modifier::Shift as i32);
    }

    Some(v1::Key {
        code: Some(code),
        modifiers,
        keypad: event.state.contains(KeyEventState::KEYPAD),
    })
}

fn button(button: MouseButton) -> v1::Button {
    match button {
        MouseButton::Left => v1::Button::Left,
        MouseButton::Middle => v1::Button::Middle,
        MouseButton::Right => v1::Button::Right,
    }
}

/// Describe a mouse event, at the screen cell it happened on.
pub fn mouse(event: MouseEvent) -> v1::Mouse {
    let action = match event.kind {
        MouseEventKind::Down(b) => mouse::Action::Press(v1::Press {
            button: button(b) as i32,
        }),
        MouseEventKind::Up(b) => mouse::Action::Release(v1::Release {
            button: button(b) as i32,
        }),
        MouseEventKind::Drag(b) => mouse::Action::Move(v1::Move {
            button: Some(button(b) as i32),
        }),
        MouseEventKind::Moved => mouse::Action::Move(v1::Move { button: None }),
        // Lines, positive up and left, as Lem reads them.
        MouseEventKind::ScrollUp => mouse::Action::Wheel(v1::Wheel { dx: 0, dy: 1 }),
        MouseEventKind::ScrollDown => mouse::Action::Wheel(v1::Wheel { dx: 0, dy: -1 }),
        MouseEventKind::ScrollLeft => mouse::Action::Wheel(v1::Wheel { dx: 1, dy: 0 }),
        MouseEventKind::ScrollRight => mouse::Action::Wheel(v1::Wheel { dx: -1, dy: 0 }),
    };
    v1::Mouse {
        x: u32::from(event.column),
        y: u32::from(event.row),
        action: Some(action),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn described(code: KeyCode, modifiers: Modifiers) -> v1::Key {
        key(KeyEvent::new(code, modifiers)).expect("a key the protocol knows")
    }

    fn text(k: &v1::Key) -> Option<&str> {
        match &k.code {
            Some(key::Code::Text(text)) => Some(text),
            _ => None,
        }
    }

    fn has(k: &v1::Key, modifier: v1::Modifier) -> bool {
        k.modifiers.contains(&(modifier as i32))
    }

    #[test]
    fn a_character_is_its_text() {
        let k = described(KeyCode::Char('a'), Modifiers::NONE);
        assert_eq!(text(&k), Some("a"));
        assert!(k.modifiers.is_empty());
    }

    #[test]
    fn a_space_is_text_too() {
        // Naming it "Space" is the relay's job (ADR 0014).
        assert_eq!(
            text(&described(KeyCode::Char(' '), Modifiers::NONE)),
            Some(" ")
        );
    }

    #[test]
    fn modifiers_are_reported_as_they_are() {
        let k = described(
            KeyCode::Char('A'),
            Modifiers::SHIFT | Modifiers::ALT | Modifiers::CONTROL,
        );
        assert!(
            has(&k, v1::Modifier::Shift),
            "the relay decides what shift means"
        );
        assert!(has(&k, v1::Modifier::Meta), "ALT is Lem's meta");
        assert!(has(&k, v1::Modifier::Ctrl));
    }

    #[test]
    fn named_keys_are_named() {
        for (code, named) in [
            (KeyCode::Enter, NamedKey::Enter),
            (KeyCode::Tab, NamedKey::Tab),
            (KeyCode::Escape, NamedKey::Escape),
            (KeyCode::Backspace, NamedKey::Backspace),
            (KeyCode::Insert, NamedKey::Insert),
            (KeyCode::Delete, NamedKey::Delete),
            (KeyCode::Up, NamedKey::Up),
            (KeyCode::Home, NamedKey::Home),
            (KeyCode::PageDown, NamedKey::PageDown),
            (KeyCode::Menu, NamedKey::ContextMenu),
        ] {
            assert_eq!(
                described(code, Modifiers::NONE).code,
                Some(key::Code::Named(named as i32)),
                "{code:?}"
            );
        }
    }

    #[test]
    fn function_keys_are_numbered() {
        assert_eq!(
            described(KeyCode::Function(12), Modifiers::NONE).code,
            Some(key::Code::Function(12))
        );
    }

    #[test]
    fn back_tab_is_shift_tab() {
        let k = described(KeyCode::BackTab, Modifiers::NONE);
        assert_eq!(k.code, Some(key::Code::Named(NamedKey::Tab as i32)));
        assert!(has(&k, v1::Modifier::Shift));
        let k = described(KeyCode::BackTab, Modifiers::SHIFT);
        assert_eq!(k.modifiers.len(), 1, "shift once, not twice");
    }

    #[test]
    fn control_digits_and_ascii_controls_remain_distinct() {
        for character in ['4', '5', '6', '7', '\\', ']', '^', '_'] {
            let k = described(KeyCode::Char(character), Modifiers::CONTROL);
            assert_eq!(text(&k), Some(character.to_string().as_str()));
            assert!(has(&k, v1::Modifier::Ctrl));
        }
    }

    #[test]
    fn keypad_provenance_is_independent_of_code_and_modifiers() {
        for code in [
            KeyCode::Char('1'),
            KeyCode::Char('5'),
            KeyCode::Char('+'),
            KeyCode::Char('/'),
            KeyCode::Enter,
            KeyCode::Left,
            KeyCode::Home,
        ] {
            for modifiers in [Modifiers::NONE, Modifiers::CONTROL] {
                let ordinary = described(code, modifiers);
                let mut event = KeyEvent::new(code, modifiers);
                event.state = KeyEventState::KEYPAD;
                let keypad = key(event).unwrap();
                assert!(keypad.keypad);
                assert!(!ordinary.keypad);
                assert_eq!(keypad.code, ordinary.code);
                assert_eq!(keypad.modifiers, ordinary.modifiers);
            }
        }
    }

    #[test]
    fn unicode_and_shifted_control_text_reach_the_relay_unchanged() {
        for (character, modifiers) in [
            ('č', Modifiers::NONE),
            ('š', Modifiers::NONE),
            ('g', Modifiers::CONTROL | Modifiers::SHIFT),
            ('G', Modifiers::CONTROL),
        ] {
            let k = described(KeyCode::Char(character), modifiers);
            assert_eq!(text(&k), Some(character.to_string().as_str()));
        }
    }

    #[test]
    fn under_legacy_encoding_ctrl_4_to_7_are_the_ascii_controls() {
        for (digit, name) in [('4', '\\'), ('5', ']'), ('6', '^'), ('7', '_')] {
            let key = legacy_control(KeyEvent::new(KeyCode::Char(digit), Modifiers::CONTROL));
            assert_eq!(key.code, KeyCode::Char(name));
            assert_eq!(key.modifiers, Modifiers::CONTROL);
        }
    }

    #[test]
    fn meta_survives_the_renaming() {
        let key = legacy_control(KeyEvent::new(
            KeyCode::Char('7'),
            Modifiers::CONTROL | Modifiers::ALT,
        ));
        assert_eq!(key.code, KeyCode::Char('_'));
        assert_eq!(key.modifiers, Modifiers::CONTROL | Modifiers::ALT);
    }

    #[test]
    fn other_keys_keep_their_names() {
        let mut keypad = KeyEvent::new(KeyCode::Char('4'), Modifiers::CONTROL);
        keypad.state = KeyEventState::KEYPAD;
        for key in [
            KeyEvent::new(KeyCode::Char('4'), Modifiers::NONE),
            KeyEvent::new(KeyCode::Char('8'), Modifiers::CONTROL),
            KeyEvent::new(KeyCode::Char('3'), Modifiers::CONTROL),
            keypad,
        ] {
            assert_eq!(legacy_control(key), key);
        }
    }

    #[test]
    fn decoding_renames_only_legacy_keys() {
        let ctrl_5 = || Event::Key(KeyEvent::new(KeyCode::Char('5'), Modifiers::CONTROL));
        let mut legacy = ctrl_5();
        decode(&mut legacy, Encoding::Legacy);
        assert_eq!(
            legacy,
            Event::Key(KeyEvent::new(KeyCode::Char(']'), Modifiers::CONTROL))
        );
        let mut kitty = ctrl_5();
        decode(&mut kitty, Encoding::Kitty);
        assert_eq!(kitty, ctrl_5());
        let mut paste = Event::Paste("5".into());
        decode(&mut paste, Encoding::Legacy);
        assert_eq!(paste, Event::Paste("5".into()));
    }

    #[test]
    fn unknown_keys_are_dropped() {
        assert!(key(KeyEvent::new(KeyCode::Null, Modifiers::NONE)).is_none());
    }

    fn mouse_at(kind: MouseEventKind) -> v1::Mouse {
        mouse(MouseEvent {
            kind,
            column: 7,
            row: 3,
            modifiers: Modifiers::NONE,
        })
    }

    #[test]
    fn mouse_events_carry_their_cell_and_action() {
        let press = mouse_at(MouseEventKind::Down(MouseButton::Left));
        assert_eq!((press.x, press.y), (7, 3));
        assert_eq!(
            press.action,
            Some(mouse::Action::Press(v1::Press {
                button: v1::Button::Left as i32
            }))
        );
        assert_eq!(
            mouse_at(MouseEventKind::Drag(MouseButton::Right)).action,
            Some(mouse::Action::Move(v1::Move {
                button: Some(v1::Button::Right as i32)
            })),
            "a drag is a move with a button"
        );
        assert_eq!(
            mouse_at(MouseEventKind::Moved).action,
            Some(mouse::Action::Move(v1::Move { button: None }))
        );
    }

    #[test]
    fn the_wheel_is_in_lines_positive_up() {
        assert_eq!(
            mouse_at(MouseEventKind::ScrollUp).action,
            Some(mouse::Action::Wheel(v1::Wheel { dx: 0, dy: 1 }))
        );
        assert_eq!(
            mouse_at(MouseEventKind::ScrollDown).action,
            Some(mouse::Action::Wheel(v1::Wheel { dx: 0, dy: -1 }))
        );
    }
}

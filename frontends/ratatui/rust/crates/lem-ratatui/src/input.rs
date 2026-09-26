//! crossterm events to `lem.relay.v1` input.
//!
//! The display describes what happened and the relay turns it into what
//! Lem consumes (ADR 0013, 0014): which key, which modifiers, which mouse
//! button at which cell. Lem's key names, its shift rules and click counts
//! are all the relay's. What stays here is reading the terminal right.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use lem_protocol::v1::{self, NamedKey, key, mouse};

/// Describe a key event, or `None` for a key the protocol has no word for.
///
/// Dropping an unknown key rather than guessing matters: forwarded as
/// something else, it would run some other binding.
pub fn key(event: KeyEvent) -> Option<v1::Key> {
    let code = match event.code {
        // crossterm decodes the C0 controls 0x1C..0x1F as Ctrl+'4'..'7'
        // (`c - 0x1C + b'4'` in its unix parser), but the terminal sent
        // C-\ C-] C-^ C-_, as ASCII and Lem name those bytes; C-_ is
        // redo, and C-] is abort. Both readings arrive as the same byte,
        // and the ASCII one is what Lem binds. The kitty keyboard
        // protocol would tell them apart.
        KeyCode::Char(digit @ ('4'..='7')) if event.modifiers.contains(KeyModifiers::CONTROL) => {
            const CONTROLS: [&str; 4] = ["\\", "]", "^", "_"];
            key::Code::Text(CONTROLS[digit as usize - '4' as usize].into())
        }
        KeyCode::Char(c) => key::Code::Text(c.to_string()),
        KeyCode::Enter => key::Code::Named(NamedKey::Enter as i32),
        KeyCode::Tab | KeyCode::BackTab => key::Code::Named(NamedKey::Tab as i32),
        KeyCode::Backspace => key::Code::Named(NamedKey::Backspace as i32),
        KeyCode::Esc => key::Code::Named(NamedKey::Escape as i32),
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
        KeyCode::F(n) => key::Code::Function(u32::from(n)),
        _ => return None,
    };

    let mut modifiers = Vec::new();
    for (flag, modifier) in [
        (KeyModifiers::CONTROL, v1::Modifier::Ctrl),
        (KeyModifiers::ALT, v1::Modifier::Meta),
        (KeyModifiers::SHIFT, v1::Modifier::Shift),
        (KeyModifiers::SUPER, v1::Modifier::Super),
    ] {
        if event.modifiers.contains(flag) {
            modifiers.push(modifier as i32);
        }
    }
    // BackTab is shift-tab whether or not the terminal also sets SHIFT.
    if event.code == KeyCode::BackTab && !event.modifiers.contains(KeyModifiers::SHIFT) {
        modifiers.push(v1::Modifier::Shift as i32);
    }

    Some(v1::Key {
        code: Some(code),
        modifiers,
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

    fn described(code: KeyCode, modifiers: KeyModifiers) -> v1::Key {
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
        let k = described(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(text(&k), Some("a"));
        assert!(k.modifiers.is_empty());
    }

    #[test]
    fn a_space_is_text_too() {
        // Naming it "Space" is the relay's job (ADR 0014).
        assert_eq!(
            text(&described(KeyCode::Char(' '), KeyModifiers::NONE)),
            Some(" ")
        );
    }

    #[test]
    fn modifiers_are_reported_as_they_are() {
        let k = described(
            KeyCode::Char('A'),
            KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::CONTROL,
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
            (KeyCode::Esc, NamedKey::Escape),
            (KeyCode::Backspace, NamedKey::Backspace),
            (KeyCode::Insert, NamedKey::Insert),
            (KeyCode::Delete, NamedKey::Delete),
            (KeyCode::Up, NamedKey::Up),
            (KeyCode::Home, NamedKey::Home),
            (KeyCode::PageDown, NamedKey::PageDown),
            (KeyCode::Menu, NamedKey::ContextMenu),
        ] {
            assert_eq!(
                described(code, KeyModifiers::NONE).code,
                Some(key::Code::Named(named as i32)),
                "{code:?}"
            );
        }
    }

    #[test]
    fn function_keys_are_numbered() {
        assert_eq!(
            described(KeyCode::F(12), KeyModifiers::NONE).code,
            Some(key::Code::Function(12))
        );
    }

    #[test]
    fn back_tab_is_shift_tab() {
        let k = described(KeyCode::BackTab, KeyModifiers::NONE);
        assert_eq!(k.code, Some(key::Code::Named(NamedKey::Tab as i32)));
        assert!(has(&k, v1::Modifier::Shift));
        let k = described(KeyCode::BackTab, KeyModifiers::SHIFT);
        assert_eq!(k.modifiers.len(), 1, "shift once, not twice");
    }

    #[test]
    fn the_c0_controls_keep_their_ascii_meaning() {
        for (reported, meant) in [('4', "\\"), ('5', "]"), ('6', "^"), ('7', "_")] {
            let k = described(KeyCode::Char(reported), KeyModifiers::CONTROL);
            assert_eq!(text(&k), Some(meant), "Ctrl+{reported}");
            assert!(has(&k, v1::Modifier::Ctrl));
        }
        assert_eq!(
            text(&described(KeyCode::Char('4'), KeyModifiers::NONE)),
            Some("4")
        );
    }

    #[test]
    fn unknown_keys_are_dropped() {
        assert!(key(KeyEvent::new(KeyCode::Null, KeyModifiers::NONE)).is_none());
    }

    fn mouse_at(kind: MouseEventKind) -> v1::Mouse {
        mouse(MouseEvent {
            kind,
            column: 7,
            row: 3,
            modifiers: KeyModifiers::NONE,
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

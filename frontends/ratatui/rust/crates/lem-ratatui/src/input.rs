//! crossterm key events to Lem key syms.
//!
//! The vocabulary is Lem's, not crossterm's. `frontends/ncurses/key.lisp`
//! is the authoritative list of sym names, and `convert-keyevent` in
//! `frontends/server/main.lisp` is the receiving end.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::Serialize;

/// The `value` of an `input` notification of kind `key`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyPayload {
    pub key: String,
    pub ctrl: bool,
    pub meta: bool,
    /// Serialised as `super`, which is a reserved word in Rust.
    #[serde(rename = "super")]
    pub super_: bool,
    pub shift: bool,
}

/// Translate a key event, or `None` if Lem has no sym for it.
///
/// Returning `None` rather than guessing matters: an unrecognised key
/// forwarded as a bogus sym would be interpreted by Lem as some other
/// binding, which is worse than being ignored.
pub fn convert(event: KeyEvent) -> Option<KeyPayload> {
    let sym = match event.code {
        // Space is a named sym, not the character; convert-keyevent
        // special-cases it on the far side too.
        KeyCode::Char(' ') => "Space".to_string(),
        // crossterm decodes the C0 controls 0x1C..0x1F as Ctrl+'4'..'7'
        // — `(c - 0x1C + b'4')` in its unix parser — but ASCII names
        // those bytes C-\ C-] C-^ C-_, and so does Lem
        // (`frontends/ncurses/key.lisp`). C-_ is bound to redo, so
        // passing the digit through breaks a documented binding.
        //
        // The two are indistinguishable on the wire: a terminal sends
        // 0x1C for both Ctrl+4 and C-\. The ASCII reading is the one
        // Lem binds, so it wins. Enabling the kitty keyboard protocol
        // would disambiguate them and this would need revisiting.
        KeyCode::Char(digit @ ('4'..='7')) if event.modifiers.contains(KeyModifiers::CONTROL) => {
            const NAMES: [&str; 4] = ["\\", "]", "^", "_"];
            NAMES[digit as usize - '4' as usize].to_string()
        }
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "Return".to_string(),
        KeyCode::Tab | KeyCode::BackTab => "Tab".to_string(),
        KeyCode::Esc => "Escape".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Delete => "Delete".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::PageUp => "PageUp".to_string(),
        KeyCode::PageDown => "PageDown".to_string(),
        KeyCode::F(n) => format!("F{n}"),
        _ => return None,
    };

    // Mirror lem:insertion-key-sym-p, which is simply (= 1 (length sym)).
    // Lem drops shift for those, folding it into the character itself; a
    // sym of "A" with shift set matches no binding.
    let is_insertion = sym.chars().count() == 1;
    let shift =
        event.modifiers.contains(KeyModifiers::SHIFT) || matches!(event.code, KeyCode::BackTab);

    Some(KeyPayload {
        ctrl: event.modifiers.contains(KeyModifiers::CONTROL),
        meta: event.modifiers.contains(KeyModifiers::ALT),
        super_: event.modifiers.contains(KeyModifiers::SUPER),
        shift: !is_insertion && shift,
        key: sym,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Option<KeyPayload> {
        convert(KeyEvent::new(code, modifiers))
    }

    #[test]
    fn plain_characters_become_single_character_syms() {
        let payload = key(KeyCode::Char('a'), KeyModifiers::NONE).unwrap();
        assert_eq!(payload.key, "a");
        assert!(!payload.ctrl && !payload.meta && !payload.shift && !payload.super_);
    }

    #[test]
    fn control_and_alt_are_carried_as_flags() {
        let payload = key(KeyCode::Char('x'), KeyModifiers::CONTROL).unwrap();
        assert_eq!(payload.key, "x");
        assert!(payload.ctrl);

        let payload = key(KeyCode::Char('x'), KeyModifiers::ALT).unwrap();
        assert!(payload.meta, "ALT is Lem's meta");
    }

    #[test]
    fn shift_is_dropped_for_insertion_keys() {
        // convert-keyevent discards shift when the sym is one character
        // (lem:insertion-key-sym-p). Sending it produces a key Lem cannot
        // match, so the capital arrives with shift already folded in.
        let payload = key(KeyCode::Char('A'), KeyModifiers::SHIFT).unwrap();
        assert_eq!(payload.key, "A");
        assert!(!payload.shift, "shift must be dropped for a 1-char sym");
    }

    #[test]
    fn shift_survives_for_named_keys() {
        let payload = key(KeyCode::F(3), KeyModifiers::SHIFT).unwrap();
        assert_eq!(payload.key, "F3");
        assert!(payload.shift);
    }

    #[test]
    fn space_has_its_own_sym() {
        let payload = key(KeyCode::Char(' '), KeyModifiers::NONE).unwrap();
        assert_eq!(payload.key, "Space");
        assert!(!payload.shift);
    }

    #[test]
    fn named_keys_use_lems_vocabulary() {
        for (code, expected) in [
            (KeyCode::Enter, "Return"),
            (KeyCode::Tab, "Tab"),
            (KeyCode::BackTab, "Tab"),
            (KeyCode::Esc, "Escape"),
            (KeyCode::Backspace, "Backspace"),
            (KeyCode::Delete, "Delete"),
            (KeyCode::Up, "Up"),
            (KeyCode::Down, "Down"),
            (KeyCode::Left, "Left"),
            (KeyCode::Right, "Right"),
            (KeyCode::Home, "Home"),
            (KeyCode::End, "End"),
            (KeyCode::PageUp, "PageUp"),
            (KeyCode::PageDown, "PageDown"),
            (KeyCode::F(1), "F1"),
            (KeyCode::F(12), "F12"),
        ] {
            assert_eq!(
                key(code, KeyModifiers::NONE).unwrap().key,
                expected,
                "{code:?}"
            );
        }
    }

    #[test]
    fn back_tab_carries_shift() {
        // BackTab *is* shift-tab; the modifier may or may not be set
        // depending on the terminal, so it is asserted explicitly.
        let payload = key(KeyCode::BackTab, KeyModifiers::NONE).unwrap();
        assert_eq!(payload.key, "Tab");
        assert!(payload.shift);
    }

    #[test]
    fn the_c0_controls_keep_their_ascii_names() {
        // crossterm reports bytes 0x1C..0x1F as Ctrl+'4'..'7' — see
        // parse_event in its unix parser — but ASCII and Lem both call
        // them C-\ C-] C-^ C-_. C-_ is bound to redo, so getting this
        // wrong breaks a documented binding.
        for (reported, expected) in [('4', "\\"), ('5', "]"), ('6', "^"), ('7', "_")] {
            let payload = key(KeyCode::Char(reported), KeyModifiers::CONTROL).unwrap();
            assert_eq!(payload.key, expected, "Ctrl+{reported}");
            assert!(payload.ctrl);
        }
    }

    #[test]
    fn digits_without_control_are_left_alone() {
        assert_eq!(
            key(KeyCode::Char('4'), KeyModifiers::NONE).unwrap().key,
            "4"
        );
    }

    #[test]
    fn unmapped_keys_are_dropped() {
        assert!(key(KeyCode::Null, KeyModifiers::NONE).is_none());
    }

    #[test]
    fn a_multibyte_character_is_one_sym() {
        let payload = key(KeyCode::Char('é'), KeyModifiers::NONE).unwrap();
        assert_eq!(payload.key, "é");
        assert!(!payload.shift, "one char, so shift is dropped");
    }

    #[test]
    fn serialises_under_the_wire_names() {
        let payload = key(KeyCode::Char('a'), KeyModifiers::SUPER).unwrap();
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["super"], true);
        assert!(
            json.get("super_").is_none(),
            "no Rust-side name on the wire"
        );
        for field in ["key", "ctrl", "meta", "shift"] {
            assert!(json.get(field).is_some(), "missing {field}");
        }
    }
}

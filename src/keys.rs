//! A key from the terminal, as the framework reports it, turned into the key a web page hears.
//!
//! A terminal reports the character a key typed, not the physical key under the finger, so the
//! physical key (the DOM `code`) and the Windows key code (`keyCode`) are a best guess from the
//! character on a US layout: `?` is Shift with the `Slash` key, `1` is `Digit1`. A letter outside
//! ASCII (`ş`, `é`, `ж`) has no key on that layout, so its `code` is empty and its key code 0;
//! the page still receives the character as the key's value and as the text it types, which is
//! what fields and nearly every page listen to.
//!
//! The framework's chords carry Ctrl, Alt and Shift; terminals do not report Meta to it, so Meta
//! is never held here.

use qframe::event::{KeyEvent, KeyKind};
use qframe::keymap::Key;

use crate::engine::input::{KeyPress, Modifiers};

/// The page's key for a terminal key, or `None` for a key coming up: the page gets its key-up
/// with the key-down, since most terminals never report a release.
///
/// A key that types something carries its text, unless Ctrl or Alt is held: then it is a
/// shortcut for the page (`ctrl+a` selects all) and types nothing. Enter types `"\r"`, which is
/// what submits a form. Tab, Backspace, Esc, the arrows, Home, End, Page Up and Down, Delete,
/// Insert, the menu key and the function keys act rather than type.
#[must_use]
pub fn key_press(event: &KeyEvent) -> Option<KeyPress> {
    if event.kind == KeyKind::Release {
        return None;
    }
    let mods = event.chord.mods;
    let mut modifiers = Modifiers { alt: mods.alt, ctrl: mods.ctrl, meta: false, shift: mods.shift };
    let types = !mods.ctrl && !mods.alt;
    let acting = move |key: &str, key_code: u32| KeyPress {
        key: key.to_owned(),
        code: key.to_owned(),
        key_code,
        text: None,
        modifiers,
    };
    let press = match event.chord.key {
        Key::Char(c) => {
            let (code, key_code, shifted) = physical(c);
            modifiers.shift |= shifted;
            // The framework keeps a letter lowercase beside Shift; the page hears it as typed.
            let typed = event.text.unwrap_or(if mods.shift { c.to_uppercase().next().unwrap_or(c) } else { c });
            KeyPress {
                key: typed.to_string(),
                code: code.to_owned(),
                key_code,
                text: types.then(|| typed.to_string()),
                modifiers,
            }
        }
        Key::Space => KeyPress {
            key: " ".to_owned(),
            code: "Space".to_owned(),
            key_code: 32,
            text: types.then(|| " ".to_owned()),
            modifiers,
        },
        Key::Enter => KeyPress {
            key: "Enter".to_owned(),
            code: "Enter".to_owned(),
            key_code: 13,
            text: types.then(|| "\r".to_owned()),
            modifiers,
        },
        Key::Tab => acting("Tab", 9),
        Key::Backspace => acting("Backspace", 8),
        Key::Esc => acting("Escape", 27),
        Key::Delete => acting("Delete", 46),
        Key::Insert => acting("Insert", 45),
        Key::Home => acting("Home", 36),
        Key::End => acting("End", 35),
        Key::PageUp => acting("PageUp", 33),
        Key::PageDown => acting("PageDown", 34),
        Key::Left => acting("ArrowLeft", 37),
        Key::Up => acting("ArrowUp", 38),
        Key::Right => acting("ArrowRight", 39),
        Key::Down => acting("ArrowDown", 40),
        Key::Menu => acting("ContextMenu", 93),
        // F1 is 112; the key codes run on to F24.
        Key::F(n @ 1..=24) => acting(&format!("F{n}"), 111 + u32::from(n)),
        Key::F(_) => return None,
    };
    Some(press)
}

/// The physical key of a character on a US layout: its DOM `code`, its Windows key code and
/// whether Shift is pressed to type it. A character that layout has no key for gets an empty
/// code and key code 0.
fn physical(c: char) -> (&'static str, u32, bool) {
    const LETTERS: [&str; 26] = [
        "KeyA", "KeyB", "KeyC", "KeyD", "KeyE", "KeyF", "KeyG", "KeyH", "KeyI", "KeyJ", "KeyK", "KeyL", "KeyM", "KeyN",
        "KeyO", "KeyP", "KeyQ", "KeyR", "KeyS", "KeyT", "KeyU", "KeyV", "KeyW", "KeyX", "KeyY", "KeyZ",
    ];
    const DIGITS: [&str; 10] =
        ["Digit0", "Digit1", "Digit2", "Digit3", "Digit4", "Digit5", "Digit6", "Digit7", "Digit8", "Digit9"];
    // The characters Shift types on the digit row, from 0 to 9.
    const SHIFTED_DIGITS: [char; 10] = [')', '!', '@', '#', '$', '%', '^', '&', '*', '('];
    if c.is_ascii_alphabetic() {
        let index = u32::from(c.to_ascii_lowercase()) - u32::from('a');
        return (LETTERS[index as usize], 65 + index, c.is_ascii_uppercase());
    }
    if let Some(digit) = c.to_digit(10).filter(|_| c.is_ascii_digit()) {
        return (DIGITS[digit as usize], 48 + digit, false);
    }
    if let Some(digit) = SHIFTED_DIGITS.iter().position(|shifted| *shifted == c) {
        let digit = u32::try_from(digit).unwrap_or_default();
        return (DIGITS[digit as usize], 48 + digit, true);
    }
    match c {
        '`' => ("Backquote", 192, false),
        '~' => ("Backquote", 192, true),
        '-' => ("Minus", 189, false),
        '_' => ("Minus", 189, true),
        '=' => ("Equal", 187, false),
        '+' => ("Equal", 187, true),
        '[' => ("BracketLeft", 219, false),
        '{' => ("BracketLeft", 219, true),
        ']' => ("BracketRight", 221, false),
        '}' => ("BracketRight", 221, true),
        '\\' => ("Backslash", 220, false),
        '|' => ("Backslash", 220, true),
        ';' => ("Semicolon", 186, false),
        ':' => ("Semicolon", 186, true),
        '\'' => ("Quote", 222, false),
        '"' => ("Quote", 222, true),
        ',' => ("Comma", 188, false),
        '<' => ("Comma", 188, true),
        '.' => ("Period", 190, false),
        '>' => ("Period", 190, true),
        '/' => ("Slash", 191, false),
        '?' => ("Slash", 191, true),
        _ => ("", 0, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(chord: &str) -> KeyPress {
        key_press(&KeyEvent::press(chord)).unwrap_or_else(|| panic!("`{chord}` reaches the page"))
    }

    const SHIFT: Modifiers = Modifiers { alt: false, ctrl: false, meta: false, shift: true };
    const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, meta: false, shift: false };

    #[test]
    fn a_letter_types_itself_on_its_physical_key() {
        assert_eq!(
            press("a"),
            KeyPress {
                key: "a".into(),
                code: "KeyA".into(),
                key_code: 65,
                text: Some("a".into()),
                modifiers: Modifiers::default()
            }
        );
        assert_eq!(
            press("shift+z"),
            KeyPress { key: "Z".into(), code: "KeyZ".into(), key_code: 90, text: Some("Z".into()), modifiers: SHIFT }
        );
        let digit = press("7");
        assert_eq!((digit.code.as_str(), digit.key_code, digit.text.as_deref()), ("Digit7", 55, Some("7")));
        let space = press("space");
        assert_eq!((space.key.as_str(), space.code.as_str(), space.key_code), (" ", "Space", 32));
        assert_eq!(space.text.as_deref(), Some(" "));
    }

    #[test]
    fn a_shifted_character_is_shift_on_its_us_key_and_a_letter_beyond_ascii_has_no_key() {
        let question = press("?");
        assert_eq!((question.code.as_str(), question.key_code, question.modifiers), ("Slash", 191, SHIFT));
        assert_eq!(question.text.as_deref(), Some("?"));
        let bang = press("!");
        assert_eq!((bang.code.as_str(), bang.key_code, bang.modifiers.shift), ("Digit1", 49, true));
        let minus = press("-");
        assert_eq!((minus.code.as_str(), minus.key_code, minus.modifiers.shift), ("Minus", 189, false));
        let turkish = press("ş");
        assert_eq!((turkish.key.as_str(), turkish.code.as_str(), turkish.key_code), ("ş", "", 0));
        assert_eq!(turkish.text.as_deref(), Some("ş"), "the page still gets what was typed");
    }

    #[test]
    fn with_ctrl_or_alt_a_key_is_a_shortcut_that_types_nothing() {
        assert_eq!(
            press("ctrl+a"),
            KeyPress { key: "a".into(), code: "KeyA".into(), key_code: 65, text: None, modifiers: CTRL }
        );
        let alt = press("alt+x");
        assert_eq!((alt.text, alt.modifiers.alt), (None, true));
        let ctrl_shift = press("ctrl+shift+k");
        assert_eq!((ctrl_shift.key.as_str(), ctrl_shift.text), ("K", None));
        assert_eq!(press("ctrl+space").text, None);
        assert_eq!(press("ctrl+enter").text, None);
    }

    #[test]
    fn enter_types_a_return_so_forms_submit() {
        let enter = press("enter");
        assert_eq!((enter.key.as_str(), enter.code.as_str(), enter.key_code), ("Enter", "Enter", 13));
        assert_eq!(enter.text.as_deref(), Some("\r"));
        assert_eq!(press("shift+enter").text.as_deref(), Some("\r"), "a textarea's new line");
    }

    #[test]
    fn keys_that_act_go_down_raw_with_their_codes() {
        for (chord, key, key_code) in [
            ("tab", "Tab", 9),
            ("backspace", "Backspace", 8),
            ("esc", "Escape", 27),
            ("delete", "Delete", 46),
            ("insert", "Insert", 45),
            ("home", "Home", 36),
            ("end", "End", 35),
            ("pgup", "PageUp", 33),
            ("pgdn", "PageDown", 34),
            ("left", "ArrowLeft", 37),
            ("up", "ArrowUp", 38),
            ("right", "ArrowRight", 39),
            ("down", "ArrowDown", 40),
            ("f1", "F1", 112),
            ("f12", "F12", 123),
            ("menu", "ContextMenu", 93),
        ] {
            let got = press(chord);
            assert_eq!((got.key.as_str(), got.code.as_str(), got.key_code), (key, key, key_code), "{chord}");
            assert_eq!(got.text, None, "{chord} types nothing");
        }
        let back = press("shift+tab");
        assert_eq!((back.key.as_str(), back.modifiers), ("Tab", SHIFT), "shift+Tab goes back through the page");
    }

    #[test]
    fn a_held_key_repeats_and_a_release_is_not_sent() {
        let mut held = KeyEvent::press("x");
        held.kind = KeyKind::Repeat;
        assert_eq!(key_press(&held).and_then(|press| press.text).as_deref(), Some("x"));
        held.kind = KeyKind::Release;
        assert_eq!(key_press(&held), None);
    }
}

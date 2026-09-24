//! What reaches a page from the mouse and the keyboard, in the shape the DevTools protocol's
//! `Input.dispatchMouseEvent` and `Input.dispatchKeyEvent` take it.
//!
//! Positions are CSS pixels of the page's viewport. Turning a terminal cell into a pixel, and a
//! terminal key into a [`KeyPress`], is the screen's work; this module only speaks the protocol.

use serde_json::{Value, json};

/// Modifier keys held with a click or a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    /// Alt.
    pub alt: bool,
    /// Ctrl.
    pub ctrl: bool,
    /// Meta, the Super or Command key.
    pub meta: bool,
    /// Shift.
    pub shift: bool,
}

impl Modifiers {
    /// The protocol's bit field: Alt 1, Ctrl 2, Meta 4, Shift 8.
    #[must_use]
    pub fn bits(self) -> u8 {
        u8::from(self.alt) | u8::from(self.ctrl) << 1 | u8::from(self.meta) << 2 | u8::from(self.shift) << 3
    }
}

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// The left button.
    Left,
    /// The middle button, the wheel pressed.
    Middle,
    /// The right button.
    Right,
}

impl Button {
    /// The protocol's name for it.
    fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Middle => "middle",
            Self::Right => "right",
        }
    }

    /// The protocol's bit for it among the buttons held: left 1, right 2, middle 4.
    fn held(self) -> u8 {
        match self {
            Self::Left => 1,
            Self::Right => 2,
            Self::Middle => 4,
        }
    }
}

/// What the mouse did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mouse {
    /// A button went down; `clicks` is 2 for the second press of a double click.
    Pressed {
        /// The button.
        button: Button,
        /// How many presses in a row this one is.
        clicks: u8,
    },
    /// A button came up; `clicks` as for the press it ends.
    Released {
        /// The button.
        button: Button,
        /// How many presses in a row the press it ends was.
        clicks: u8,
    },
    /// The pointer moved, with this button held while dragging.
    Moved {
        /// The button held, if any.
        held: Option<Button>,
    },
    /// The wheel turned by this many CSS pixels, across and down; down is positive.
    Wheel {
        /// Pixels across, right positive.
        dx: f64,
        /// Pixels down, down positive.
        dy: f64,
    },
}

/// The parameters of `Input.dispatchMouseEvent` for `mouse` at (`x`, `y`) in CSS pixels.
#[must_use]
pub fn mouse_params(mouse: Mouse, x: f64, y: f64, modifiers: Modifiers) -> Value {
    let base = |kind: &str, button: &str, buttons: u8, clicks: u8| {
        json!({
            "type": kind,
            "x": x,
            "y": y,
            "modifiers": modifiers.bits(),
            "button": button,
            "buttons": buttons,
            "clickCount": clicks,
        })
    };
    match mouse {
        Mouse::Pressed { button, clicks } => base("mousePressed", button.name(), button.held(), clicks),
        Mouse::Released { button, clicks } => base("mouseReleased", button.name(), 0, clicks),
        Mouse::Moved { held: Some(button) } => base("mouseMoved", button.name(), button.held(), 0),
        Mouse::Moved { held: None } => base("mouseMoved", "none", 0, 0),
        Mouse::Wheel { dx, dy } => {
            let mut params = base("mouseWheel", "none", 0, 0);
            params["deltaX"] = json!(dx);
            params["deltaY"] = json!(dy);
            params
        }
    }
}

/// A key as the page hears it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPress {
    /// The DOM `key` value: `"a"`, `"A"`, `"Enter"`, `"ArrowLeft"`.
    pub key: String,
    /// The DOM `code` value, the physical key: `"KeyA"`, `"Enter"`, `"ArrowLeft"`.
    pub code: String,
    /// The Windows virtual key code the page's `keyCode` reports: 65 for A, 13 for Enter.
    pub key_code: u32,
    /// What the key types, when it types something and no Ctrl, Alt or Meta is held; `None` for
    /// keys that act rather than type, which go down as raw keys so the page's shortcuts see them.
    pub text: Option<String>,
    /// The modifiers held.
    pub modifiers: Modifiers,
}

/// The two `Input.dispatchKeyEvent` calls a key press makes: down, then up. A key that types goes
/// down as `keyDown` with its text, so the page's listeners hear it and the text lands in the
/// field; any other goes down as `rawKeyDown`.
#[must_use]
pub fn key_params(press: &KeyPress) -> [Value; 2] {
    let common = |kind: &str| {
        json!({
            "type": kind,
            "key": press.key,
            "code": press.code,
            "windowsVirtualKeyCode": press.key_code,
            "nativeVirtualKeyCode": press.key_code,
            "modifiers": press.modifiers.bits(),
        })
    };
    let down = match &press.text {
        Some(text) => {
            let mut down = common("keyDown");
            down["text"] = json!(text);
            down["unmodifiedText"] = json!(text);
            down
        }
        None => common("rawKeyDown"),
    };
    [down, common("keyUp")]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_are_the_protocols_bits() {
        assert_eq!(Modifiers::default().bits(), 0);
        assert_eq!(Modifiers { alt: true, ..Modifiers::default() }.bits(), 1);
        assert_eq!(Modifiers { ctrl: true, shift: true, ..Modifiers::default() }.bits(), 10);
        assert_eq!(Modifiers { alt: true, ctrl: true, meta: true, shift: true }.bits(), 15);
    }

    #[test]
    fn a_press_holds_its_button_and_a_release_holds_none() {
        let press = mouse_params(Mouse::Pressed { button: Button::Right, clicks: 1 }, 12.5, 40.0, Modifiers::default());
        assert_eq!(press["type"], "mousePressed");
        assert_eq!(press["button"], "right");
        assert_eq!(press["buttons"], 2);
        assert_eq!(press["clickCount"], 1);
        assert_eq!(press["x"], 12.5);
        let release =
            mouse_params(Mouse::Released { button: Button::Middle, clicks: 2 }, 0.0, 0.0, Modifiers::default());
        assert_eq!(release["type"], "mouseReleased");
        assert_eq!(release["button"], "middle");
        assert_eq!(release["buttons"], 0);
        assert_eq!(release["clickCount"], 2);
    }

    #[test]
    fn a_drag_holds_its_button_and_the_wheel_carries_its_deltas() {
        let drag = mouse_params(Mouse::Moved { held: Some(Button::Left) }, 1.0, 2.0, Modifiers::default());
        assert_eq!(drag["type"], "mouseMoved");
        assert_eq!(drag["buttons"], 1);
        let hover = mouse_params(Mouse::Moved { held: None }, 1.0, 2.0, Modifiers::default());
        assert_eq!(hover["button"], "none");
        let wheel = mouse_params(Mouse::Wheel { dx: 0.0, dy: -60.0 }, 5.0, 6.0, Modifiers::default());
        assert_eq!(wheel["type"], "mouseWheel");
        assert_eq!(wheel["deltaY"], -60.0);
        assert_eq!(wheel["deltaX"], 0.0);
    }

    #[test]
    fn a_typing_key_goes_down_with_its_text_and_an_acting_key_goes_down_raw() {
        let shift = Modifiers { shift: true, ..Modifiers::default() };
        let typed =
            KeyPress { key: "A".into(), code: "KeyA".into(), key_code: 65, text: Some("A".into()), modifiers: shift };
        let [down, up] = key_params(&typed);
        assert_eq!(down["type"], "keyDown");
        assert_eq!(down["text"], "A");
        assert_eq!(down["modifiers"], 8);
        assert_eq!(up["type"], "keyUp");
        assert!(up.get("text").is_none(), "only the press types");
        let enter = KeyPress {
            key: "Enter".into(),
            code: "Enter".into(),
            key_code: 13,
            text: None,
            modifiers: Modifiers::default(),
        };
        let [down, up] = key_params(&enter);
        assert_eq!(down["type"], "rawKeyDown");
        assert_eq!(down["windowsVirtualKeyCode"], 13);
        assert!(down.get("text").is_none());
        assert_eq!(up["key"], "Enter");
    }
}

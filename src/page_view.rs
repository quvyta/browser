//! The page area: the picture Chromium draws, and the mouse, the keys and pastes that go back to
//! the page.

use std::time::Duration;

use qframe::event::{Event, MouseButton, MouseEvent, MouseKind};
use qframe::geometry::{Rect, Size};
use qframe::keymap::Scope;
use qframe::widget::{EventCx, MeasureCx, PaintCx, Widget};
use qframe::widgets::{Fit, Image, ImageData};

use crate::engine::input::{Button, KeyPress, Modifiers, Mouse};
use crate::keys;

/// Two presses of the same button on the same cell closer together than this are a double
/// click, as on a desktop; a third makes a triple click, which selects a paragraph.
const MULTI_CLICK: Duration = Duration::from_millis(400);

/// How far one step of the wheel scrolls, in CSS pixels: three lines of twenty, close to what
/// desktop browsers scroll.
const WHEEL_STEP: f64 = 60.0;

/// What the person did to the page, with the cell it happened on counted from the page area's
/// top left corner.
#[derive(Debug, Clone, PartialEq)]
pub enum PageInput {
    /// The mouse pressed, released, dragged, moved or scrolled on the page.
    Mouse {
        /// What the mouse did.
        mouse: Mouse,
        /// The column, from the page area's left edge.
        column: u16,
        /// The row, from the page area's top edge.
        row: u16,
        /// The modifiers held.
        modifiers: Modifiers,
    },
    /// A key pressed while the page had focus.
    Key(KeyPress),
    /// Text pasted while the page had focus: the terminal's own paste, or the clipboard the
    /// paste key read.
    Paste(String),
}

/// The page area: draws the latest picture of the page and turns what the person does over it
/// into [`PageInput`].
///
/// The picture is drawn with the framework's [`Image`], fitted inside the area; with no picture
/// yet the area is the theme's `canvas` ground. The area takes focus when it is pressed, and
/// while it has focus every key and paste goes to the page, Tab and Shift+Tab too, since that is
/// how a page moves between its fields. Only qbrowser's own keys stay with qbrowser: those bound
/// to an application action in the keymap (new tab, the address, back, reload…), the global
/// `quit`, and the global `paste`, which the runtime answers by reading the clipboard and handing
/// the text back as a paste. `cancel` is the exception among the application's keys: Esc belongs
/// to the page, which closes its own dialogs with it.
pub struct PageView<Msg> {
    picture: Option<ImageData>,
    on_input: Option<Box<dyn Fn(PageInput) -> Msg>>,
}

impl<Msg> PageView<Msg> {
    /// A page area showing `picture`, or the bare ground when there is none yet.
    #[must_use]
    pub fn new(picture: Option<&ImageData>) -> Self {
        Self { picture: picture.cloned(), on_input: None }
    }

    /// The message sent for every input the page receives.
    #[must_use]
    pub fn on_input(mut self, message: impl Fn(PageInput) -> Msg + 'static) -> Self {
        self.on_input = Some(Box::new(message));
        self
    }

    /// Sends `input` to the application; `false` when nobody listens, so the event goes on.
    fn send(&self, cx: &mut EventCx<'_, Msg>, input: PageInput) -> bool {
        let Some(message) = &self.on_input else { return false };
        cx.emit(message(input));
        true
    }
}

/// Whether a keymap action stays with qbrowser rather than going to the page. See [`PageView`].
fn leaves_to_the_app(scope: Scope, action: &str) -> bool {
    match scope {
        Scope::App => action != "cancel",
        Scope::Global => matches!(action, "quit" | "paste"),
    }
}

/// The last press, to count the presses of a double or triple click.
#[derive(Default)]
struct Presses {
    /// The button, the cell, when it went down and how many presses in a row it was.
    last: Option<(Button, u16, u16, Duration, u8)>,
}

fn button(button: MouseButton) -> Button {
    match button {
        MouseButton::Left => Button::Left,
        MouseButton::Middle => Button::Middle,
        MouseButton::Right => Button::Right,
    }
}

/// The cell of `mouse` counted from `area`'s corner. A drag that leaves the area is held at its
/// edge, where the page's own selection or slider stops too.
fn cell(mouse: &MouseEvent, area: Rect) -> (u16, u16) {
    let along = |at: i32, start: i32, length: u16| {
        let last = i32::from(length.saturating_sub(1));
        u16::try_from((at - start).clamp(0, last)).unwrap_or_default()
    };
    (along(mouse.x, area.x, area.width), along(mouse.y, area.y, area.height))
}

impl<Msg: 'static> Widget<Msg> for PageView<Msg> {
    fn measure(&self, _cx: &mut MeasureCx<'_>, available: Size) -> Size {
        available
    }

    fn paint(&self, cx: &mut PaintCx<'_>, area: Rect) {
        let ground = cx.color("canvas");
        cx.clear(area, ground);
        if let Some(picture) = &self.picture {
            Widget::<Msg>::paint(&Image::new(picture).fit(Fit::Contain), cx, area);
        }
        cx.register_hit(area);
        // Hovering changes pages: menus open, links light up and the pointer's shape follows.
        cx.track_pointer_moves();
        // Every Enter and Space is typed into the page, however fast they come.
        cx.takes_text();
    }

    fn event(&self, cx: &mut EventCx<'_, Msg>, event: &Event) -> bool {
        match event {
            Event::Key(key) => {
                if let Some((scope, action)) = cx.env().keymap().action_for(key.chord)
                    && leaves_to_the_app(scope, action)
                {
                    return false;
                }
                keys::key_press(key).is_some_and(|press| self.send(cx, PageInput::Key(press)))
            }
            Event::Paste(text) => self.send(cx, PageInput::Paste(text.clone())),
            Event::Mouse(mouse_event) => {
                let (column, row) = cell(mouse_event, cx.area());
                let mods = mouse_event.mods;
                let modifiers = Modifiers { alt: mods.alt, ctrl: mods.ctrl, meta: false, shift: mods.shift };
                let mouse = match mouse_event.kind {
                    MouseKind::Down(pressed) => {
                        // Keeps the drag and the release coming here when the pointer leaves the
                        // area with the button held, as a selection on the page does.
                        cx.capture_pointer();
                        let pressed = button(pressed);
                        let now = cx.now();
                        let presses = cx.memory::<Presses>();
                        let clicks = match presses.last {
                            Some((last, at_column, at_row, at, count))
                                if last == pressed
                                    && (at_column, at_row) == (column, row)
                                    && now.saturating_sub(at) < MULTI_CLICK =>
                            {
                                count % 3 + 1
                            }
                            _ => 1,
                        };
                        presses.last = Some((pressed, column, row, now, clicks));
                        Mouse::Pressed { button: pressed, clicks }
                    }
                    MouseKind::Up(released) => {
                        let released = button(released);
                        let clicks = cx
                            .memory::<Presses>()
                            .last
                            .filter(|(last, ..)| *last == released)
                            .map_or(1, |(.., count)| count);
                        Mouse::Released { button: released, clicks }
                    }
                    MouseKind::Drag(held) => Mouse::Moved { held: Some(button(held)) },
                    MouseKind::Moved => Mouse::Moved { held: None },
                    MouseKind::ScrollUp => Mouse::Wheel { dx: 0.0, dy: -WHEEL_STEP },
                    MouseKind::ScrollDown => Mouse::Wheel { dx: 0.0, dy: WHEEL_STEP },
                };
                self.send(cx, PageInput::Mouse { mouse, column, row, modifiers })
            }
            Event::PointerOutside => false,
        }
    }

    fn focusable(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests;

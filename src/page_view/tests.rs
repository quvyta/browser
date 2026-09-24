//! The page area driven from where the person clicks, types and pastes, inside a small screen:
//! two rows of header above it, three columns of side bar to its left and more to its right and
//! below, so every position it reports is counted from its own corner and not the screen's.

use std::time::Duration;

use qframe::color::{ColorDepth, Rgb};
use qframe::event::{MouseButton, MouseKind};
use qframe::graphics::Graphics;
use qframe::icons::GlyphMode;
use qframe::prelude::*;
use qframe::widget::{MeasureCx, PaintCx, Widget};
use qframe::widgets::ImageData;

use super::{PageInput, PageView};
use crate::engine::input::{Button, KeyPress, Modifiers, Mouse};
use crate::locales;

/// The page area on the test screen: its top left corner and its size.
const LEFT: i32 = 3;
const TOP: i32 = 2;
const WIDTH: i32 = 35;
const HEIGHT: i32 = 9;

/// A container's `surface` ground under the page area, so the page's own ground shows.
struct Surface;

impl Widget<Msg> for Surface {
    fn measure(&self, _cx: &mut MeasureCx<'_>, available: Size) -> Size {
        available
    }

    fn paint(&self, cx: &mut PaintCx<'_>, area: Rect) {
        let surface = cx.color("surface");
        cx.clear(area, surface);
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Msg {
    Page(PageInput),
    Action(String),
}

#[derive(Default)]
struct Screen {
    picture: Option<ImageData>,
    page: Vec<PageInput>,
    actions: Vec<String>,
}

impl Screen {
    /// The keys the page received.
    fn keys(&self) -> Vec<&KeyPress> {
        self.page
            .iter()
            .filter_map(|input| match input {
                PageInput::Key(press) => Some(press),
                _ => None,
            })
            .collect()
    }

    /// What the mouse did on the page, with where.
    fn mouse(&self) -> Vec<(Mouse, u16, u16)> {
        self.page
            .iter()
            .filter_map(|input| match input {
                PageInput::Mouse { mouse, column, row, .. } => Some((*mouse, *column, *row)),
                _ => None,
            })
            .collect()
    }
}

impl App for Screen {
    type Msg = Msg;

    fn update(&mut self, msg: Msg) -> Command<Msg> {
        match msg {
            Msg::Page(input) => self.page.push(input),
            Msg::Action(name) => self.actions.push(name),
        }
        Command::none()
    }

    fn view(&self, ui: &mut View<'_, Msg>) {
        ui.column(|ui| {
            ui.add(Text::new("header")).height(Length::Cells(2));
            ui.row(|ui| {
                ui.add(Text::new("bar")).width(Length::Cells(3));
                ui.stack(|ui| {
                    ui.add(Surface).fill();
                    ui.add(PageView::new(self.picture.as_ref()).on_input(Msg::Page)).id("page").fill();
                })
                .fill();
                ui.add(Text::new("ab")).width(Length::Cells(2));
            })
            .fill();
            ui.add(Text::new("footer")).height(Length::Cells(1));
        })
        .fill();
    }

    fn action(&self, name: &str) -> Option<Msg> {
        Some(Msg::Action(name.to_owned()))
    }
}

fn screen() -> Harness<Screen> {
    showing(None)
}

/// The test screen with `picture` on the page, drawn the same wherever the tests run: the
/// environment's own terminal (a C locale's ASCII glyphs, 256 colours) would decide otherwise.
fn showing(picture: Option<ImageData>) -> Harness<Screen> {
    let mut screen = Harness::with_env(Screen { picture, ..Screen::default() }, locales::env(), 40, 12);
    screen.set_depth(ColorDepth::TrueColor).set_glyph_mode(GlyphMode::Unicode).set_graphics(Graphics::HalfBlock);
    screen
}

/// A screen whose page area has focus from a click on it: that click's press and release are the
/// page's first two inputs.
fn focused() -> Harness<Screen> {
    let mut screen = screen();
    screen.click(LEFT + 1, TOP + 1);
    assert!(screen.is_focused("page"), "a click on the page gives it focus");
    screen
}

fn typed(screen: &Harness<Screen>) -> String {
    screen.app().keys().iter().filter_map(|press| press.text.as_deref()).collect()
}

#[test]
fn a_click_reaches_the_page_at_its_own_cell_and_gives_it_focus() {
    let mut screen = screen();
    assert!(!screen.is_focused("page"));
    screen.click(LEFT + 5, TOP + 4);
    assert!(screen.is_focused("page"));
    assert_eq!(
        screen.app().mouse(),
        [
            (Mouse::Pressed { button: Button::Left, clicks: 1 }, 5, 4),
            (Mouse::Released { button: Button::Left, clicks: 1 }, 5, 4),
        ]
    );
    screen.click(LEFT, TOP);
    assert_eq!(screen.app().mouse()[2], (Mouse::Pressed { button: Button::Left, clicks: 1 }, 0, 0));
}

#[test]
fn a_click_on_the_header_or_the_side_bar_is_not_the_pages() {
    let mut screen = screen();
    screen.click(LEFT + 5, TOP - 1).click(LEFT - 1, TOP + 3).click(LEFT + WIDTH, TOP + 3).click(LEFT + 5, TOP + HEIGHT);
    assert!(screen.app().page.is_empty());
    assert!(!screen.is_focused("page"));
}

#[test]
fn a_second_press_on_the_same_cell_soon_after_is_a_double_click() {
    let mut screen = screen();
    screen.click(LEFT + 2, TOP + 2).click(LEFT + 2, TOP + 2);
    assert_eq!(
        screen.app().mouse()[2..],
        [
            (Mouse::Pressed { button: Button::Left, clicks: 2 }, 2, 2),
            (Mouse::Released { button: Button::Left, clicks: 2 }, 2, 2),
        ]
    );
    // Later, or elsewhere, a press starts counting again.
    screen.advance(Duration::from_millis(500)).click(LEFT + 2, TOP + 2).click(LEFT + 3, TOP + 2);
    assert_eq!(screen.app().mouse()[4].0, Mouse::Pressed { button: Button::Left, clicks: 1 });
    assert_eq!(screen.app().mouse()[6].0, Mouse::Pressed { button: Button::Left, clicks: 1 });
}

#[test]
fn other_buttons_drags_moves_and_the_wheel_reach_the_page() {
    let mut screen = screen();
    screen.mouse(MouseKind::Down(MouseButton::Right), LEFT + 1, TOP + 1);
    screen.mouse(MouseKind::Up(MouseButton::Right), LEFT + 1, TOP + 1);
    screen.drag((LEFT + 1, TOP + 1), (LEFT + 6, TOP + 3));
    screen.hover(LEFT + 7, TOP + 5);
    screen.mouse(MouseKind::ScrollDown, LEFT + 7, TOP + 5).mouse(MouseKind::ScrollUp, LEFT + 7, TOP + 5);
    let mouse = screen.app().mouse();
    assert_eq!(mouse[0], (Mouse::Pressed { button: Button::Right, clicks: 1 }, 1, 1));
    assert_eq!(mouse[3], (Mouse::Moved { held: Some(Button::Left) }, 6, 3), "a drag holds its button");
    assert_eq!(mouse[5], (Mouse::Moved { held: None }, 7, 5), "the pointer's moves reach the page");
    assert_eq!(mouse[6], (Mouse::Wheel { dx: 0.0, dy: 60.0 }, 7, 5));
    assert_eq!(mouse[7], (Mouse::Wheel { dx: 0.0, dy: -60.0 }, 7, 5));
}

#[test]
fn a_drag_that_leaves_the_page_stays_on_its_edge() {
    let mut screen = screen();
    screen.drag((LEFT + 2, TOP + 2), (0, 0));
    assert_eq!(screen.app().mouse()[1], (Mouse::Moved { held: Some(Button::Left) }, 0, 0));
    assert_eq!(screen.app().mouse()[2], (Mouse::Released { button: Button::Left, clicks: 1 }, 0, 0));
    let (right, bottom) = (u16::try_from(WIDTH - 1).unwrap(), u16::try_from(HEIGHT - 1).unwrap());
    screen.drag((LEFT + 2, TOP + 2), (39, 11));
    assert_eq!(screen.app().mouse()[4], (Mouse::Moved { held: Some(Button::Left) }, right, bottom));
}

#[test]
fn typing_reaches_the_page_with_its_text_and_tab_stays_there() {
    let mut screen = focused();
    screen.type_text("Hi ?").press("enter").press("tab").press("shift+tab");
    assert_eq!(typed(&screen), "Hi ?\r");
    let keys: Vec<&str> = screen.app().keys().iter().map(|press| press.key.as_str()).collect();
    assert_eq!(keys, ["H", "i", " ", "?", "Enter", "Tab", "Tab"]);
    assert!(screen.is_focused("page"), "Tab moves through the page, not away from it");
    assert!(screen.app().actions.is_empty(), "`?` is typed, not the help");
}

#[test]
fn keys_the_page_uses_as_shortcuts_reach_it_without_text() {
    let mut screen = focused();
    screen.press("ctrl+a").press("esc").press("f12");
    let keys: Vec<(&str, Option<&str>, Modifiers)> =
        screen.app().keys().iter().map(|press| (press.key.as_str(), press.text.as_deref(), press.modifiers)).collect();
    let ctrl = Modifiers { ctrl: true, ..Modifiers::default() };
    assert_eq!(keys, [("a", None, ctrl), ("Escape", None, Modifiers::default()), ("F12", None, Modifiers::default())]);
}

#[test]
fn qbrowsers_own_keys_are_not_taken_by_the_page() {
    let mut screen = focused();
    for chord in ["ctrl+t", "ctrl+w", "ctrl+pgdn", "ctrl+pgup", "ctrl+l", "f6", "alt+left", "alt+right", "ctrl+r", "f5"]
    {
        screen.press(chord);
    }
    assert!(screen.app().keys().is_empty(), "the page got {:?}", screen.app().keys());
    assert_eq!(
        screen.app().actions,
        ["new-tab", "close-tab", "next-tab", "prev-tab", "location", "location", "back", "forward", "reload", "reload"]
    );
    screen.press("ctrl+q");
    assert!(screen.app().keys().is_empty());
    assert!(screen.quit_requested(), "ctrl+q quits from the page too");
}

#[test]
fn a_paste_and_the_paste_key_reach_the_page_as_text() {
    let mut screen = focused();
    screen.paste("from the terminal");
    screen.set_system_clipboard(Some("from the clipboard")).press("ctrl+v");
    assert_eq!(
        screen.app().page[2..],
        [PageInput::Paste("from the terminal".into()), PageInput::Paste("from the clipboard".into())]
    );
}

#[test]
fn the_page_is_its_picture_and_the_ground_before_there_is_one() {
    let (x, y) = (u16::try_from(LEFT + 4).unwrap(), u16::try_from(TOP + 4).unwrap());
    let empty = screen();
    let ground = empty.env().theme().color("canvas").expect("the theme has a canvas");
    assert_eq!(empty.bg(x, y), Some(ground));
    // A red picture of the area's shape, two pixels to a cell's height.
    let red = Rgb::new(200, 30, 30);
    let (width, height) = (u32::try_from(WIDTH).unwrap(), u32::try_from(HEIGHT * 2).unwrap());
    let picture = ImageData::from_rgb(width, height, &[200, 30, 30].repeat((width * height) as usize)).expect("pixels");
    let shown = showing(Some(picture));
    assert_eq!((shown.fg(x, y), shown.bg(x, y)), (Some(red), Some(red)));
}

#[test]
fn keys_that_arrive_together_all_reach_the_page() {
    // A paste without bracketed paste, or a slow connection, brings Enter and Space closer
    // together than a held key would; each is still typed.
    let mut screen = focused();
    let burst: Vec<Event> =
        ["space", "space", "enter", "enter"].iter().map(|chord| Event::Key(KeyEvent::press(chord))).collect();
    screen.events(&burst);
    assert_eq!(typed(&screen), "  \r\r");
}

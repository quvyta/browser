//! A page's drop-down list from the person's hands: a click on a select opens qbrow's own list of
//! its options, choosing one is heard by the page, and every other press still reaches the page.

use qframe::event::{MouseButton, MouseKind};
use qframe::icons::GlyphMode;
use qframe::prelude::Harness;
use serde_json::json;

use super::super::Browser;
use super::{Scratch, Slot, cell_of, eval, open, page, until};

/// The screen on the form, once its document can answer.
fn opened(scratch: &Scratch) -> Harness<Browser> {
    let mut h = open(scratch, &page("/select"));
    until(&mut h, "the form's script", |h| {
        let browser = h.app();
        let (Some(engine), Some(tab)) = (browser.engine(), browser.active_tab()) else { return false };
        engine.evaluate(tab, "window.seen !== undefined", super::PATIENCE).is_ok_and(|v| v == json!(true))
    });
    h
}

/// Whether qbrow's list of the select's options is on the screen. The page's picture has no text
/// in it, so the words of the options can only come from the list.
fn listed(h: &Harness<Browser>) -> bool {
    let screen = h.screen();
    ["Apple", "Banana", "Lemon", "Lime"].iter().all(|word| screen.contains(word))
}

/// Opens the select's list with a click on it.
fn click_select(h: &mut Harness<Browser>) -> (i32, i32) {
    let (x, y) = cell_of(h, "#fruit");
    h.click(x, y);
    until(h, "the list of options", listed);
    (x, y)
}

/// What the page's change listener wrote.
fn seen(h: &Harness<Browser>) -> serde_json::Value {
    eval(h, "seen.join(' ')")
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_click_on_a_select_shows_its_options_in_qbrows_own_list() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = opened(&scratch);
    let (_, y) = click_select(&mut h);
    let (apple_x, apple_y) = h.find("Apple").unwrap();
    assert!(apple_y > y, "the list opens under the select:\n{}", h.screen());
    // The select stands from the page's column 4 to its column 24.
    assert!((4..24).contains(&apple_x), "inside the select's own columns:\n{}", h.screen());
    assert!(h.screen().contains("Citrus"), "the group's heading is on the list:\n{}", h.screen());
    let check = h.env().icons().glyph("check").into_owned();
    let banana = h.screen().lines().nth(usize::try_from(h.find("Banana").unwrap().1).unwrap()).unwrap().to_owned();
    assert!(banana.contains(&check), "the option the select holds is marked:\n{}", h.screen());
    let apple = h.screen().lines().nth(usize::try_from(apple_y).unwrap()).unwrap().to_owned();
    assert!(!apple.contains(&check), "and only that one:\n{}", h.screen());
    assert_eq!(eval(&h, "pressed"), json!(0), "the press went to the list, not to the page");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn choosing_an_option_with_a_click_sets_it_and_the_page_hears() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = opened(&scratch);
    click_select(&mut h);
    let (x, y) = h.find("Lemon").unwrap();
    h.click(x + 1, y);
    until(&mut h, "the page's listener", |h| seen(h) == json!("input lemon"));
    assert_eq!(eval(&h, "fruit.value"), json!("lemon"), "the select holds it");
    assert!(!listed(&h), "and the list is closed:\n{}", h.screen());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_disabled_option_and_a_heading_are_not_chosen() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = opened(&scratch);
    click_select(&mut h);
    for word in ["Cherry", "Citrus"] {
        let (x, y) = h.find(word).unwrap();
        h.click(x + 1, y);
        assert!(listed(&h), "{word} cannot be chosen and the list stays:\n{}", h.screen());
    }
    assert_eq!(eval(&h, "fruit.value"), json!("banana"), "the select still holds what it held");
    assert_eq!(seen(&h), json!(""), "and the page heard nothing");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn esc_closes_the_list_and_the_value_stays() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = opened(&scratch);
    click_select(&mut h);
    h.press("up");
    h.press("esc");
    assert!(!listed(&h), "esc closed the list:\n{}", h.screen());
    assert_eq!(eval(&h, "fruit.value"), json!("banana"), "with nothing chosen");
    assert_eq!(seen(&h), json!(""));
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_keyboard_opens_the_list_of_a_focused_select_and_chooses_in_it() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = opened(&scratch);
    // Tab from the page brings the keyboard to the select, the first thing on it that takes it.
    h.press("tab");
    until(&mut h, "the select focused", |h| eval(h, "document.activeElement.id") == json!("fruit"));
    for key in ["alt+down", "f4", "space"] {
        h.press(key);
        until(&mut h, &format!("the list on {key}"), listed);
        h.press("esc");
        assert!(!listed(&h), "esc closed it again:\n{}", h.screen());
    }
    // A letter jumps to the option it starts, past the group's heading, and Enter chooses it.
    h.press("alt+down");
    until(&mut h, "the list again", listed);
    h.press("l").press("l").press("enter");
    until(&mut h, "the page's listener", |h| seen(h) == json!("input lime"));
    assert_eq!(eval(&h, "fruit.value"), json!("lime"));
    // A plain arrow is the page's: Chromium changes the value without a list, as on a desktop.
    h.press("up");
    until(&mut h, "the value changed by the page", |h| eval(h, "fruit.value") == json!("lemon"));
    assert!(!listed(&h), "no list for a plain arrow:\n{}", h.screen());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_click_on_a_list_that_shows_its_own_rows_goes_to_the_page() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = opened(&scratch);
    let (x, y) = cell_of(&h, "#many");
    h.click(x, y);
    until(&mut h, "the page's own choice", |h| eval(h, "many.selectedOptions.length") == json!(1));
    assert!(!h.screen().contains("Two"), "qbrow drew no list of its own:\n{}", h.screen());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_pages_own_scripts_cannot_see_the_question() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = opened(&scratch);
    click_select(&mut h);
    assert_eq!(eval(&h, "spied"), json!(0), "the page's functions were never called");
    assert_eq!(
        eval(&h, "typeof window.qbrowserSelect"),
        json!("undefined"),
        "and what qbrow keeps is not in its world"
    );
    let (x, y) = h.find("Apple").unwrap();
    h.click(x + 1, y);
    until(&mut h, "the page's listener", |h| seen(h) == json!("input apple"));
    assert_eq!(eval(&h, "spied"), json!(0), "nor when the choice was set");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_click_outside_closes_the_list_and_chooses_nothing() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = opened(&scratch);
    click_select(&mut h);
    let (x, y) = cell_of(&h, "#go");
    h.click(x, y);
    assert!(!listed(&h), "the click closed the list:\n{}", h.screen());
    assert_eq!(eval(&h, "fruit.value"), json!("banana"), "and chose nothing");
    // The page area opened the list, and the framework lets a press on a layer's opener only
    // close it; so today the button needs a second click. The framework's request 19 makes the
    // first one reach the page too, and this test then asks for it.
    h.click(x, y);
    until(&mut h, "the button pressed", |h| eval(h, "went") == json!(1));
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_double_click_elsewhere_still_reaches_the_page_as_one() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = opened(&scratch);
    let (x, y) = cell_of(&h, "#words");
    // Two presses on one cell with no time between them: the second is the second of a pair, and
    // the question asked before each must not break the pair.
    h.click(x, y).click(x, y);
    until(&mut h, "the page's double click", |h| eval(h, "doubled") == json!(1));
    assert!(!listed(&h), "no list where there is no select:\n{}", h.screen());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn narrow_screens_and_ascii_keep_the_list_usable() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = opened(&scratch);
    h.resize(40, 20).set_glyph_mode(GlyphMode::Ascii);
    until(&mut h, "the narrow window", |h| h.buffer().area.width == 40);
    click_select(&mut h);
    let screen = h.screen();
    for (row, line) in screen.lines().enumerate() {
        assert!(line.chars().count() <= 40, "row {row} is no wider than the screen:\n{screen}");
        let drawn = line.chars().find(|c| !(c.is_ascii_graphic() || *c == ' '));
        assert!(drawn.is_none(), "row {row} draws {drawn:?}, which is not printable ASCII:\n{screen}");
    }
    let (x, y) = h.find("Lime").unwrap();
    h.mouse(MouseKind::Down(MouseButton::Left), x + 1, y);
    h.mouse(MouseKind::Up(MouseButton::Left), x + 1, y);
    until(&mut h, "the page's listener", |h| seen(h) == json!("input lime"));
}

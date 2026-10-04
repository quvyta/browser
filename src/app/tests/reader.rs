//! Reading mode from where a person's hands reach it: F9 and the toolbar's button open the page's
//! own text where its picture was and close it again, Esc closes it while it has the keyboard,
//! another tab keeps the reading of its own, a page with nothing to read says so and its button
//! brings the page back, a terminal that draws no picture has a button to read with, and the
//! page's own scripts never learn that any of it happened.

use std::time::Duration;

use qframe::graphics::Graphics;
use qframe::icons::GlyphMode;
use qframe::runtime::Harness;
use serde_json::json;

use super::{HEIGHT, Scratch, Slot, click_icon, find_in_row, open, page, until};
use crate::app::Browser;

/// The page's own words, as the reading shows them.
const HEADING: &str = "The page's own words";

/// The plain page's own words, the ones the narrow terminal's screen has to hold whole.
const NARROW: &str = "Notes in plain words";

/// Words of the page's furniture: they are on the page, both inside the article and around it,
/// but they are not what it has to say.
const FURNITURE: [&str; 5] = ["Sign in", "Contents", "Newsletter", "Copyright nobody", "Related reading"];

/// A cell of the page area, where the pointer is put while a toolbar button's colours are read.
const STILL: (i32, i32) = (2, 4);

/// How long the theme's press flash takes to finish, so that a button's resting colours are the
/// ones a test reads.
const SETTLED: Duration = Duration::from_millis(100);

/// The screen's words in one line, so that a sentence wrapped over several rows can be read as
/// the page wrote it.
fn words(h: &Harness<Browser>) -> String {
    h.screen().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The words of `text` in one line, to look for among [`words`].
fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The column a piece of the reading starts at, in whichever row of the screen it is on.
fn column_of(h: &Harness<Browser>, said: &str) -> Option<i32> {
    (0..h.screen().lines().count()).find_map(|row| find_in_row(h, said, row).map(|(x, _)| x))
}

/// Fails with the screen when a cell holds a character outside printable ASCII.
fn only_ascii(h: &Harness<Browser>, what: &str) {
    let buffer = h.buffer();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let symbol = buffer[(x, y)].symbol();
            assert!(
                symbol.chars().all(|c| c.is_ascii_graphic() || c == ' '),
                "{what}: `{symbol}` at {x},{y} is not ASCII printable:\n{}",
                h.screen()
            );
        }
    }
}

/// Opens the reading of the tab on screen with `key` and waits for the page's own words.
fn read_with(h: &mut Harness<Browser>, key: &str, said: &str) {
    h.press(key);
    until(h, &format!("`{said}` with {key}"), |h| h.screen().contains(said));
}

/// Whether the reading button stands out from the toolbar's other buttons: it is drawn selected
/// while the reading is open, and the settings gear beside it never is.
///
/// The pointer is put on the page and the clock moved on first: a button under the pointer, or one
/// whose press is still flashing, is drawn in the same tones whether it is selected or not.
fn drawn_while_reading(h: &mut Harness<Browser>) -> bool {
    h.hover(STILL.0, STILL.1).advance(SETTLED);
    let ink = |icon: &str| {
        let (x, y) = find_in_row(h, &h.env().icons().glyph(icon), 1)?;
        h.fg(u16::try_from(x).ok()?, u16::try_from(y).ok()?)
    };
    let reader = ink("browser.reader");
    reader.is_some() && reader != ink("settings")
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_reading_shows_the_articles_own_words_and_leaves_its_furniture_out() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/article"));
    read_with(&mut h, "f9", HEADING);

    for said in [
        HEADING,
        "A browser draws a page as a picture",
        "strong words",
        "The second item",
        "A line worth keeping for its own sake.",
        "Key · What it opens",
        "reader · reading mode",
    ] {
        assert!(h.screen().contains(said), "the reading shows `{said}`:\n{}", h.screen());
    }
    // A nested list stays deeper than the list it is nested in, or the page's shape is lost.
    let nested = column_of(&h, "and an item nested inside it");
    let beside = column_of(&h, "The second item");
    assert!(
        nested.zip(beside).is_some_and(|(nested, beside)| nested > beside),
        "a nested item is drawn deeper than the item beside it ({nested:?} against {beside:?}):\n{}",
        h.screen()
    );
    // A code block keeps its lines and its indentation, which are part of what code says.
    assert!(h.screen().contains("fn main() {"), "the reading shows the code's first line:\n{}", h.screen());
    assert!(h.screen().contains("    print(answer);"), "the reading shows the code's indentation:\n{}", h.screen());
    // The page's own title is above its words.
    let title = "Reading in the terminal";
    assert!(
        (2..HEIGHT as usize).any(|row| find_in_row(&h, title, row).is_some()),
        "the reading shows the page's title above its words:\n{}",
        h.screen()
    );

    for said in FURNITURE {
        assert!(!h.screen().contains(said), "the reading does not show `{said}`:\n{}", h.screen());
    }

    // A picture with words of its own is told on a line of its own, in the language on screen and
    // with the page's own words for it, further down than this screen reaches. End scrolls to it,
    // Home comes back, and Space takes a screenful at a time.
    let picture = format!("{}: A quiet harbour at dusk", h.env().i18n().translate("browser.reader.image", &[]));
    h.press("end");
    until(&mut h, "the picture's line at the end of the reading", |h| h.screen().contains(&picture));
    assert!(!h.screen().contains("[image:"), "the engine's own marker is not left on the screen:\n{}", h.screen());
    h.press("home");
    until(&mut h, "the top of the reading again", |h| h.screen().contains(HEADING));
    h.press("space");
    until(&mut h, "a screenful further down", |h| !h.screen().contains(HEADING));
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn f9_and_the_toolbar_button_open_and_close_the_reading_and_esc_closes_it() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/article"));

    read_with(&mut h, "f9", HEADING);
    assert!(drawn_while_reading(&mut h), "the button is drawn on while reading:\n{}", h.screen());
    h.press("f9");
    until(&mut h, "the page back", |h| h.screen().contains('▀'));
    assert!(!drawn_while_reading(&mut h), "the button is drawn off with the page:\n{}", h.screen());

    click_icon(&mut h, "browser.reader");
    until(&mut h, "the reading again", |h| h.screen().contains(HEADING));
    assert!(drawn_while_reading(&mut h), "the button is drawn on while reading:\n{}", h.screen());
    click_icon(&mut h, "browser.reader");
    until(&mut h, "the page back", |h| h.screen().contains('▀'));
    assert!(
        !drawn_while_reading(&mut h),
        "the button is drawn off again, reading {}:\n{}",
        h.app().reading(),
        h.screen()
    );

    // Esc closes the reading while it holds the keyboard, as Esc closes any mode of qbrowser.
    read_with(&mut h, "f9", HEADING);
    h.press("esc");
    until(&mut h, "the page back after Esc", |h| h.screen().contains('▀'));
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn each_tab_keeps_its_own_reading_and_a_tab_that_goes_elsewhere_loses_it() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/article"));
    read_with(&mut h, "f9", HEADING);
    assert!(h.app().reading(), "the tab on screen is being read");

    h.press("ctrl+t");
    until(&mut h, "the new tab", |h| h.app().tab_count() == 2);
    assert!(!h.app().reading(), "a new tab has nothing read yet");
    assert!(!h.screen().contains(HEADING), "the new tab shows its own page:\n{}", h.screen());
    h.press("ctrl+pgup");
    until(&mut h, "the first tab's reading again", |h| h.screen().contains(HEADING));

    // An address typed for this tab is another page: its words are not left standing under it.
    h.press("ctrl+l");
    h.type_text(&page("/second"));
    h.press("enter");
    until(&mut h, "the second page", |h| h.app().address() == page("/second"));
    until(&mut h, "the reading of the old page closed", |h| !h.app().reading());
    until(&mut h, "the new page drawn", |h| h.screen().contains('▀'));
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_page_with_almost_no_text_says_so_and_its_button_brings_the_page_back() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/sparse"));
    h.press("f9");
    let nothing = h.env().i18n().translate("browser.reader.nothing", &[]);
    until(&mut h, "the nothing-to-read state", |h| h.screen().contains(&nothing));
    let why = flat(&h.env().i18n().translate("browser.reader.nothing-message", &[]));
    assert!(words(&h).contains(&why), "the state says why there is nothing to read:\n{}", h.screen());

    h.click_text(&h.env().i18n().translate("browser.reader.back", &[]));
    until(&mut h, "the page back", |h| !h.app().reading());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_terminal_that_draws_no_picture_reads_the_page_from_its_own_button() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/article"));
    h.set_graphics(Graphics::None);
    let cannot = h.env().i18n().translate("browser.page.cannot-show", &[]);
    until(&mut h, "the page's place saying why", |h| h.screen().contains(&cannot));

    h.click_text(&h.env().i18n().translate("browser.reader.open", &[]));
    until(&mut h, "the article's words", |h| h.screen().contains(HEADING));
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_pages_own_scripts_never_hear_of_the_reading() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/article"));
    // The page counts every `querySelector` and `querySelectorAll` asked of its own world, on
    // elements and on the document alike.
    assert_eq!(super::eval(&h, "window.__seen"), json!(0), "the page is watched before the reading");

    read_with(&mut h, "f9", HEADING);
    assert_eq!(super::eval(&h, "window.__seen"), json!(0), "the reading was read where the page cannot see it");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_reading_of_an_ascii_page_fits_a_terminal_of_forty_columns_in_ascii() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/narrow"));
    h.set_glyph_mode(GlyphMode::Ascii).resize(40, HEIGHT);
    read_with(&mut h, "f9", NARROW);
    only_ascii(&h, "the reading on a plain terminal");

    // Every word of the article is on the screen: a line cut at the edge of the screen would take
    // a word with it.
    let said = flat(
        "This article says so at some length, so that there is a whole screen of words to scroll \
         through and every kind of them to look at.",
    );
    assert!(words(&h).contains(&said), "the whole paragraph is on a screen of forty columns:\n{}", h.screen());
}

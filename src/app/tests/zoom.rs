//! Zoom from where the person zooms: the three keys and the toolbar's button, read from the page
//! Chromium draws and from the cells qbrow draws the picture into.
//!
//! What the page reports is what it was really given: the level is never asked back of the
//! application, only of the page's own `innerWidth` and of the cells on screen.

use std::time::Duration;

use qframe::color::Rgb;

use super::{HEIGHT, Scratch, Slot, WIDTH, click_icon, eval, find_in_row, open, page, until, until_page};
use crate::app::zoom::{HOME, LADDER};
use crate::engine::{ZOOM_BOUNDS, css_pixels};

type Harness = qframe::runtime::Harness<crate::app::Browser>;

/// The page area's first row: the strip and the toolbar are above it.
const FIRST_PAGE_ROW: u16 = 2;

/// The page area's pixels on the test screen: 100 cells of 10 wide, 30 rows of 20 high.
const PLAIN: (u32, u32) = (1000, 600);

/// A sentence long enough to wrap into several lines and to need one more of them once the page is
/// laid out for a narrower viewport.
const SENTENCE: &str = "the quick brown fox jumps over the lazy dog while the browser redraws \
the page around it and the writing grows large enough to be read without a pair of glasses";

/// The rows of the open list: the level in force, and the `In`, `Out` and `Reset` under it.
fn list(h: &Harness) -> Option<(usize, usize, usize, usize)> {
    let rows = h.screen().lines().count();
    let level = (FIRST_PAGE_ROW as usize..rows).find(|&row| find_in_row(h, "%", row).is_some())?;
    Some((level, level + 1, level + 2, level + 3))
}

/// The open list, with its three rows: the level in force, `In`, `Out` and `Reset`.
fn open_list(h: &mut Harness, icon: &str) -> (usize, usize, usize, usize) {
    h.press("esc");
    click_icon(h, icon);
    until(h, "the zoom list", |h| list(h).is_some());
    list(h).expect("the list is open")
}

/// Whether the open list says `level` is in force; false while it is not open.
fn says(h: &Harness, level: u32) -> bool {
    list(h).is_some_and(|(row, ..)| find_in_row(h, &format!("{level}%"), row).is_some())
}

/// The text colour of the first cell of `text` on row `row`.
fn ink_of(h: &Harness, text: &str, row: usize) -> Option<Rgb> {
    let (x, y) = find_in_row(h, text, row)?;
    h.fg(u16::try_from(x).ok()?, u16::try_from(y).ok()?)
}

/// Whether the zoom button is drawn with the glyph of `icon`.
fn button_is(h: &Harness, icon: &str) -> bool {
    find_in_row(h, &h.env().icons().glyph(icon), 1).is_some()
}

/// Where the picture reaches on the screen: its first and last row and column.
fn painted(h: &Harness) -> Option<(u16, u16, u16, u16)> {
    let area = h.buffer().area;
    let cell = |x: u16, y: u16| h.buffer()[(x, y)].symbol() == "▀";
    let row = |y: u16| (0..area.width).any(|x| cell(x, y));
    let column = |x: u16| (0..area.height).any(|y| cell(x, y));
    Some((
        (0..area.height).find(|&y| row(y))?,
        (0..area.height).rev().find(|&y| row(y))?,
        (0..area.width).find(|&x| column(x))?,
        (0..area.width).rev().find(|&x| column(x))?,
    ))
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn zooming_in_makes_the_writing_bigger_and_the_layout_reflows() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/second"));
    let plain = eval(&h, "innerWidth").as_u64().expect("a width");
    let tall = eval(&h, "innerHeight").as_u64().expect("a height");
    assert_eq!((plain, tall), (u64::from(PLAIN.0), u64::from(PLAIN.1)), "the page area's own pixels");
    // The writing is a twentieth of the width the page itself reports, so the sentence needs the
    // room the page gives it and less of it once the page is laid out narrower.
    let font = plain / 20;
    eval(
        &h,
        &format!(
            "document.body.style.margin = '0';\
             document.body.innerHTML = '<p id=\"sentence\" style=\"margin:0;font-size:{font}px;line-height:1.25\">{SENTENCE}</p>';\
             true"
        ),
    );
    let written =
        |h: &Harness| eval(h, "parseFloat(getComputedStyle(document.getElementById('sentence')).fontSize)").as_u64();
    assert_eq!(written(&h), Some(font), "the writing is a twentieth of the page's width");
    // A block has one box however many lines it wraps into, so the lines are its height over
    // the height of one.
    let lines = |h: &Harness| {
        eval(h, "(() => { const p = document.querySelector('p'); return Math.round(p.getBoundingClientRect().height / parseFloat(getComputedStyle(p).lineHeight)); })()")
            .as_u64()
            .expect("a count of lines")
    };
    let before = lines(&h);
    assert!(before > 1, "the sentence wraps even at 100%:\n{}", h.screen());
    let plain_ratio = eval(&h, "devicePixelRatio").as_f64().expect("a device pixel ratio");

    // Two steps up the ladder from 100% is 125%, so the page is laid out for a fifth less width
    // and drawn a quarter larger.
    h.press("ctrl++").press("ctrl++");
    until_page(&mut h, "innerWidth === 800");
    until_page(&mut h, "devicePixelRatio > 1");
    let zoomed_ratio = eval(&h, "devicePixelRatio").as_f64().expect("a device pixel ratio");
    assert_eq!(zoomed_ratio, plain_ratio * 1.25, "the writing is a quarter bigger than it was");
    // A zoom that magnified without reflowing would leave the line count where it was.
    until(&mut h, "the sentence wrapped into more lines", |h| lines(h) > before);
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_picture_is_the_same_size_at_every_zoom() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let long = page("/long");
    let mut h = open(&scratch, &long);
    let whole = (FIRST_PAGE_ROW, HEIGHT - 1, 0, WIDTH - 1);
    // A frame Chromium drew before the page area's size reached it may still be on screen; the
    // next one is drawn for the area.
    until(&mut h, "the picture filling the page area at 100%", |h| painted(h) == Some(whole));
    let address = find_in_row(&h, &long, 1).expect("the address in the toolbar");

    h.press("ctrl++").press("ctrl++");
    until_page(&mut h, "innerWidth === 800");
    until(&mut h, "the picture filling the same page area at 125%", |h| painted(h) == Some(whole));
    assert_eq!(h.screen().lines().count(), usize::from(HEIGHT), "the screen is as tall as it was");
    assert_eq!(find_in_row(&h, &long, 1), Some(address), "the toolbar did not move");
    assert!(button_is(&h, "browser.zoom.larger"), "and the button is still on it:\n{}", h.screen());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn zooming_out_and_reset_walk_the_ladder_and_stop_at_its_ends() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/third"));
    assert!(button_is(&h, "browser.zoom"), "at rest at 100%:\n{}", h.screen());

    // Out twice: the ladder's own two steps down, and the list says so.
    h.press("ctrl+-").press("ctrl+-");
    until_page(&mut h, "innerWidth === 1250");
    let (_, _, out, _) = open_list(&mut h, "browser.zoom.smaller");
    assert!(says(&h, 80), "the level in the list is the ladder's:\n{}", h.screen());
    let live = ink_of(&h, "Out", out).expect("the Out row");

    // Down to the end of the ladder, where the entry that would pass it is disabled.
    for _ in 0..6 {
        h.press("ctrl+-");
    }
    let (_, _, out, _) = open_list(&mut h, "browser.zoom.smaller");
    assert!(says(&h, 30), "the bottom of the ladder:\n{}", h.screen());
    assert_ne!(ink_of(&h, "Out", out), Some(live), "the entry that would pass the end is drawn as disabled");
    let (x, y) = find_in_row(&h, "Out", out).expect("the Out row");
    h.click(x, y);
    let (_, _, _, reset) = open_list(&mut h, "browser.zoom.smaller");
    assert!(says(&h, 30), "a disabled entry does nothing at all, quietly or otherwise");
    assert!(ink_of(&h, "Reset", reset).is_some(), "the reset row is still there, drawn rather than missing");

    // And back to the size the page area already is.
    let (x, y) = find_in_row(&h, "Reset", reset).expect("the Reset row");
    h.click(x, y);
    until_page(&mut h, "innerWidth === 1000");
    assert!(button_is(&h, "browser.zoom"), "the button is at rest again:\n{}", h.screen());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_level_is_visible_without_being_asked_for() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/third"));
    h.press("ctrl++").press("ctrl++");
    until_page(&mut h, "innerWidth === 800");
    // The glyph is the state, not the hue: a marked one is there and the resting one is not.
    assert!(button_is(&h, "browser.zoom.larger"), "the button is marked at 125%:\n{}", h.screen());
    assert!(!button_is(&h, "browser.zoom"), "the resting glyph is gone:\n{}", h.screen());
    // And the words under the pointer say the level itself.
    let glyph = h.env().icons().glyph("browser.zoom.larger").into_owned();
    let (x, y) = find_in_row(&h, &glyph, 1).expect("the zoom button");
    h.hover(x, y).advance(Duration::from_secs(1));
    assert!(h.screen().contains("Zoom 125%"), "the tooltip says the level:\n{}", h.screen());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_level_is_the_tabs_own() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/second"));
    for _ in 0..5 {
        h.press("ctrl++");
    }
    until_page(&mut h, "innerWidth === 500");
    assert!(button_is(&h, "browser.zoom.larger"), "the first tab is marked:\n{}", h.screen());

    h.press("ctrl+t");
    until(&mut h, "a second tab", |h| h.app().tab_count() == 2);
    assert!(button_is(&h, "browser.zoom"), "a new tab starts at 100%:\n{}", h.screen());
    h.press("ctrl++");
    until_page(&mut h, "innerWidth === 909");

    h.press("ctrl+pgup");
    until_page(&mut h, "innerWidth === 500");
    assert!(button_is(&h, "browser.zoom.larger"), "the first tab kept its own level:\n{}", h.screen());
    open_list(&mut h, "browser.zoom.larger");
    assert!(says(&h, 200), "and it is 200%, not the other tab's:\n{}", h.screen());

    h.press("esc");
    h.press("ctrl+pgdn");
    until_page(&mut h, "innerWidth === 909");
    assert!(button_is(&h, "browser.zoom.larger"), "the second kept its own:\n{}", h.screen());
    open_list(&mut h, "browser.zoom.larger");
    assert!(says(&h, 110), "and it is 110%:\n{}", h.screen());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_level_survives_a_resize_of_the_terminal() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/long"));
    h.press("ctrl++").press("ctrl++");
    until_page(&mut h, "innerWidth === 800");

    h.resize(60, 20);
    // 60 cells of 10 wide and 18 rows of 20 high, laid out for the same 125%.
    until_page(&mut h, "innerWidth === 480 && innerHeight === 288");
    open_list(&mut h, "browser.zoom.larger");
    assert!(says(&h, 125), "the level held through the resize:\n{}", h.screen());

    h.press("esc");
    h.resize(100, 32);
    until_page(&mut h, "innerWidth === 800 && innerHeight === 480");
    open_list(&mut h, "browser.zoom.larger");
    assert!(says(&h, 125), "and it is still the level:\n{}", h.screen());
    h.press("esc");
    // The picture of the area the terminal is now, not of the one it was.
    until(&mut h, "the picture of the new area", |h| painted(h) == Some((FIRST_PAGE_ROW, HEIGHT - 1, 0, WIDTH - 1)));
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_keys_work_from_the_page_and_from_the_toolbar() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/form"));
    eval(&h, "window.__seen = []; addEventListener('keydown', (event) => window.__seen.push(event.key), true); true");
    // The recorder itself works, so a list that stays at one says the keys never reached the page.
    h.press("a");
    until_page(&mut h, "window.__seen.length === 1");

    h.press("ctrl++");
    until_page(&mut h, "innerWidth === 909");
    assert_eq!(eval(&h, "window.__seen.length").as_u64(), Some(1), "the page did not see ctrl++");

    // The same change from the toolbar's own list, opened with a click alone: Esc, which the
    // other tests press first to close any list, is a key the page is meant to see.
    click_icon(&mut h, "browser.zoom.larger");
    until(&mut h, "the zoom list", |h| list(h).is_some());
    let (level, ..) = list(&h).expect("the list is open");
    let (x, y) = find_in_row(&h, "In", level + 1).expect("the In row");
    h.click(x, y);
    until_page(&mut h, "innerWidth === 800");
    assert_eq!(eval(&h, "window.__seen.length").as_u64(), Some(1), "the page saw nothing of that either");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn ctrl_minus_as_most_terminals_send_it_zooms_out() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/third"));
    // Without the kitty keyboard protocol ctrl+- arrives as the byte of ctrl+_.
    h.press("ctrl+_");
    until_page(&mut h, "innerWidth === 1111");
    h.press("ctrl+=");
    until_page(&mut h, "innerWidth === 1000");
}

#[test]
fn the_engine_clamps_and_rounds_the_way_it_says_it_does() {
    let (floor, ceiling) = ZOOM_BOUNDS;
    assert_eq!((floor, ceiling), (LADDER[0], LADDER[LADDER.len() - 1]), "the ladder and the clamp agree");
    let mut levels: Vec<u32> = LADDER.to_vec();
    levels.extend([0, 1, 25, 29, 201, 500, 10_000, u32::MAX]);
    for percent in levels {
        let in_force = u64::from(percent.clamp(floor, ceiling));
        for pixels in [1, 7, 9, 10, 20, 63, 600, 999, 1000, 1001, 16_384, 65_536] {
            let css = css_pixels(pixels, percent);
            assert!(css >= 1, "{pixels} px at {percent}% is no viewport at all");
            // Rounded down: this many CSS pixels still fit in the area and one more would not.
            let area = u64::from(pixels) * 100;
            assert!(
                css == 1 || u64::from(css) * in_force <= area,
                "{pixels} px at {percent}% became {css}, more than fits"
            );
            assert!(u64::from(css + 1) * in_force > area, "{pixels} px at {percent}% became {css}, rounded up");
        }
    }
    assert_eq!(css_pixels(1000, HOME), 1000, "100% is the size the area already is");
    assert_eq!(css_pixels(1000, 125), 800);
    assert_eq!(css_pixels(600, 125), 480);
    // Down and never up: 1000 px at 110% is 909 and a bit, and 910 would be a pixel too many.
    assert_eq!(css_pixels(1000, 110), 909);
    // The clamp is not silent: a level outside the ladder is the level it was brought to.
    assert_eq!(css_pixels(1000, 0), css_pixels(1000, floor));
    assert_eq!(css_pixels(1000, 29), css_pixels(1000, floor));
    assert_eq!(css_pixels(1000, 201), css_pixels(1000, ceiling));
    assert_eq!(css_pixels(1000, 10_000), css_pixels(1000, ceiling));
    assert_eq!(css_pixels(1000, u32::MAX), css_pixels(1000, ceiling));
}

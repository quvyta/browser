//! The screen on a plain terminal: sixteen colours, ASCII glyphs and no pictures, as over a serial
//! line or in a console. Every part of it is still there, drawn with characters such a terminal
//! has, and what colour alone would say is said by a sign as well.

use qframe::color::ColorDepth;
use qframe::graphics::Graphics;
use qframe::icons::GlyphMode;

use super::{Scratch, Slot, find_in_row, open, page, until};
use crate::app::Browser;

type Harness = qframe::runtime::Harness<Browser>;

/// Turns `h` into a plain terminal's screen.
fn plain(h: &mut Harness) {
    h.set_depth(ColorDepth::Ansi16).set_glyph_mode(GlyphMode::Ascii).set_graphics(Graphics::None);
}

/// Fails with the screen when a cell holds a character outside printable ASCII.
fn only_ascii(h: &Harness, what: &str) {
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

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_plain_terminal_shows_every_part_of_the_screen_in_ascii() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let links = page("/links");
    let bookmarks = scratch.path("data/quvyta/browser/bookmarks");
    std::fs::create_dir_all(bookmarks.parent().unwrap()).unwrap();
    std::fs::write(&bookmarks, format!("{links}\tLinks\n{}\tSecond\n", page("/second"))).unwrap();
    let mut h = open(&scratch, &links);
    until(&mut h, "the tab named", |h| find_in_row(h, "Links", 0).is_some());
    plain(&mut h);
    let cannot = h.env().i18n().translate("browser.page.cannot-show", &[]);
    until(&mut h, "the page's place saying why", |h| h.screen().contains(&cannot));
    only_ascii(&h, "the page with its tab, toolbar and bookmarks");
    assert!(find_in_row(&h, "Second", 2).is_some(), "the bar of bookmarks is there:\n{}", h.screen());

    // The star of a kept page is told from an open one by its sign, not by colour alone, and no
    // two buttons of the toolbar share a sign.
    let icons = h.env().icons();
    let signs: Vec<String> = [
        "chevron-left",
        "chevron-right",
        "chevron-down",
        "refresh",
        "browser.star",
        "browser.starred",
        "browser.zoom",
        "settings",
    ]
    .iter()
    .map(|icon| icons.glyph(icon).into_owned())
    .collect();
    for (at, sign) in signs.iter().enumerate() {
        assert!(!signs[at + 1..].contains(sign), "`{sign}` stands for two buttons: {signs:?}");
    }
    let kept = h.env().icons().glyph("browser.starred").into_owned();
    assert!(find_in_row(&h, &kept, 1).is_some(), "the kept sign on the toolbar:\n{}", h.screen());

    // The lens is the last of its sign on the row; an address holds slashes too.
    let lens = h.env().icons().glyph("browser.zoom").into_owned();
    let row = h.screen().lines().nth(1).unwrap_or_default().to_owned();
    let at = row.rfind(&lens).expect("the zoom button");
    h.click(i32::from(qframe::text::width(&row[..at])), 1);
    until(&mut h, "the zoom list", |h| h.screen().contains("100%"));
    only_ascii(&h, "the zoom list");
    h.press("esc");

    h.press("ctrl+,");
    let heading = h.env().i18n().translate("browser.settings.heading", &[]);
    until(&mut h, "the settings", |h| h.screen().contains(&heading));
    only_ascii(&h, "the settings");
}

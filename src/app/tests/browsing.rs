//! A page from where the person clicks, types and scrolls: the page drawn and named, links, the
//! address bar, back and forward, the wheel, fields, and Esc on a page that is loading.

use std::time::{Duration, Instant};

use qframe::color::Rgb;
use qframe::event::MouseKind;
use qframe::graphics::Graphics;
use qframe::prelude::Harness;
use serde_json::json;

use super::super::Browser;
use super::{
    PAGE_TOP, PATIENCE, Scratch, Slot, cell_of, click_icon, eval, find_in_row, open, open_on, page, page_drawn, until,
    until_page,
};

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_start_address_is_drawn_and_its_tab_named_by_its_title() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let links = page("/links");
    let mut h = open_on(scratch.machine(), Some(&links));
    until(&mut h, "the page drawn and named", |h| page_drawn(h) && find_in_row(h, "Links", 0).is_some());
    assert!(find_in_row(&h, &links, 1).is_some(), "the address bar shows the address:\n{}", h.screen());
    let viewport = eval(&h, "[innerWidth, innerHeight]");
    assert_eq!(viewport, json!([1000, 600]), "the page is laid out for the page area, 100 × 30 cells of 10 × 20");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn clicking_a_link_where_it_is_drawn_follows_it() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/links"));
    let (x, y) = cell_of(&h, "#next");
    assert!(y >= PAGE_TOP);
    h.click(x, y);
    let second = page("/second");
    until(&mut h, "the second page", |h| h.app().address() == second && find_in_row(h, &second, 1).is_some());
    assert_eq!(eval(&h, "location.href"), json!(second), "Chromium is where the address bar says");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn an_address_typed_after_clicking_the_address_bar_is_gone_to() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let links = page("/links");
    let mut h = open(&scratch, &links);
    let (x, y) = find_in_row(&h, &links, 1).expect("the address on the toolbar");
    h.click(x, y);
    let third = page("/third");
    h.type_text(&third).press("enter");
    until_page(&mut h, "document.title === 'Third'");
    assert_eq!(h.app().address(), third);
    until(&mut h, "the tab named Third", |h| find_in_row(h, "Third", 0).is_some());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn back_and_forward_on_the_toolbar_walk_the_history() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let links = page("/links");
    let mut h = open(&scratch, &links);
    let (x, y) = cell_of(&h, "#next");
    h.click(x, y);
    let second = page("/second");
    until(&mut h, "the second page", |h| h.app().address() == second);
    click_icon(&mut h, "chevron-left");
    until(&mut h, "the way back", |h| h.app().address() == links);
    assert_eq!(eval(&h, "location.href"), json!(links));
    click_icon(&mut h, "chevron-right");
    until(&mut h, "the way forward", |h| h.app().address() == second);
    assert_eq!(eval(&h, "location.href"), json!(second));
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_wheel_over_the_page_scrolls_it() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/long"));
    assert_eq!(eval(&h, "scrollY"), json!(0));
    for _ in 0..3 {
        h.mouse(MouseKind::ScrollDown, 20, PAGE_TOP + 5);
    }
    until_page(&mut h, "scrollY >= 180");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn keys_typed_after_clicking_a_field_of_the_page_land_in_it() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/form"));
    let (x, y) = cell_of(&h, "#field");
    h.click(x, y);
    until_page(&mut h, "document.activeElement.id === 'field'");
    h.type_text("Hi there");
    until_page(&mut h, "document.querySelector('#field').value === 'Hi there'");
    h.paste(", you");
    until_page(&mut h, "document.querySelector('#field').value === 'Hi there, you'");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn esc_on_a_loading_page_stops_it() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/links"));
    h.press("ctrl+l").type_text(&page("/slow")).press("enter");
    until(&mut h, "the slow page loading", |h| {
        find_in_row(h, &h.env().icons().glyph("close"), 1).is_some()
            && eval(h, "document.readyState") == json!("loading")
    });
    h.press("esc");
    until(&mut h, "the load stopped", |h| {
        find_in_row(h, &h.env().icons().glyph("refresh"), 1).is_some()
            && eval(h, "document.readyState !== 'loading'") == json!(true)
    });
}

/// A large full-pixel terminal window: 200 × 50 cells of 18 × 36 pixels, so every picture of the
/// page is 3600 × 1728 pixels, as on a high-density screen.
const LARGE: (u16, u16, (u16, u16)) = (200, 50, (18, 36));

/// How soon a turn of the wheel shows on screen at the least. Optimised, a picture of a
/// [`LARGE`] window takes a few milliseconds to decode and the whole way from the wheel to the
/// screen well under a quarter of a second; unoptimised it took over a second, and the page
/// seemed not to scroll at all.
const PROMPT: Duration = Duration::from_millis(500);

/// How many turns [`fastest_redraw`] times: a busy machine may slow one, not all of them.
const TURNS: usize = 3;

/// The screen on the `/long` page in a [`LARGE`] window drawn with `graphics`, once its first
/// picture has settled.
fn large_long_page(scratch: &Scratch, graphics: Graphics) -> Harness<Browser> {
    let (width, height, cell) = LARGE;
    let long = page("/long");
    let mut h = open_on(scratch.machine(), Some(&long));
    h.resize(width, height).set_graphics(graphics).set_cell_pixels(Some(cell));
    // Half blocks show two pixels a cell, so that is all Chromium is asked to send; a full-pixel
    // terminal gets the page area's own pixels.
    let wide = match graphics {
        Graphics::Kitty | Graphics::Sixel => u32::from(width) * u32::from(cell.0),
        _ => u32::from(width),
    };
    until(&mut h, "the page laid out for the large window", |h| {
        h.app().address() == long && h.app().tab().picture.as_ref().is_some_and(|picture| picture.width() == wide)
    });
    h
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn half_blocks_get_two_pixels_a_cell_and_real_pixels_the_whole_page_area() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let (width, height, cell) = LARGE;
    let mut h = large_long_page(&scratch, Graphics::HalfBlock);
    let rows = u32::from(height) - u32::try_from(PAGE_TOP).unwrap();
    let size = |h: &Harness<Browser>| h.app().tab().picture.as_ref().map(|picture| (picture.width(), picture.height()));
    assert_eq!(size(&h), Some((u32::from(width), rows * 2)), "a picture of two pixels a cell");
    // The page itself is still laid out for the page area's pixels; only the picture is small.
    assert_eq!(eval(&h, "innerWidth"), json!(u32::from(width) * u32::from(cell.0)));
    h.set_graphics(Graphics::Kitty);
    let full = (u32::from(width) * u32::from(cell.0), rows * u32::from(cell.1));
    until(&mut h, "the full picture for a full-pixel terminal", |h| size(h) == Some(full));
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn with_cells_not_twice_as_tall_as_wide_the_page_fills_its_area_and_a_click_lands_where_it_is_drawn() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let edges = page("/edges");
    let mut h = open(&scratch, &edges);
    // Nine by twenty: a cell narrower than half its height, as many terminal fonts have.
    h.set_cell_pixels(Some((9, 20)));
    let size = h.buffer().area;
    let rows = u32::from(size.height) - u32::try_from(PAGE_TOP).unwrap();
    // Every column gets its own pixel and the rows more than two, in the viewport's shape.
    let wanted = (u32::from(size.width), (rows * 20).div_ceil(9));
    until(&mut h, "a picture of the page for cells of 9 × 20", |h| {
        h.app().tab().picture.as_ref().is_some_and(|picture| (picture.width(), picture.height()) == wanted)
    });
    let row = PAGE_TOP + i32::try_from(rows / 2).unwrap();
    // Red, blue, a blend of the two where the picture's edge between them is soft, or the ground
    // where the picture does not reach.
    let colour = |h: &Harness<Browser>, column: u16| {
        let at = u16::try_from(row).unwrap();
        if h.buffer()[(column, at)].symbol() != "▀" {
            return '.';
        }
        match h.fg(column, at) {
            Some(rgb) if rgb.r > 150 && rgb.g < 70 && rgb.b < 70 => 'r',
            Some(rgb) if rgb.b > 150 && rgb.r < 70 && rgb.g < 70 => 'b',
            _ => '~',
        }
    };
    let line: String = (0..size.width).map(|column| colour(&h, column)).collect();
    // Drawn with the old square half blocks the picture was narrower than the area, and the
    // ground showed at both sides.
    assert!(line.starts_with('r') && line.ends_with("bb"), "the page from edge to edge: {line}");
    assert!(!line.contains('.'), "no ground beside the page: {line}");
    let bar = i32::try_from(line.find('b').unwrap()).unwrap();
    h.click(bar, row);
    until(&mut h, "the click on the bar where it is drawn", |h| eval(h, "went") == json!(1));
}

/// Turns the wheel down over the page [`TURNS`] times and returns the shortest time from a turn
/// to `seen` telling a new picture from the one before it. Between turns the picture settles.
fn fastest_redraw<T: PartialEq>(h: &mut Harness<Browser>, seen: impl Fn(&Harness<Browser>) -> T) -> Duration {
    let mut fastest = Duration::MAX;
    for _ in 0..TURNS {
        settle(h, &seen);
        let before = seen(h);
        let start = Instant::now();
        h.mouse(MouseKind::ScrollDown, 40, PAGE_TOP + 10);
        until(h, "the page redrawn after the wheel", |h| seen(h) != before);
        fastest = fastest.min(start.elapsed());
    }
    fastest
}

/// Waits until what `seen` reads has not changed for a while: the page has stopped moving.
fn settle<T: PartialEq>(h: &mut Harness<Browser>, seen: &impl Fn(&Harness<Browser>) -> T) {
    let deadline = Instant::now() + PATIENCE;
    let mut last = seen(h);
    let mut still = Instant::now();
    while still.elapsed() < Duration::from_millis(400) {
        assert!(Instant::now() < deadline, "the page never stopped moving:\n{}", h.screen());
        h.advance(Duration::from_millis(20));
        std::thread::sleep(Duration::from_millis(10));
        let now = seen(h);
        if now != last {
            last = now;
            still = Instant::now();
        }
    }
}

/// The colours of every cell of the page area.
fn page_cells(h: &Harness<Browser>) -> Vec<(Option<Rgb>, Option<Rgb>)> {
    let size = h.buffer().area;
    let top = u16::try_from(PAGE_TOP).unwrap_or_default();
    (top..size.height)
        .flat_map(|y| (0..size.width).map(move |x| (x, y)))
        .map(|(x, y)| (h.fg(x, y), h.bg(x, y)))
        .collect()
}

/// A column of pixels down the middle of the picture the page area shows, which a kitty terminal
/// draws itself.
fn picture_column(h: &Harness<Browser>) -> Vec<Option<Rgb>> {
    let Some(picture) = h.app().tab().picture.as_ref() else { return Vec::new() };
    (0..picture.height()).step_by(8).map(|y| picture.pixel(picture.width() / 2, y)).collect()
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_wheel_over_the_page_redraws_its_picture_promptly_in_a_large_window() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = large_long_page(&scratch, Graphics::HalfBlock);
    let fastest = fastest_redraw(&mut h, page_cells);
    assert!(fastest < PROMPT, "the quickest turn of the wheel took {fastest:?} to show");
    assert!(eval(&h, "scrollY") != json!(0), "and the page itself scrolled");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn in_a_full_pixel_terminal_the_wheel_brings_a_new_picture_of_the_page_promptly() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = large_long_page(&scratch, Graphics::Kitty);
    let fastest = fastest_redraw(&mut h, picture_column);
    assert!(fastest < PROMPT, "the quickest turn of the wheel took {fastest:?} to bring a new picture");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_larger_font_with_the_same_cells_lays_the_page_out_again_and_clicks_still_land() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/links"));
    assert_eq!(eval(&h, "[innerWidth, innerHeight]"), json!([1000, 600]), "cells of 10 × 20 at first");
    // The font grows while the window keeps its columns and rows: no resize says so.
    h.set_cell_pixels(Some((12, 24)));
    until_page(&mut h, "innerWidth === 1200 && innerHeight === 720");
    let middle = eval(
        &h,
        "(() => { const r = document.querySelector('#next').getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; })()",
    );
    let (x, y) = (middle[0].as_f64().unwrap(), middle[1].as_f64().unwrap());
    #[expect(clippy::cast_possible_truncation, reason = "a cell of the test screen")]
    let (column, row) = ((x / 12.0).floor() as i32, (y / 24.0).floor() as i32 + PAGE_TOP);
    h.click(column, row);
    let second = page("/second");
    until(&mut h, "the link followed", |h| h.app().address() == second);
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_page_that_takes_long_to_load_shows_the_loading_mark_beside_the_star() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let links = page("/links");
    let mut h = open(&scratch, &links);
    // The mark sits on the toolbar right of the star, after the star's own button and whatever
    // else stands there: what that part of the row shows while nothing loads is recorded first.
    let star = h.env().icons().glyph("browser.star").into_owned();
    let right_of_star = |h: &Harness<Browser>| {
        let row = h.screen().lines().nth(1).unwrap_or_default().to_owned();
        let at = row.find(star.as_str())?;
        Some(row[at..].trim_end().to_owned())
    };
    let idle = right_of_star(&h).expect("the star on the toolbar");
    let (x, y) = find_in_row(&h, &links, 1).expect("the address on the toolbar");
    h.click(x, y);
    h.type_text(&page("/slow")).press("enter");
    until(&mut h, "the loading mark", |h| right_of_star(h).is_some_and(|now| now != idle));
    h.press("esc");
}

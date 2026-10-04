//! Tabs from where the person clicks and presses: a page's new window, the close mark, the last
//! tab, `+`, and the keys that open, close and change tabs.

use qframe::graphics::Graphics;
use serde_json::json;

use super::{Scratch, Slot, cell_of, eval, find_in_row, open, open_on, page, page_drawn, until, until_page};

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_target_blank_link_opens_a_tab_right_of_its_opener_and_shows_it() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/blank"));
    let opener = h.app().active_tab().cloned();
    let (x, y) = cell_of(&h, "#out");
    h.click(x, y);
    let second = page("/second");
    until(&mut h, "the new tab shown", |h| h.app().tab_count() == 2 && h.app().address() == second);
    assert_ne!(h.app().active_tab(), opener.as_ref());
    assert_eq!(eval(&h, "location.href"), json!(second));
    until(&mut h, "both tabs named", |h| {
        let blank = find_in_row(h, "Blank", 0);
        let new = find_in_row(h, "Second", 0);
        blank.zip(new).is_some_and(|(blank, new)| blank.0 < new.0)
    });
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_close_mark_of_a_tab_closes_it_in_chromium_too() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/blank"));
    let (x, y) = cell_of(&h, "#out");
    h.click(x, y);
    until(&mut h, "the second tab named", |h| h.app().tab_count() == 2 && find_in_row(h, "Second", 0).is_some());
    let second = h.app().active_tab().cloned().expect("the second tab");
    let line = h.screen().lines().next().unwrap_or_default().to_owned();
    let name = line.find("Second").expect("the tab's name");
    let mark = line[name..].find('×').map(|at| line[..name + at].chars().count()).expect("the tab's close mark");
    h.click(i32::try_from(mark).unwrap(), 0);
    until(&mut h, "one tab left", |h| h.app().tab_count() == 1 && h.app().address() == page("/blank"));
    until(&mut h, "the tab gone from Chromium", |h| {
        h.app().engine().is_some_and(|engine| engine_lacks(engine, &second))
    });
}

/// Whether Chromium no longer has the tab: asking it anything fails.
fn engine_lacks(engine: &crate::engine::Engine, tab: &crate::engine::TabId) -> bool {
    engine.evaluate(tab, "1", std::time::Duration::from_secs(5)).is_err()
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn closing_the_last_tab_leaves_an_empty_one_with_the_keyboard_in_the_address_bar() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/links"));
    let first = h.app().active_tab().cloned();
    h.press("ctrl+w");
    until(&mut h, "an empty tab", |h| {
        h.app().tab_count() == 1 && h.app().active_tab().is_some() && h.app().active_tab() != first.as_ref()
    });
    assert_eq!(h.app().address(), "");
    assert!(find_in_row(&h, "New tab", 0).is_some(), "{}", h.screen());
    assert!(!h.quit_requested(), "qbrowser goes on");
    let second = page("/second");
    h.type_text(&second).press("enter");
    until(&mut h, "the typed address", |h| h.app().address() == second);
    assert_eq!(eval(&h, "location.href"), json!(second));
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn plus_opens_an_empty_tab_and_what_is_typed_next_goes_into_its_address() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let links = page("/links");
    let mut h = open(&scratch, &links);
    let (x, y) = find_in_row(&h, "+", 0).expect("the add button");
    h.click(x, y);
    assert_eq!(h.app().tab_count(), 2);
    let third = page("/third");
    h.type_text(&third).press("enter");
    until(&mut h, "the new tab at the typed address", |h| {
        h.app().address() == third && find_in_row(h, "Third", 0).is_some()
    });
    assert_eq!(eval(&h, "location.href"), json!(third));
    assert!(find_in_row(&h, "Links", 0).is_some(), "the first tab stays:\n{}", h.screen());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_keys_open_close_and_change_tabs() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let links = page("/links");
    let mut h = open(&scratch, &links);
    let first = h.app().active_tab().cloned();
    h.press("ctrl+t");
    let second = page("/second");
    h.type_text(&second).press("enter");
    until(&mut h, "the second tab", |h| h.app().tab_count() == 2 && h.app().address() == second);
    h.press("ctrl+pgdn");
    until(&mut h, "round to the first tab", |h| h.app().address() == links);
    assert_eq!(h.app().active_tab(), first.as_ref());
    h.press("ctrl+pgup");
    until(&mut h, "back to the second tab", |h| h.app().address() == second);
    h.press("ctrl+w");
    until(&mut h, "the first tab alone", |h| h.app().tab_count() == 1 && h.app().address() == links);
}

/// Opens empty tabs with the new tab key until there are `count`, each sent back to the page
/// with Esc so the next key opens another.
fn open_tabs(h: &mut qframe::prelude::Harness<super::super::Browser>, count: usize) {
    while h.app().tab_count() < count {
        h.press("ctrl+t").press("esc");
    }
    until(h, &format!("{count} tabs"), |h| h.app().tab_count() == count);
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_strip_is_one_row_above_the_toolbar_and_a_lone_tab_does_not_stretch_across_it() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    for graphics in [Graphics::HalfBlock, Graphics::Kitty] {
        let mut h = open(&scratch, &page("/links"));
        h.set_graphics(graphics);
        for count in [1, 5, 20] {
            open_tabs(&mut h, count);
            let screen = h.screen();
            let rows: Vec<&str> = screen.lines().collect();
            let back = h.env().icons().glyph("chevron-left").into_owned();
            assert!(
                rows[0].contains('×') && rows[0].contains('+'),
                "{graphics:?}, {count} tabs: the strip on the first row:\n{screen}"
            );
            assert!(
                rows[1].trim_start().starts_with(back.as_str()),
                "{graphics:?}, {count} tabs: the toolbar right below it:\n{screen}"
            );
            assert!(
                rows[2..].iter().all(|row| !row.contains('×') && !row.contains("New tab") && !row.contains("Links")),
                "{graphics:?}, {count} tabs: nothing of the strip below the toolbar:\n{screen}"
            );
            let marks = rows[0].matches('×').count();
            match count {
                1 => {
                    let mark = rows[0].chars().position(|c| c == '×').unwrap_or(usize::MAX);
                    assert!(
                        mark < 30,
                        "{graphics:?}: a lone tab is as wide as a desktop tab, its × at {mark}:\n{screen}"
                    );
                }
                5 => assert_eq!(marks, 5, "{graphics:?}: five tabs fit side by side:\n{screen}"),
                _ => {
                    assert!(marks < count, "{graphics:?}: twenty tabs scroll sideways:\n{screen}");
                    let arrow = h.env().icons().glyph("chevron-left").into_owned();
                    assert!(
                        rows[0].contains(arrow.as_str()) || rows[0].contains('◀'),
                        "{graphics:?}: with arrows to scroll:\n{screen}"
                    );
                }
            }
        }
    }
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_tabs_open_at_quitting_come_back_at_the_next_start() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/links"));
    h.press("ctrl+t").type_text(&page("/third")).press("enter");
    until(&mut h, "the second tab named", |h| find_in_row(h, "Third", 0).is_some());
    h.click_text("Links");
    until(&mut h, "the first tab on screen", |h| h.app().address() == page("/links"));
    h.press("ctrl+q");
    assert!(h.quit_requested());
    drop(h);
    let mut h = open_on(scratch.machine(), None);
    until(&mut h, "both tabs back", |h| {
        find_in_row(h, "Links", 0).is_some() && find_in_row(h, "Third", 0).is_some() && page_drawn(h)
    });
    assert_eq!(h.app().tab_count(), 2, "no empty tab besides them:\n{}", h.screen());
    assert_eq!(h.app().address(), page("/links"), "the tab that was on screen is on screen again");
    // An address on the command line opens after them, on screen.
    h.press("ctrl+q");
    drop(h);
    let second = page("/second");
    let h = open(&scratch, &second);
    assert_eq!(h.app().tab_count(), 3, "{}", h.screen());
    assert_eq!(h.app().address(), second);
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_tab_closed_before_chromium_opened_it_does_not_come_back() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let links = page("/links");
    let mut h = open(&scratch, &links);
    // Open and close again before the screen has heard anything from Chromium: no time passes
    // between the keys, as on a loaded machine where Chromium answers late.
    h.press("ctrl+t").press("esc").press("ctrl+w");
    assert_eq!(h.app().tab_count(), 1, "closed at once:\n{}", h.screen());
    // Chromium opens the tab it was asked for, and it is closed then; a second tab opened after
    // it shows that the answer for the first has been heard.
    h.press("ctrl+t").type_text(&page("/second")).press("enter");
    until(&mut h, "the second page drawn in its tab", |h| {
        h.app().address() == page("/second") && h.app().active_tab().is_some()
    });
    until_page(&mut h, "document.title === 'Second'");
    // Chromium's report of the first tab can come after the second page is drawn; two seconds
    // of the screen hearing it out leave it time to.
    let heard_out = std::time::Instant::now();
    while heard_out.elapsed() < std::time::Duration::from_secs(2) {
        h.advance(std::time::Duration::from_millis(20));
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(h.app().tab_count(), 2, "the tab closed early never came back:\n{}", h.screen());
}

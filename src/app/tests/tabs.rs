//! Tabs from where the person clicks and presses: a page's new window, the close mark, the last
//! tab, `+`, and the keys that open, close and change tabs.

use serde_json::json;

use super::{Scratch, Slot, cell_of, eval, find_in_row, open, page, until};

#[test]
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

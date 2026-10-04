//! The key overview: F1 opens it over the page and on every other screen, and a `?` typed into a
//! page's field is the field's.

use std::time::Duration;

use qframe::prelude::Harness;

use super::{Scratch, Slot, cell_of, open, open_on, page, until, until_page};
use crate::app::Browser;

/// The overview's own row, which no other screen of qbrow shows.
const RULE: &str = "every other key goes to the page";

/// Whether the overview is on screen.
fn overview(h: &Harness<Browser>) -> bool {
    h.screen().contains(RULE)
}

/// Whether the overview lists qbrow's own keys as the keymap binds them: filtered to the reading
/// mode, whose key no other part of the screen writes out, it shows F9.
fn lists_the_keymap(h: &Harness<Browser>) -> bool {
    let screen = h.screen();
    screen.contains("reading mode") && screen.to_lowercase().contains("f9")
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn f1_opens_the_overview_over_the_page_and_esc_closes_it() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/form"));
    let (x, y) = cell_of(&h, "#field");
    h.click(x, y);
    until_page(&mut h, "document.activeElement.id === 'field'");
    h.press("f1");
    until(&mut h, "the overview on F1", overview);
    h.type_text("reading");
    until(&mut h, "qbrow's own keys, filtered", lists_the_keymap);
    h.press("esc").press("esc");
    until(&mut h, "the overview closed", |h| !overview(h));
    // Neither F1 nor the filter's letters reached the page: the field still holds nothing.
    until_page(&mut h, "document.querySelector('#field').value === ''");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_question_mark_typed_on_the_page_is_the_pages() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/form"));
    let (x, y) = cell_of(&h, "#field");
    h.click(x, y);
    until_page(&mut h, "document.activeElement.id === 'field'");
    h.type_text("why?");
    until_page(&mut h, "document.querySelector('#field').value === 'why?'");
    assert!(!overview(&h), "the field took the question mark:\n{}", h.screen());
}

#[test]
fn without_chromium_f1_still_shows_the_keys() {
    let scratch = Scratch::new();
    let mut h = open_on(scratch.machine_without_chromium(), None);
    h.advance(Duration::from_millis(100));
    until(&mut h, "the missing Chromium", |h| h.screen().contains("Chromium was not found"));
    h.press("f1");
    until(&mut h, "the overview on F1", overview);
    h.press("esc");
    until(&mut h, "the overview closed", |h| !overview(h));
}

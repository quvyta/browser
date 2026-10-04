//! A page's own dialogs, from where the person clicks: the page opens one, it shows over the
//! page, and the person's answer reaches the page. Without them the page would wait for an answer
//! nobody could give, and stop answering clicks and keys.

use super::{Scratch, Slot, cell_of, find_in_row, open, page, until, until_page};

/// Opens `path` and clicks its button, which opens the page's dialog.
fn open_dialog(path: &str) -> (Scratch, qframe::runtime::Harness<crate::app::Browser>) {
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page(path));
    let (x, y) = cell_of(&h, "#open");
    h.click(x, y);
    (scratch, h)
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn an_alert_shows_its_message_and_enter_lets_the_page_go_on() {
    let _slot = Slot::take();
    let (_scratch, mut h) = open_dialog("/alert");
    until(&mut h, "the alert", |h| h.screen().contains("Hello from the page"));
    assert!(h.screen().contains("127.0.0.1"), "the dialog names the site that opened it:\n{}", h.screen());
    h.press("enter");
    until(&mut h, "the page going on", |h| find_in_row(h, "Alerted", 0).is_some());
    assert!(!h.screen().contains("Hello from the page"), "the dialog is gone:\n{}", h.screen());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_confirm_answered_with_cancel_says_no_to_the_page() {
    let _slot = Slot::take();
    let (_scratch, mut h) = open_dialog("/confirm");
    until(&mut h, "the question", |h| h.screen().contains("Go on?"));
    h.click_text("Cancel");
    until_page(&mut h, "document.title === 'No'");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_confirm_answered_with_ok_says_yes_to_the_page() {
    let _slot = Slot::take();
    let (_scratch, mut h) = open_dialog("/confirm");
    until(&mut h, "the question", |h| h.screen().contains("Go on?"));
    h.click_text("OK");
    until_page(&mut h, "document.title === 'Yes'");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_prompt_offers_its_text_and_hands_the_page_what_was_typed() {
    let _slot = Slot::take();
    let (_scratch, mut h) = open_dialog("/prompt");
    until(&mut h, "the prompt", |h| h.screen().contains("Your name?") && h.screen().contains("Ada"));
    // The field has the keyboard: what is typed goes after the offered text.
    h.type_text(" Lovelace").press("enter");
    until_page(&mut h, "document.title === 'Answer Ada Lovelace'");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn esc_on_a_prompt_answers_nothing() {
    let _slot = Slot::take();
    let (_scratch, mut h) = open_dialog("/prompt");
    until(&mut h, "the prompt", |h| h.screen().contains("Your name?"));
    h.press("esc");
    until_page(&mut h, "document.title === 'Answer null'");
}

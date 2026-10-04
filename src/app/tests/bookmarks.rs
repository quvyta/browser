//! Bookmarks from where the person clicks and presses: the star and ctrl+d, the bar with its
//! clicks, middle clicks, menu and "More", the address bar's suggestions, and the file that keeps
//! them across starts, broken or not writable.

use std::path::PathBuf;

use qframe::color::Rgb;
use qframe::event::{MouseButton, MouseKind};
use serde_json::json;

use super::{Scratch, Slot, click_icon, eval, find_in_row, open, open_on, page, page_drawn, until, until_page};
use crate::app::Browser;

/// The row of the bar of bookmarks, right below the toolbar.
const BAR: usize = 2;

/// The bookmarks file of the scratch machine.
fn file(scratch: &Scratch) -> PathBuf {
    scratch.path("data/quvyta/browser/bookmarks")
}

/// The bookmarks file's lines.
fn stored(scratch: &Scratch) -> Vec<String> {
    std::fs::read_to_string(file(scratch)).unwrap_or_default().lines().map(str::to_owned).collect()
}

/// Writes `bookmarks`, addresses and names, as the file qbrowser starts with.
fn keep(scratch: &Scratch, bookmarks: &[(&str, &str)]) {
    let text: String = bookmarks.iter().map(|(url, name)| format!("{url}\t{name}\n")).collect();
    write_file(scratch, &text);
}

fn write_file(scratch: &Scratch, text: &str) {
    let path = file(scratch);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// The bar's row on screen.
fn bar(h: &Harness) -> String {
    h.screen().lines().nth(BAR).unwrap_or_default().to_owned()
}

type Harness = qframe::runtime::Harness<Browser>;

/// Whether the star shows `icon`.
fn star_is(h: &Harness, icon: &str) -> bool {
    find_in_row(h, &h.env().icons().glyph(icon), 1).is_some()
}

/// The colour the star is drawn in, beside the colour of the settings gear, a toolbar button that
/// is never selected.
fn star_and_gear_ink(h: &Harness) -> (Option<Rgb>, Option<Rgb>) {
    let ink = |icon: &str| {
        let (x, y) = find_in_row(h, &h.env().icons().glyph(icon), 1)?;
        h.fg(u16::try_from(x).ok()?, u16::try_from(y).ok()?)
    };
    let star = ink("browser.starred").or_else(|| ink("browser.star"));
    (star, ink("settings"))
}

/// Clicks `text` on the bar.
fn click_on_bar(h: &mut Harness, text: &str) {
    let (x, y) = find_in_row(h, text, BAR).unwrap_or_else(|| panic!("no {text} on the bar:\n{}", h.screen()));
    h.click(x, y);
}

/// Presses and lets go of `button` on `text` on the bar.
fn press_on_bar(h: &mut Harness, text: &str, button: MouseButton) {
    let (x, y) = find_in_row(h, text, BAR).unwrap_or_else(|| panic!("no {text} on the bar:\n{}", h.screen()));
    h.mouse(MouseKind::Down(button), x, y);
    h.mouse(MouseKind::Up(button), x, y);
}

/// Where `text` is on a row below the toolbar.
fn below_toolbar(h: &Harness, text: &str) -> Option<(i32, i32)> {
    let rows = h.screen().lines().count();
    (BAR..rows).find_map(|row| find_in_row(h, text, row))
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_star_keeps_the_page_on_the_bar_and_on_disk_and_a_second_click_lets_it_go() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let links = page("/links");
    let mut h = open(&scratch, &links);
    until(&mut h, "the tab named", |h| find_in_row(h, "Links", 0).is_some());
    assert!(star_is(&h, "browser.star"), "an open star:\n{}", h.screen());
    assert!(!bar(&h).contains("Links"), "no bar without bookmarks:\n{}", h.screen());
    assert_eq!(eval(&h, "innerHeight"), json!(600));

    let (star, gear) = star_and_gear_ink(&h);
    assert_eq!(star, gear, "an open star is drawn like the other buttons");

    click_icon(&mut h, "browser.star");
    until(&mut h, "the bookmark on the bar", |h| bar(h).contains("Links") && star_is(h, "browser.starred"));
    assert_eq!(stored(&scratch), [format!("{links}\tLinks")]);
    until(&mut h, "the note", |h| h.screen().contains("Bookmarked"));
    until_page(&mut h, "innerHeight === 580");

    click_icon(&mut h, "browser.starred");
    until(&mut h, "the bar gone", |h| !bar(h).contains("Links") && star_is(h, "browser.star"));
    assert_eq!(stored(&scratch), Vec::<String>::new());
    until(&mut h, "the note", |h| h.screen().contains("Bookmark removed"));
    until_page(&mut h, "innerHeight === 600");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_kept_pages_star_is_in_the_accent_colour_not_only_filled() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let links = page("/links");
    // Kept before the start, so neither the pointer nor the focus is on the star.
    keep(&scratch, &[(&links, "Links")]);
    let mut h = open(&scratch, &links);
    until(&mut h, "the filled star", |h| star_is(h, "browser.starred"));
    let (star, gear) = star_and_gear_ink(&h);
    assert_ne!(star, gear, "the star stands out from the toolbar's other buttons:\n{}", h.screen());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn ctrl_d_on_the_page_keeps_it_and_lets_it_go() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let form = page("/form");
    let mut h = open(&scratch, &form);
    until(&mut h, "the tab named", |h| find_in_row(h, "Form", 0).is_some());
    h.press("ctrl+d");
    until(&mut h, "the bookmark on the bar", |h| bar(h).contains("Form"));
    assert_eq!(stored(&scratch), [format!("{form}\tForm")]);
    h.press("ctrl+d");
    until(&mut h, "the bar gone", |h| !bar(h).contains("Form"));
    assert_eq!(stored(&scratch), Vec::<String>::new(), "a kept page is let go, never kept twice");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_click_on_the_bar_opens_the_bookmark_in_the_tab_and_a_middle_click_in_a_new_one() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let (second, third) = (page("/second"), page("/third"));
    keep(&scratch, &[(&second, "Second"), (&third, "Third")]);
    let mut h = open(&scratch, &page("/links"));
    click_on_bar(&mut h, "Second");
    until(&mut h, "the second page", |h| h.app().address() == second);
    assert_eq!(eval(&h, "location.href"), json!(second));
    assert_eq!(h.app().tab_count(), 1);

    press_on_bar(&mut h, "Third", MouseButton::Middle);
    until(&mut h, "a new tab on the third page", |h| h.app().tab_count() == 2 && h.app().address() == third);
    until_page(&mut h, "document.title === 'Third'");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_menu_of_a_bookmark_opens_it_in_a_new_tab_and_takes_it_out() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let (links, second) = (page("/links"), page("/second"));
    keep(&scratch, &[(&second, "Second")]);
    let mut h = open(&scratch, &links);
    until_page(&mut h, "innerHeight === 580");

    press_on_bar(&mut h, "Second", MouseButton::Right);
    until(&mut h, "the menu", |h| h.find("Open in new tab").is_some());
    let (x, y) = h.find("Open in new tab").unwrap();
    h.click(x, y);
    until(&mut h, "a new tab on the second page", |h| h.app().tab_count() == 2 && h.app().address() == second);

    press_on_bar(&mut h, "Second", MouseButton::Right);
    until(&mut h, "the menu", |h| h.find("Remove").is_some());
    let (x, y) = h.find("Remove").unwrap();
    h.click(x, y);
    until(&mut h, "the bar gone", |h| !bar(h).contains("Second"));
    assert_eq!(stored(&scratch), Vec::<String>::new());
    until_page(&mut h, "innerHeight === 600");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn bookmarks_that_do_not_fit_are_behind_more() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let third = page("/third");
    let mut kept: Vec<(String, String)> =
        (1..12).map(|n| (page(&format!("/nowhere/{n}")), format!("Kept page number {n:02}"))).collect();
    kept.push((third.clone(), "The last one".to_owned()));
    let pairs: Vec<(&str, &str)> = kept.iter().map(|(url, name)| (url.as_str(), name.as_str())).collect();
    keep(&scratch, &pairs);
    let mut h = open(&scratch, &page("/links"));
    assert!(bar(&h).contains("Kept page number 01"), "the first ones on the bar:\n{}", h.screen());
    assert!(!h.screen().contains("The last one"), "the last one does not fit:\n{}", h.screen());
    assert!(bar(&h).trim_end().ends_with("More"), "More at the bar's end:\n{}", h.screen());

    click_on_bar(&mut h, "More");
    until(&mut h, "the rest shown", |h| below_toolbar(h, "The last one").is_some());
    let (x, y) = below_toolbar(&h, "The last one").unwrap();
    h.click(x, y);
    until(&mut h, "the last one opened", |h| h.app().address() == third);
    until(&mut h, "the rest hidden", |h| below_toolbar(h, "The last one").is_none());
}

/// The screen with the bookmarks Second, Third and a form kept, on the links page, with the
/// address bar open and `typed` typed in it. The form's bookmark has a query the form page's own
/// address lacks, so typing that address suggests the bookmark without being it.
fn typing(scratch: &Scratch, typed: &str) -> Harness {
    keep(scratch, &[(&page("/second"), "Second"), (&page("/third"), "Third"), (&page("/form?kept"), "Kept form")]);
    let mut h = open(scratch, &page("/links"));
    h.press("ctrl+l").type_text(typed);
    h
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn typing_in_the_address_bar_suggests_bookmarks_and_the_arrows_and_enter_open_one() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let (second, third) = (page("/second"), page("/third"));
    let mut h = typing(&scratch, "hir");
    until(&mut h, "the suggestion", |h| below_toolbar(h, &third).is_some());
    assert!(below_toolbar(&h, &second).is_none(), "only what holds the typed text:\n{}", h.screen());
    // The list is as wide as the field: a suggestion's address stands at its right end, just
    // short of the star after the field, not where the list's own content would end.
    let (star, _) = find_in_row(&h, &h.env().icons().glyph("browser.star"), 1).expect("the star");
    let (at, _) = below_toolbar(&h, &third).expect("the suggestion");
    let end = at + i32::try_from(third.len()).unwrap();
    assert!((star - 6..star).contains(&end), "the address ends at {end}, the star is at {star}:\n{}", h.screen());
    h.press("ctrl+a").type_text("127.0.0.1");
    until(&mut h, "every bookmark suggested", |h| {
        [&second, &third, &page("/form")].iter().all(|url| below_toolbar(h, url).is_some())
    });
    h.press("down").press("down").press("enter");
    until(&mut h, "the second suggestion opened", |h| h.app().address() == third);
    assert_eq!(eval(&h, "location.href"), json!(third));
    assert!(below_toolbar(&h, &second).is_none(), "the list is closed:\n{}", h.screen());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_click_on_a_suggestion_opens_it() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let third = page("/third");
    let mut h = typing(&scratch, "THIRD");
    until(&mut h, "the suggestion", |h| below_toolbar(h, &third).is_some());
    let (x, y) = below_toolbar(&h, &third).unwrap();
    h.click(x, y);
    until(&mut h, "the third page", |h| h.app().address() == third);
    until_page(&mut h, "document.title === 'Third'");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn enter_without_a_chosen_suggestion_goes_to_the_typed_address() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let form = page("/form");
    let mut h = typing(&scratch, &form);
    until(&mut h, "the kept form suggested", |h| below_toolbar(h, "Kept form").is_some());
    h.press("enter");
    until(&mut h, "the typed address", |h| h.app().address() == form);
    until_page(&mut h, "document.title === 'Form'");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn esc_closes_the_suggestions_and_keeps_the_typed_text() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = typing(&scratch, "second");
    until(&mut h, "the suggestion", |h| below_toolbar(h, &page("/second")).is_some());
    h.press("esc");
    until(&mut h, "the list closed", |h| below_toolbar(h, &page("/second")).is_none());
    assert!(h.is_focused("location"), "the field keeps the keyboard");
    assert!(find_in_row(&h, "second", 1).is_some(), "the typed text stays:\n{}", h.screen());
    h.press("esc");
    until(&mut h, "the address back", |h| find_in_row(h, &page("/links"), 1).is_some());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn bookmarks_are_there_after_a_restart() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let links = page("/links");
    let mut h = open(&scratch, &links);
    until(&mut h, "the tab named", |h| find_in_row(h, "Links", 0).is_some());
    click_icon(&mut h, "browser.star");
    until(&mut h, "the bookmark on the bar", |h| bar(h).contains("Links"));
    drop(h);
    let mut h = open_on(scratch.machine(), Some(&page("/second")));
    until(&mut h, "the page drawn", page_drawn);
    assert!(bar(&h).contains("Links"), "the bookmark is back:\n{}", h.screen());
    click_on_bar(&mut h, "Links");
    until(&mut h, "the bookmark opened", |h| h.app().address() == links);
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn broken_lines_in_the_file_are_skipped() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let second = page("/second");
    write_file(&scratch, &format!("just words\n\u{0}\t\n{second}\tSecond\n\tNameless\n{second}\tTwice\n"));
    let h = open(&scratch, &page("/links"));
    assert!(bar(&h).contains("Second"), "the good line is on the bar:\n{}", h.screen());
    for broken in ["just words", "Nameless", "Twice"] {
        assert!(!bar(&h).contains(broken), "{broken} is skipped:\n{}", h.screen());
    }
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_file_that_cannot_be_written_is_said_in_a_warning() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    // A folder where the file should be: reading finds no list, writing fails.
    std::fs::create_dir_all(file(&scratch)).unwrap();
    let mut h = open(&scratch, &page("/links"));
    until(&mut h, "the tab named", |h| find_in_row(h, "Links", 0).is_some());
    click_icon(&mut h, "browser.star");
    until(&mut h, "the warning", |h| h.screen().contains("The bookmarks could not be saved"));
    assert!(bar(&h).contains("Links"), "the bookmark stays for this run:\n{}", h.screen());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_bookmark_another_window_kept_meanwhile_stays_when_the_star_keeps_this_page() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/links"));
    until(&mut h, "the tab named", |h| find_in_row(h, "Links", 0).is_some());
    // A second qbrowser window, started on a temporary profile, kept a page after this one read
    // the file.
    let other = page("/third");
    keep(&scratch, &[(&other, "Third")]);
    click_icon(&mut h, "browser.star");
    until(&mut h, "this page on the bar", |h| bar(h).contains("Links"));
    assert_eq!(stored(&scratch), [format!("{other}\tThird"), format!("{}\tLinks", page("/links"))]);
    assert!(bar(&h).contains("Third"), "the other window's bookmark is on this bar too:\n{}", h.screen());
}

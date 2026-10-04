//! What the person's hands get from the page's selection and from a right click on the page: the
//! copy key, the two copies, the menu with the keyboard and with the mouse, and the entries that
//! are not there because they could not do anything.

use std::time::Duration;

use qframe::event::{MouseButton, MouseKind};
use qframe::icons::GlyphMode;
use qframe::prelude::Harness;
use serde_json::{Value, json};

use super::super::Browser;
use super::{CELL, PAGE_TOP, Scratch, Slot, eval, open, page, until};

/// The words the selection tests read. The clipboard has to hold these and not a constant, so
/// they come from the page.
const WORDS: &str = "the quick brown fox";

/// A small web server of its own that answers `body` to everything asked of it, so this file
/// needs nothing from the fixture's one shared list of pages.
fn serve_html(body: &str) -> String {
    use std::io::{BufRead, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let body = body.to_owned();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = std::io::BufReader::new(&stream);
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let mut out = &stream;
            let _ = out.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            );
        }
    });
    format!("http://127.0.0.1:{port}/")
}

/// A page with one big paragraph and a listener that remembers what the browser hands the page:
/// a right press, and the copy chord.
fn listening_page() -> String {
    serve_html(&format!(
        "<!doctype html><html><body style='margin:0'><p id=words style='font-size:40px;width:760px'>{WORDS}</p>\
         <script>window.heard = {{ right: 0, copy: 0, double: 0 }};\
         addEventListener('mousedown', e => {{ if (e.button === 2) window.heard.right++ }});\
         addEventListener('dblclick', () => window.heard.double++);\
         addEventListener('keydown', e => {{ if (e.key === 'c' && (e.ctrlKey || e.metaKey)) window.heard.copy++ }});\
         </script></body></html>"
    ))
}

/// A page with a snippet that keeps its own shape: a run of spaces and a line of its own.
fn snippet_page() -> String {
    serve_html(
        "<!doctype html><html><body style='margin:0'>\
         <pre id=code style=\"position:absolute;left:20px;top:20px;font-size:30px;line-height:1.2;margin:0\">alpha   beta\ngamma   delta</pre>\
         </body></html>",
    )
}

/// Selects `selector` with a drag of a person's hand: down inside its top left corner, dragged to
/// inside its bottom right corner, up there.
fn drag_over(h: &mut Harness<Browser>, selector: &str) {
    let rect = eval(
        h,
        &format!(
            "(() => {{ const r = document.querySelector('{selector}').getBoundingClientRect(); return [r.x, r.y, r.right, r.bottom]; }})()"
        ),
    );
    let at = |n: usize| rect[n].as_f64().unwrap();
    let column = |x: f64| (x / f64::from(CELL.0)).floor() as i32;
    let row = |y: f64| (y / f64::from(CELL.1)).floor() as i32 + PAGE_TOP;
    // Released clear of the element's edge: letting go a hair past the last glyph leaves Chromium
    // with nothing selected, which is its own business and not what this drag is measuring. Never
    // off the page area either: a press outside the screen lands on nothing and starts nothing.
    let (from, to) =
        ((column(at(0) + 1.0).max(0), row(at(1) + 1.0).max(PAGE_TOP)), (column(at(2) - 8.0), row(at(3) - 8.0)));
    h.mouse(MouseKind::Moved, from.0, from.1);
    h.mouse(MouseKind::Down(MouseButton::Left), from.0, from.1);
    for step in 1..=4 {
        let x = from.0 + (to.0 - from.0) * step / 4;
        let y = from.1 + (to.1 - from.1) * step / 4;
        h.mouse(MouseKind::Drag(MouseButton::Left), x, y);
    }
    h.mouse(MouseKind::Up(MouseButton::Left), to.0, to.1);
}

/// The screen on `address`, once its page is drawn and its document is there to be asked.
///
/// A page that is still on its way has a picture before it has a document, and an expression cannot
/// be run in a document that is not there yet. The tests here ask the page what it heard and where
/// its text is, so they wait for the page to be able to answer before they start.
fn open_ready(scratch: &Scratch, address: &str) -> Harness<Browser> {
    let mut h = open(scratch, address);
    until(&mut h, "the page's document", |h| page_answers(h, "1 + 1"));
    h
}

/// Whether the page on screen can answer `expression` at all. A page mid-navigation answers with
/// the reason it cannot, which is not a failure of the test but a page that is not ready.
fn page_answers(h: &Harness<Browser>, expression: &str) -> bool {
    let browser = h.app();
    let (Some(engine), Some(tab)) = (browser.engine(), browser.active_tab()) else { return false };
    engine.evaluate(tab, expression, super::PATIENCE).is_ok()
}

/// The screen cell where the middle of `selector` is drawn, once the page has it: a page that is
/// loading again has no such element to measure, and the drag and the click need a real one.
fn cell_of(h: &mut Harness<Browser>, selector: &str) -> (i32, i32) {
    until(h, &format!("`{selector}` in the page"), |h| {
        page_answers(h, &format!("!!document.querySelector('{selector}')"))
    });
    super::cell_of(h, selector)
}

/// The selection the page has reported, once there is one.
fn selected(h: &mut Harness<Browser>) -> String {
    until(h, "the page's selection reported", |h| !h.app().tab().selection.is_empty());
    h.app().tab().selection.clone()
}

/// The text the last copy put on the clipboard.
fn copied(h: &Harness<Browser>) -> String {
    h.copied().last().cloned().unwrap_or_default()
}

/// A right press on the page at (`x`, `y`) and the release after it.
fn right_click(h: &mut Harness<Browser>, x: i32, y: i32) {
    h.mouse(MouseKind::Down(MouseButton::Right), x, y);
    h.mouse(MouseKind::Up(MouseButton::Right), x, y);
}

/// Whether the menu's row that says `label` is drawn as one that cannot be chosen.
fn row_is_muted(h: &Harness<Browser>, label: &str) -> bool {
    let (x, y) = h.find(label).unwrap_or_else(|| panic!("no `{label}` on screen:\n{}", h.screen()));
    let (x, y) = (u16::try_from(x).unwrap(), u16::try_from(y).unwrap());
    h.fg(x, y) == h.env().theme().color("muted")
}

/// What the page heard of the browser's own input.
fn heard(h: &Harness<Browser>, what: &str) -> Value {
    eval(h, &format!("window.heard.{what}"))
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn selecting_text_in_the_page_and_pressing_ctrl_c_copies_it() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open_ready(&scratch, &listening_page());
    drag_over(&mut h, "#words");
    let selection = selected(&mut h);
    assert!(selection.contains("quick brown fox"), "the page's own words: {selection:?}");

    h.press("ctrl+c");
    assert!(copied(&h).contains("quick brown fox"), "the clipboard holds what the page selected: {}", h.screen());
    assert_eq!(heard(&h, "copy"), Value::from(0), "the page was not asked to copy it itself");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_double_click_still_reaches_the_page_as_one() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open_ready(&scratch, &listening_page());
    let (x, y) = cell_of(&mut h, "#words");
    // Two presses on the same cell with no time between them: the framework counts the second as
    // the second of a pair, and the page must hear it as the browser's own double click.
    h.click(x, y).click(x, y);
    until(&mut h, "the page's double click", |h| heard(h, "double") == json!(1));
    let word = selected(&mut h);
    assert!(!word.trim().is_empty() && !word.trim().contains(' '), "a double click selects one word: {word:?}");
    assert!(WORDS.split(' ').any(|w| w == word.trim()), "one of the page's own words: {word:?}");
    assert!(h.copied().is_empty(), "and selecting it copied nothing");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn letting_go_does_not_copy() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open_ready(&scratch, &listening_page());
    drag_over(&mut h, "#words");
    selected(&mut h);
    assert!(h.copied().is_empty(), "letting go of a selection copies nothing: only ctrl+c or the menu copies");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn copy_gives_clean_text_and_raw_copy_gives_the_shapes() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open_ready(&scratch, &snippet_page());
    drag_over(&mut h, "#code");
    let selection = selected(&mut h);
    assert!(selection.contains('\n'), "the snippet kept its line: {selection:?}");
    assert!(selection.contains("   "), "and its run of spaces: {selection:?}");

    // The raw copy, by keyboard: Shift+F10, down to it, enter.
    h.press("shift+f10");
    assert!(h.screen().contains("Raw copy"), "the menu is on screen:\n{}", h.screen());
    h.press("down").press("enter");
    assert_eq!(copied(&h), selection, "the raw copy is the page's own characters");

    // The clean copy, by keyboard again: the first row is Copy.
    h.press("shift+f10").press("enter");
    let clean = copied(&h);
    assert!(!clean.contains('\n'), "one line, as it would be pasted into a field: {clean:?}");
    assert!(!clean.contains("   "), "and no run of spaces is left: {clean:?}");
    assert!(!clean.starts_with(' ') && !clean.ends_with(' '), "the ends are trimmed: {clean:?}");
    assert!(
        clean.split_whitespace().collect::<Vec<_>>().join(" ") == clean,
        "every run of whitespace is one space: {clean:?}"
    );
    assert!(clean.starts_with("alpha beta gamma"), "the page's own words are in it: {clean:?}");

    // And by the mouse, on the same two rows.
    h.press("shift+f10");
    let (_, row) = h.find("Raw copy").expect("the raw copy row");
    h.click(6, row);
    assert_eq!(copied(&h), selection, "the mouse chooses the raw copy too");
    h.press("shift+f10");
    let (_, row) = h.find("Copy").expect("the copy row");
    h.click(6, row);
    assert_eq!(copied(&h), clean, "and the clean one, the very same text");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_address_bar_keeps_its_own_copy() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let address = listening_page();
    let mut h = open_ready(&scratch, &address);
    h.press("ctrl+l");
    assert!(h.is_focused("location"), "the keyboard is in the address field");
    // The field selects all of its text when it takes focus, so the copy has something to take.
    h.press("ctrl+c");
    assert_eq!(copied(&h), address, "the field's own selection is what is copied");
    assert_eq!(heard(&h, "copy"), Value::from(0), "and the page is never asked");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn ctrl_c_with_no_selection_still_reaches_the_page() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open_ready(&scratch, &listening_page());
    assert!(h.app().tab().selection.is_empty(), "nothing is selected");
    h.press("ctrl+c");
    until(&mut h, "the copy chord at the page", |h| heard(h, "copy") == json!(1));
    assert!(h.copied().is_empty(), "and nothing is written to the clipboard");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_menu_opens_on_a_right_click_where_the_pointer_is() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open_ready(&scratch, &listening_page());
    let (x, y) = (40, PAGE_TOP + 3);
    right_click(&mut h, x, y);
    let (label_x, label_y) = h.find("Copy").unwrap_or_else(|| panic!("no menu:\n{}", h.screen()));
    assert_eq!(label_y, y + 1, "the menu opens on the row under the pointer:\n{}", h.screen());
    assert!(label_x >= x, "starting at the pointer's column:\n{}", h.screen());
    assert_eq!(heard(&h, "right"), Value::from(0), "the page did not receive the press");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_menu_is_reached_from_the_keyboard_too() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open_ready(&scratch, &snippet_page());
    drag_over(&mut h, "#code");
    let selection = selected(&mut h);

    h.press("shift+f10");
    assert!(h.screen().contains("Raw copy"), "shift+f10 opens the menu:\n{}", h.screen());
    h.press("esc");
    assert!(!h.screen().contains("Raw copy"), "esc closes it and chooses nothing");
    assert!(h.copied().is_empty(), "esc copies nothing");

    h.press("menu");
    assert!(h.screen().contains("Raw copy"), "the menu key opens it too:\n{}", h.screen());
    // Up from the first row wraps round to the last row that can be chosen, the reload.
    h.press("up").press("enter");
    assert!(!h.screen().contains("Raw copy"), "enter chose a row and closed the menu:\n{}", h.screen());
    assert!(h.copied().is_empty(), "the reload row copies nothing");

    // Down reaches the raw copy, and enter chooses it.
    h.press("shift+f10").press("down").press("enter");
    assert_eq!(copied(&h), selection, "the arrows and enter reach the raw copy");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_click_outside_the_menu_closes_it_and_still_reaches_what_it_landed_on() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open_ready(&scratch, &page("/links"));
    let (link_x, link_y) = cell_of(&mut h, "#next");
    // The menu opens at the pointer, so it is opened well away from the link.
    right_click(&mut h, 70, PAGE_TOP + 1);
    assert!(h.screen().contains("Raw copy"), "the menu is open:\n{}", h.screen());

    h.click(link_x, link_y);
    assert!(!h.screen().contains("Raw copy"), "the click closed the menu:\n{}", h.screen());
    let second = page("/second");
    until(&mut h, "the page followed the link", |h| h.app().address() == second);
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn copy_and_raw_copy_are_disabled_with_nothing_selected() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open_ready(&scratch, &page("/links"));
    right_click(&mut h, 40, PAGE_TOP + 3);
    let screen = h.screen();
    for label in ["Copy", "Raw copy", "Back", "Forward"] {
        assert!(screen.contains(label), "the row is there:\n{screen}");
    }
    assert!(row_is_muted(&h, "Copy"), "with nothing selected Copy cannot be chosen");
    assert!(row_is_muted(&h, "Raw copy"), "and Raw copy cannot either");
    assert!(row_is_muted(&h, "Back"), "nor Back on a tab with nowhere to go back to");
    // A disabled row cannot be reached with the arrows either: every one of these chooses a row
    // that can do something, and the reload copies nothing.
    for keys in [1, 2, 3] {
        h.press("esc");
        right_click(&mut h, 40, PAGE_TOP + 3);
        for _ in 0..keys {
            h.press("down");
        }
        h.press("enter");
        assert!(h.copied().is_empty(), "no disabled row is chosen with {keys} presses down:\n{}", h.screen());
    }

    // Where there is somewhere to go, Back is live.
    let (link_x, link_y) = cell_of(&mut h, "#next");
    h.click(link_x, link_y);
    let second = page("/second");
    until(&mut h, "the second page", |h| h.app().address() == second);
    right_click(&mut h, 40, PAGE_TOP + 3);
    assert!(!row_is_muted(&h, "Back"), "with a page behind it Back can be chosen:\n{}", h.screen());
    assert!(row_is_muted(&h, "Forward"), "and Forward cannot, with nothing ahead:\n{}", h.screen());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_reload_row_becomes_stop_while_the_page_loads() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open_ready(&scratch, &page("/links"));
    right_click(&mut h, 40, PAGE_TOP + 3);
    let screen = h.screen();
    assert!(screen.contains("Reload this page"), "the reload row is there:\n{screen}");
    h.press("esc");

    h.press("ctrl+l").type_text(&page("/slow")).press("enter");
    until(&mut h, "the slow page loading", |h| h.app().tab().loading);
    right_click(&mut h, 40, PAGE_TOP + 3);
    let screen = h.screen();
    assert!(screen.contains("Stop loading"), "the row is Stop while loading:\n{screen}");
    assert!(!screen.contains("Reload this page"), "and not the reload:\n{screen}");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_right_click_over_a_link_offers_to_open_it_in_a_new_tab_and_to_copy_its_address() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open_ready(&scratch, &page("/links"));
    let (x, y) = cell_of(&mut h, "#next");
    right_click(&mut h, x, y);
    until(&mut h, "the link entries", |h| {
        h.screen().contains("Open in new tab") && h.screen().contains("Copy address")
    });
    let second = page("/second");
    let screen = h.screen();
    assert!(screen.contains(&second), "the address is on the rows, so the link is known before choosing:\n{screen}");

    let (address_x, address_y) = h.find("Copy address").expect("the copy address row");
    h.click(address_x + 2, address_y);
    assert_eq!(copied(&h), second, "the address really is copied");

    right_click(&mut h, x, y);
    until(&mut h, "the link entries again", |h| h.screen().contains("Open in new tab"));
    let (open_x, open_y) = h.find("Open in new tab").expect("the open in a new tab row");
    h.click(open_x + 2, open_y);
    until(&mut h, "the second tab", |h| h.app().tab_count() == 2);
    assert_eq!(h.app().address(), second, "and it is on the address the link had");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_right_click_where_there_is_no_link_offers_no_link_entries() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open_ready(&scratch, &page("/links"));
    // A link is known first, so a right press somewhere else must not go on offering its address.
    let (x, y) = cell_of(&mut h, "#next");
    right_click(&mut h, x, y);
    until(&mut h, "the link entries", |h| h.screen().contains("Open in new tab"));
    h.press("esc");

    // Read as the press lands, before the page could have answered about this pointer: what is
    // on the menu then is what the screen knew, and it must not be the last press's link.
    h.mouse(MouseKind::Down(MouseButton::Right), 80, PAGE_TOP + 10);
    let screen = h.screen();
    h.mouse(MouseKind::Up(MouseButton::Right), 80, PAGE_TOP + 10);
    assert!(screen.contains("Raw copy"), "the menu is open:\n{screen}");
    assert!(!screen.contains("Open in new tab"), "another press's link is not this one's:\n{screen}");
    assert!(!screen.contains("Copy address"), "nor the row that would copy its address:\n{screen}");
    // Long enough for the answer to the question about this pointer to have landed.
    for _ in 0..20 {
        h.advance(Duration::from_millis(20));
        std::thread::sleep(Duration::from_millis(10));
    }
    let screen = h.screen();
    assert!(!screen.contains("Open in new tab"), "no link, no such row:\n{screen}");
    assert!(!screen.contains("Copy address"), "and neither the other:\n{screen}");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_selection_in_another_tab_does_not_answer_for_this_one() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open_ready(&scratch, &listening_page());
    drag_over(&mut h, "#words");
    selected(&mut h);

    h.press("ctrl+t");
    h.type_text(&page("/second")).press("enter");
    until(&mut h, "the second tab's page", |h| h.app().address() == page("/second"));
    assert!(h.app().tab().selection.is_empty(), "the new tab has no selection of its own");

    h.press("ctrl+c");
    assert!(h.copied().is_empty(), "nothing is copied from the other tab:\n{}", h.screen());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn narrow_screens_and_ascii_keep_the_menu_usable() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open_ready(&scratch, &snippet_page());
    h.resize(40, 20).set_glyph_mode(GlyphMode::Ascii);
    drag_over(&mut h, "#code");
    selected(&mut h);
    right_click(&mut h, 4, PAGE_TOP + 1);

    let screen = h.screen();
    for label in ["Copy", "Raw copy", "Back", "Forward", "Reload this page"] {
        assert!(screen.contains(label), "`{label}` is whole in 40 columns and ASCII:\n{screen}");
    }
    for line in screen.lines() {
        for c in line.chars() {
            assert!(c == '\n' || c.is_ascii_graphic() || c == ' ', "only ASCII on screen, found {c:?}:\n{screen}");
        }
    }
    for decoration in ["[", "]", "(", ")", "|", "==="] {
        assert!(!screen.contains(decoration), "no forbidden decoration {decoration:?}:\n{screen}");
    }
    // The rows are still there to choose, in this narrow window too.
    let (_, row) = h.find("Raw copy").expect("the raw copy row");
    h.click(6, row);
    assert!(copied(&h).contains('\n'), "and choosing one still copies the page's own text");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_right_click_over_a_link_finds_it_at_any_zoom() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    // A small link away from the corner, so that a point read at the wrong scale misses it.
    let target = page("/second");
    let mut h = open_ready(
        &scratch,
        &serve_html(&format!(
            "<!doctype html><html><body style='margin:0'><a id=next href='{target}' \
             style='position:absolute;left:300px;top:150px;width:60px;height:40px;display:block'>go</a></body></html>"
        )),
    );
    cell_of(&mut h, "#next");
    // Zoomed in until the page says it is drawn larger: one CSS pixel is then more than one of the
    // page area's own, and a question asked in the area's pixels would land beside the link.
    for _ in 0..3 {
        h.press("ctrl+=");
    }
    until(&mut h, "the page drawn larger", |h| {
        eval(h, "window.devicePixelRatio").as_f64().is_some_and(|ratio| ratio > 1.4)
    });
    let rect = eval(
        &h,
        "(() => { const r = document.querySelector('#next').getBoundingClientRect(); \
         return [(r.x + r.width / 2) * devicePixelRatio, (r.y + r.height / 2) * devicePixelRatio]; })()",
    );
    let at = |n: usize, cell: u32| (rect[n].as_f64().unwrap() / f64::from(cell)).floor();
    #[expect(clippy::cast_possible_truncation, reason = "a cell of the test screen")]
    let (x, y) = (at(0, CELL.0) as i32, at(1, CELL.1) as i32 + PAGE_TOP);
    right_click(&mut h, x, y);
    until(&mut h, "the link entries at this zoom", |h| h.screen().contains("Open in new tab"));
    assert!(h.screen().contains(&target), "the link under the pointer, not another:\n{}", h.screen());
}

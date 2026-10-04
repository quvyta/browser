//! History from where the person clicks and presses: the lists the two arrows open, ctrl+h and the
//! pages it holds, taking a visit out, the file beside the profile and what a broken or unwritable
//! one costs, and a narrow screen in ASCII.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use qframe::event::{MouseButton, MouseKind};
use qframe::icons::GlyphMode;
use serde_json::json;

use super::{Scratch, Slot, cell_of, eval, find_in_row, open, open_on, page, page_drawn, until, until_page};
use crate::app::Browser;

type Harness = qframe::runtime::Harness<Browser>;

/// The row of the toolbar, where the two arrows and the address are.
const BAR: usize = 1;

/// The first row of the body: the page, the history screen, or a list opened over the page.
const BODY: usize = 2;

/// Serves `body` once on this machine and gives the address it is at, for the pages a history test
/// needs and the shared server has not: a title long enough that a row has to be cut.
fn serve_html(body: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port of its own");
    let port = listener.local_addr().expect("the address of that port").port();
    let body = body.to_owned();
    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else { return };
        let mut reader = BufReader::new(stream.try_clone().expect("a second handle on the stream"));
        let mut line = String::new();
        while reader.read_line(&mut line).is_ok_and(|read| read > 2) {
            line.clear();
        }
        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n";
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(body.as_bytes());
    });
    format!("http://127.0.0.1:{port}/")
}

/// The history file of the scratch machine's profile: beside the profile folder, never in the Quvyta
/// data folder where the bookmarks are.
fn file(scratch: &Scratch) -> PathBuf {
    scratch.path("home/history")
}

/// The file's lines, as the visits are kept on them.
fn kept(scratch: &Scratch) -> Vec<String> {
    std::fs::read_to_string(file(scratch)).unwrap_or_default().lines().map(str::to_owned).collect()
}

/// Puts `text` in the history file, as qbrowser finds it at start.
fn keep(scratch: &Scratch, text: &str) {
    let path = file(scratch);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// The row of the screen `text` is on, anywhere below the toolbar.
fn body_row(h: &Harness, text: &str) -> Option<i32> {
    (BODY..h.screen().lines().count())
        .find(|row| find_in_row(h, text, *row).is_some())
        .and_then(|row| i32::try_from(row).ok())
}

/// Where `text` is on the screen, below the toolbar.
fn body_at(h: &Harness, text: &str) -> (i32, i32) {
    let at = body_row(h, text).unwrap_or_else(|| panic!("no {text} on the screen:\n{}", h.screen()));
    let row = usize::try_from(at).unwrap();
    find_in_row(h, text, row).unwrap_or_else(|| panic!("no {text} on row {row}:\n{}", h.screen()))
}

/// The toolbar's row, whole: what the address bar sits among and what must not move under a list.
fn toolbar(h: &Harness) -> String {
    h.screen().lines().nth(BAR).unwrap_or_default().to_owned()
}

/// Where the list button beside the arrow drawn with `arrow` is: the first list mark after it.
fn steps_at(h: &Harness, arrow: &str) -> (i32, i32) {
    let glyphs = h.env().icons();
    let (arrow, down) = (glyphs.glyph(arrow).into_owned(), glyphs.glyph("chevron-down").into_owned());
    let line = h.screen().lines().nth(BAR).unwrap_or_default().to_owned();
    let at = line.find(&arrow).unwrap_or_else(|| panic!("no {arrow} on the toolbar:\n{}", h.screen()));
    let tail = &line[at + arrow.len()..];
    let mark = tail.find(&down).unwrap_or_else(|| panic!("no list button beside {arrow}:\n{}", h.screen()));
    let column = line[..at + arrow.len() + mark].chars().count();
    (i32::try_from(column).ok().unwrap(), i32::try_from(BAR).unwrap())
}

/// Opens the list beside the arrow drawn with `arrow`.
fn open_steps(h: &mut Harness, arrow: &str) {
    let (x, y) = steps_at(h, arrow);
    h.click(x, y);
}

/// Moves the pointer off the toolbar, so that no tooltip is drawn over what a test reads.
fn away(h: &mut Harness) {
    h.hover(2, 19).advance(Duration::from_millis(120));
}

/// The tab on the /links page, walked on to /second and /third by the person's own hands: a click on
/// the link, then the address bar. The tab is at /third and its own steps are all three.
fn walked(scratch: &Scratch) -> (Harness, String, String, String) {
    let (links, second, third) = (page("/links"), page("/second"), page("/third"));
    let mut h = open(scratch, &links);
    let (x, y) = cell_of(&h, "#next");
    h.click(x, y);
    until(&mut h, "the second page", |h| h.app().address() == second);
    h.press("ctrl+l").type_text(&third).press("enter");
    until(&mut h, "the third page", |h| h.app().address() == third);
    until_page(&mut h, "document.title === 'Third'");
    // The address bar is set before Chromium goes, so the tab's own list is waited for as well.
    until(&mut h, "the tab's own three steps", |h| {
        h.app().tab().history.as_ref().is_some_and(|(current, entries)| *current == 2 && entries.len() == 3)
    });
    (h, links, second, third)
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_engine_reports_the_tabs_own_steps_and_which_one_it_is_at() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let (h, links, second, third) = walked(&scratch);
    let (current, entries) = h.app().tab().history.clone().expect("the tab's own steps");
    assert_eq!(current, 2, "the tab is at the third of the three it walked");
    assert_eq!(
        entries.iter().map(|entry| entry.url.as_str()).collect::<Vec<_>>(),
        [links.as_str(), second.as_str(), third.as_str()],
        "Chromium's own list, oldest first"
    );
    // Read from the page as well, so the list cannot be one the application made up.
    assert_eq!(eval(&h, "history.length"), json!(3), "the page counts the same three steps");
    assert_eq!(eval(&h, "location.href"), json!(third), "the page is at the step the list says it is at");
    assert_eq!(eval(&h, "history.state"), json!(null), "these pages push no state of their own");
    assert_eq!(h.app().address(), third, "and the address bar is where the third step is");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_back_arrow_lists_where_the_tab_has_been() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let (mut h, links, second, third) = walked(&scratch);
    open_steps(&mut h, "chevron-left");
    until(&mut h, "the list of the earlier steps", |h| body_row(h, &second).is_some());
    away(&mut h);
    for earlier in [&second, &links] {
        assert!(body_row(&h, earlier).is_some(), "every earlier step is on the list:\n{}", h.screen());
    }
    let row = |h: &Harness, at: &str| body_row(h, at).unwrap_or_else(|| panic!("no {at} on the list:\n{}", h.screen()));
    let (second_row, links_row, third_row) = (row(&h, &second), row(&h, &links), row(&h, &third));
    assert!(second_row < links_row, "nearest first:\n{}", h.screen());
    assert!(third_row > links_row, "the one the tab is at closes the list:\n{}", h.screen());
    // Marked by a sign of its own, and drawn faint where the earlier step that is not chosen is
    // not: where the tab is does not rest on colour alone.
    let dot = h.env().icons().glyph("dot").into_owned();
    let line = |row: i32| h.screen().lines().nth(usize::try_from(row).unwrap()).unwrap_or_default().to_owned();
    assert!(line(third_row).contains(&dot), "the step the tab is at carries the mark:\n{}", h.screen());
    assert!(!line(links_row).contains(&dot), "a place to go does not:\n{}", h.screen());
    // The label is the page's title, or its address while Chromium had none yet: either way the
    // first letter on the row is where it starts.
    let label = |row: i32| {
        let x = line(row).chars().position(char::is_alphanumeric).expect("the row's label");
        h.fg(u16::try_from(x).unwrap(), u16::try_from(row).unwrap())
    };
    assert_ne!(label(third_row), label(links_row), "and is drawn faint:\n{}", h.screen());
    h.press("esc");
    until(&mut h, "the list closed", |h| body_row(h, &second).is_none());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn choosing_a_step_from_the_list_goes_there() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let (mut h, links, second, third) = walked(&scratch);
    // Enter on the row the list opens on: the nearest step.
    open_steps(&mut h, "chevron-left");
    until(&mut h, "the list open", |h| body_row(h, &second).is_some());
    away(&mut h);
    h.press("enter");
    until(&mut h, "the second page again", |h| h.app().address() == second);
    assert_eq!(eval(&h, "location.href"), json!(second), "the page is where the address bar says");
    assert_eq!(eval(&h, "history.length"), json!(3), "stepped back to, not opened again over the walk");

    // And a click on the row of the oldest step, after walking on to the third page again.
    h.press("ctrl+l").type_text(&third).press("enter");
    until_page(&mut h, "document.title === 'Third'");
    until(&mut h, "the tab's own three steps again", |h| {
        h.app().tab().history.as_ref().is_some_and(|(current, entries)| *current == 2 && entries.len() == 3)
    });
    open_steps(&mut h, "chevron-left");
    until(&mut h, "the list open again", |h| body_row(h, &links).is_some());
    away(&mut h);
    let (x, y) = body_at(&h, &links);
    h.click(x, y);
    until(&mut h, "the first page", |h| h.app().address() == links);
    assert_eq!(eval(&h, "location.href"), json!(links));
    until(&mut h, "the list closed behind it", |h| body_row(h, &second).is_none());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_middle_click_on_a_step_opens_it_in_a_new_tab() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let (mut h, _links, second, _third) = walked(&scratch);
    open_steps(&mut h, "chevron-left");
    until(&mut h, "the list open", |h| body_row(h, &second).is_some());
    away(&mut h);
    let (x, y) = body_at(&h, &second);
    h.mouse(MouseKind::Down(MouseButton::Middle), x, y);
    h.mouse(MouseKind::Up(MouseButton::Middle), x, y);
    until(&mut h, "a new tab on the second page", |h| h.app().tab_count() == 2 && h.app().address() == second);
    until_page(&mut h, "document.title === 'Second'");
    assert_eq!(h.app().tab_count(), 2, "a middle click opens in a new tab, as it does everywhere else");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_list_buttons_are_disabled_with_nowhere_to_go() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let links = page("/links");
    let mut h = open(&scratch, &links);
    until(&mut h, "the tab named", |h| find_in_row(h, "Links", 0).is_some());
    for arrow in ["chevron-left", "chevron-right"] {
        let (x, y) = steps_at(&h, arrow);
        h.click(x, y);
    }
    away(&mut h);
    assert!(body_row(&h, &links).is_none(), "a press on a disabled button opens nothing:\n{}", h.screen());
    assert!(!h.screen().contains("Where the tab has been"), "and says nothing either:\n{}", h.screen());
    // The arrow beside it is disabled too, and one step back makes the list live.
    let (x, y) = cell_of(&h, "#next");
    h.click(x, y);
    let second = page("/second");
    until(&mut h, "the second page", |h| h.app().address() == second);
    open_steps(&mut h, "chevron-left");
    until(&mut h, "the back steps now", |h| body_row(h, &second).is_some());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_history_screen_lists_what_was_seen_newest_first() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let (mut h, links, second, third) = walked(&scratch);
    away(&mut h);
    h.press("ctrl+h");
    until(&mut h, "the history screen", |h| find_in_row(h, "History", BODY).is_some());
    for seen in [&third, &second, &links] {
        assert!(body_row(&h, seen).is_some(), "{seen} is on the screen:\n{}", h.screen());
    }
    let row =
        |h: &Harness, at: &str| body_row(h, at).unwrap_or_else(|| panic!("no {at} on the screen:\n{}", h.screen()));
    assert!(row(&h, &third) < row(&h, &second), "the newest first:\n{}", h.screen());
    assert!(row(&h, &second) < row(&h, &links), "and then the one before it:\n{}", h.screen());
    let today = body_row(&h, "Today").expect("the day the visits fell on");
    assert!(today < row(&h, &third), "under the day they fell on:\n{}", h.screen());
    // And the file holds the same three, newest first, one line each.
    let lines = kept(&scratch);
    assert_eq!(lines.len(), 3, "one line per visit: {lines:?}");
    assert!(lines[0].starts_with(&third), "{lines:?}");
    assert!(lines[1].starts_with(&second), "{lines:?}");
    assert!(lines[2].starts_with(&links), "{lines:?}");
    let at = |line: &str| line.split('\t').nth(2).and_then(|at| at.parse::<u64>().ok());
    assert!(at(&lines[0]).is_some_and(|at| at > 0), "each with the time it really happened: {lines:?}");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_same_address_twice_is_one_visit_at_the_newest_time() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let (mut h, links, second, third) = walked(&scratch);
    // Back over the same steps and forward over them again: the pages are visited twice.
    h.press("alt+left");
    until(&mut h, "the second page", |h| h.app().address() == second);
    h.press("alt+left");
    until(&mut h, "the first page", |h| h.app().address() == links);
    h.press("alt+right");
    until(&mut h, "the second page again", |h| h.app().address() == second);
    h.press("alt+right");
    until(&mut h, "the third page again", |h| h.app().address() == third);
    away(&mut h);
    h.press("ctrl+h");
    until(&mut h, "the history screen", |h| find_in_row(h, "History", BODY).is_some());
    let lines = kept(&scratch);
    assert_eq!(lines.len(), 3, "a list of pages, not a log of every click: {lines:?}");
    for url in [&links, &second, &third] {
        assert_eq!(lines.iter().filter(|line| line.starts_with(url)).count(), 1, "{url} is one visit");
    }
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_visit_survives_a_restart_and_a_temporary_profile_does_not() {
    // Two Chromiums run at once at the end; taken one by one, the second slot could wait for the
    // first forever.
    let _slots = Slot::take_many(2);
    let scratch = Scratch::new();
    let (mut h, links, _second, _third) = walked(&scratch);
    away(&mut h);
    h.press("ctrl+h");
    until(&mut h, "the history screen", |h| find_in_row(h, "History", BODY).is_some());
    assert!(body_row(&h, &links).is_some(), "the walk is in the file:\n{}", h.screen());
    drop(h);

    // A second screen on the same profile reads the same file back.
    let mut again = open_on(scratch.machine(), Some(&page("/second")));
    until(&mut again, "the page drawn", page_drawn);
    away(&mut again);
    again.press("ctrl+h");
    until(&mut again, "the history screen", |h| find_in_row(h, "History", BODY).is_some());
    assert!(body_row(&again, &links).is_some(), "the visit is there tomorrow too:\n{}", again.screen());

    // A second qbrowser over the same profile runs on a temporary one, and its history is its own.
    let mut temporary = open(&scratch, &page("/third"));
    until(&mut temporary, "the temporary profile", |h| find_in_row(h, "temporary profile", BAR).is_some());
    let its_own = temporary.app().history_file.clone().expect("a file beside the profile in use");
    assert!(its_own.starts_with(scratch.path("temp")), "beside the temporary profile: {}", its_own.display());
    away(&mut temporary);
    temporary.press("ctrl+h");
    until(&mut temporary, "its own history screen", |h| find_in_row(h, "History", BODY).is_some());
    assert!(body_row(&temporary, &links).is_none(), "the first window's record is not in it:\n{}", temporary.screen());
    assert!(its_own.exists(), "and it wrote a file of its own: {}", its_own.display());
    drop(temporary);
    assert!(!its_own.exists(), "which a temporary profile takes with it when it closes");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_visit_can_be_taken_out() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let (mut h, links, second, third) = walked(&scratch);
    away(&mut h);
    h.press("ctrl+h");
    until(&mut h, "the history screen", |h| find_in_row(h, "History", BODY).is_some());
    h.press("down");
    until(&mut h, "a visit focused", |h| body_row(h, &third).is_some());
    h.press("delete");
    until(&mut h, "the question", |h| h.screen().contains("Take this page out of the history?"));
    away(&mut h);
    h.click_text("Delete");
    until(&mut h, "the visit gone from the screen", |h| body_row(h, &third).is_none());
    for other in [&second, &links] {
        assert!(body_row(&h, other).is_some(), "the others are untouched:\n{}", h.screen());
    }
    let lines = kept(&scratch);
    assert_eq!(lines.len(), 2, "and gone from the file: {lines:?}");
    assert!(lines.iter().all(|line| !line.starts_with(&third)), "{lines:?}");

    // A second Delete on the same row: there is nothing there to take out.
    h.press("delete");
    h.advance(Duration::from_millis(200));
    assert!(!h.screen().contains("Take this page out of the history?"), "nothing left to ask about:\n{}", h.screen());
    assert_eq!(kept(&scratch).len(), 2, "and the file is as it was");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_history_file_that_is_broken_costs_the_history_not_the_screen() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let second = page("/second");
    let third = page("/third");
    // A line with no time, a line with no address, and a byte that is not text at all: none of them
    // is a visit, and none of them may cost the screen.
    keep(
        &scratch,
        &format!("{second}\tSecond\t1700000000\n{third}\tThird\tnot a time\n\tNameless\t1\nnoise\u{ff}\u{fe}\n"),
    );
    let mut h = open(&scratch, &page("/links"));
    until(&mut h, "the page drawn", page_drawn);
    away(&mut h);
    h.press("ctrl+h");
    until(&mut h, "the history screen", |h| find_in_row(h, "History", BODY).is_some());
    assert!(body_row(&h, &second).is_some(), "the good line is still there:\n{}", h.screen());
    for broken in ["Nameless", "not a time", "noise"] {
        assert!(body_row(&h, broken).is_none(), "{broken} is skipped, not shown:\n{}", h.screen());
    }
    // Nothing visited at all: an empty tab has no page of its own to be a visit, and then the
    // screen says so rather than showing a list with no explanation.
    let empty = Scratch::new();
    keep(&empty, "");
    let mut h = open_on(empty.machine(), None);
    until(&mut h, "the tab opened", |h| h.app().active_tab().is_some());
    away(&mut h);
    h.press("ctrl+h");
    until(&mut h, "the history screen", |h| find_in_row(h, "History", BODY).is_some());
    assert!(body_row(&h, "Nothing has been visited yet").is_some(), "and it says so:\n{}", h.screen());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_history_file_that_cannot_be_written_is_said_in_a_corner() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    // A folder where the file should be: reading finds no visits, writing fails.
    std::fs::create_dir_all(file(&scratch)).unwrap();
    let links = page("/links");
    let mut h = open(&scratch, &links);
    until(&mut h, "the tab named", |h| find_in_row(h, "Links", 0).is_some());
    away(&mut h);
    until(&mut h, "the warning in the corner", |h| h.screen().contains("The history could not be saved"));
    h.press("ctrl+h");
    until(&mut h, "the history screen", |h| find_in_row(h, "History", BODY).is_some());
    assert!(body_row(&h, &links).is_some(), "the visit is still on the screen for this run:\n{}", h.screen());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn narrow_screens_and_ascii_keep_the_history_usable() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    // A title far too long for one row of a narrow window, which is where a cut in the wrong place
    // would show.
    let long = serve_html(
        "<title>A page with a title far too long to fit on one row of a narrow terminal window</title><p>long</p>",
    );
    let mut h = open(&scratch, &long);
    until_page(&mut h, "document.title.startsWith('A page with a title')");
    h.set_glyph_mode(GlyphMode::Ascii);
    h.resize(40, 20);
    until(&mut h, "the narrow window", |h| h.buffer().area.width == 40);
    h.press("ctrl+h");
    until(&mut h, "the history screen", |h| find_in_row(h, "History", BODY).is_some());
    h.press("down");
    away(&mut h);
    let screen = h.screen();
    for (row, line) in screen.lines().enumerate() {
        assert!(line.chars().count() <= 40, "row {row} is no wider than the screen:\n{screen}");
        let drawn = line.chars().find(|c| !(c.is_ascii_graphic() || *c == ' '));
        assert!(drawn.is_none(), "row {row} draws {drawn:?}, which is not printable ASCII:\n{screen}");
    }
    assert!(screen.contains('~'), "a long title is cut with the framework's own mark:\n{screen}");
    // The accent bar stands at the left edge of the focused row, which in ASCII is a coloured cell
    // rather than a drawn glyph, so it is the row's own surface at column 0 that shows.
    let heading = u16::try_from(body_row(&h, "Today").expect("the day's heading")).unwrap();
    let raised: Vec<usize> = (BODY..screen.lines().count())
        .filter(|row| h.bg(0, u16::try_from(*row).unwrap()) != h.bg(0, heading))
        .collect();
    assert_eq!(raised.len(), 1, "one row is raised at its left edge:\n{screen}");
    let focused = *raised.first().expect("the raised row");
    let marked = screen.lines().nth(focused).expect("the raised row");
    assert!(marked.contains('~'), "on the long visit's own row:\n{screen}");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_lists_and_the_screen_never_cover_the_address_bar() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let (mut h, links, second, _third) = walked(&scratch);
    away(&mut h);
    let before = toolbar(&h);
    let address_at = find_in_row(&h, &links, BAR).map(|(x, _)| x);
    for arrow in ["chevron-left", "chevron-right"] {
        open_steps(&mut h, arrow);
        away(&mut h);
        // Every row a list draws is below the toolbar, so the address bar is never under it.
        let listed: Vec<usize> = (0..h.screen().lines().count())
            .filter(|row| h.screen().lines().nth(*row).is_some_and(|l| l.contains(&second)))
            .collect();
        assert!(listed.iter().all(|row| *row >= BODY), "the list is under the toolbar, not over it");
        assert_eq!(toolbar(&h), before, "and the row did not move");
        assert_eq!(find_in_row(&h, &links, BAR).map(|(x, _)| x), address_at, "the address is where it was");
        h.press("esc");
        away(&mut h);
    }
    assert_eq!(toolbar(&h), before, "the row is still the row: the constant still describes it");
    h.press("ctrl+h");
    until(&mut h, "the history screen", |h| find_in_row(h, "History", BODY).is_some());
    assert_eq!(toolbar(&h), before, "the screen is under the toolbar too");
    assert_eq!(find_in_row(&h, &links, BAR).map(|(x, _)| x), address_at, "with the address untouched");
}

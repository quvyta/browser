//! What the page reports about its selection, and the question the engine asks it without waiting.

use serde_json::json;

use super::fixture::{Browser, PATIENCE, Scratch, Slot};

/// A page with a paragraph wide and tall enough to hit with the mouse.
const WORDS: &str = "the quick brown fox jumps";

/// A small web server of its own that answers `body` to everything it is asked, so this file
/// needs nothing from the shared fixture's one shared page list.
fn serve_html(body: &str) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let body = body.to_owned();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            use std::io::{BufRead, Write};
            let mut reader = std::io::BufReader::new(&stream);
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let mut out = &stream;
            let _ = out.write_all(head.as_bytes());
        }
    });
    format!("http://127.0.0.1:{port}/")
}

/// The page the selection tests read: one paragraph, big enough to select with the mouse.
fn words_page() -> String {
    serve_html(&format!(
        "<!doctype html><html><body style='margin:0'><p id=words style='font-size:40px;width:700px'>{WORDS}</p></body></html>"
    ))
}

/// Selects the whole of `#words` the way a page's own script would, which fires the very event a
/// drag fires.
fn select_words(browser: &Browser, tab: &crate::engine::TabId) {
    browser.eval(tab, "getSelection().selectAllChildren(document.querySelector('#words'))");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_engine_reports_the_selection_and_forgets_it_when_the_tab_goes() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let browser = Browser::start(&scratch.options());
    let tab = browser.open(&words_page());

    // A page that has selected nothing has no selection to report, and none is invented.
    select_words(&browser, &tab);
    let text = browser.wait("the selection", |event| match event {
        crate::engine::Event::Selection { tab: at, text } if *at == tab => Some(text.clone()),
        _ => None,
    });
    assert!(text.contains("quick brown fox"), "the page's own words, not a constant: {text:?}");

    // Letting the selection go is reported as the empty string, so a stale one cannot linger.
    browser.eval(&tab, "getSelection().removeAllRanges()");
    let emptied = browser.wait("the empty selection", |event| match event {
        crate::engine::Event::Selection { tab: at, text } if *at == tab => Some(text.clone()),
        _ => None,
    });
    assert_eq!(emptied, "", "an emptied selection is reported, not kept");

    // The tab is gone, and nothing about it is left to answer for.
    select_words(&browser, &tab);
    browser.wait("the selection again", |event| match event {
        crate::engine::Event::Selection { tab: at, text } if *at == tab && !text.is_empty() => Some(()),
        _ => None,
    });
    browser.engine.close_tab(&tab);
    browser.wait("the tab closed", |event| match event {
        crate::engine::Event::TabClosed { tab: at } if *at == tab => Some(()),
        _ => None,
    });
    browser.engine.ask(&tab, "1 + 1");
    let answer = browser.wait("the answer for a tab that is gone", |event| match event {
        crate::engine::Event::Answered { tab: at, value } if *at == tab => Some(value.clone()),
        _ => None,
    });
    assert!(answer.is_err(), "a tab that is gone has nothing to answer with: {answer:?}");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn an_expression_that_throws_answers_with_its_reason() {
    const BROKEN: &str = "qbrowThereIsNoSuchThing()";
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let browser = Browser::start(&scratch.options());
    let tab = browser.open(&words_page());

    browser.engine.ask(&tab, BROKEN);
    let value = browser.wait("the answer", |event| match event {
        crate::engine::Event::Answered { tab: at, value } if *at == tab => Some(value.clone()),
        _ => None,
    });
    let reason = value.as_ref().err().unwrap_or_else(|| panic!("a broken expression answers why: {value:?}"));
    assert!(!reason.is_empty() && reason != "the expression threw", "Chromium's own words: {reason:?}");
    // The waiting call reads the same answer, which is why there is only one reader.
    assert_eq!(browser.engine.evaluate(&tab, BROKEN, PATIENCE), value);
    assert_eq!(browser.eval(&tab, "1 + 1"), json!(2), "a good expression still answers with its value");
}

//! Pages in tabs: frames, the mouse, the keys, the wheel, history, popups, titles and crashes.

use serde_json::json;

use super::fixture::{Browser, Scratch, Slot, jpeg_size, page, processes_mentioning};
use crate::engine::{Event, KeyPress, Modifiers, Mouse, procfs};

#[test]
fn a_shown_tab_sends_frames_the_size_of_its_viewport() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let browser = Browser::start(&scratch.options());
    let tab = browser.open(&page("/long"));
    browser.engine.set_viewport(&tab, 640, 400);
    let size = browser.wait("a 640 × 400 frame", |event| match event {
        Event::Frame { tab: from, jpeg } if *from == tab => jpeg_size(jpeg).filter(|size| *size == (640, 400)),
        _ => None,
    });
    assert_eq!(size, (640, 400));
    assert_eq!(browser.eval(&tab, "[innerWidth, innerHeight]"), json!([640, 400]));
}

#[test]
fn clicking_a_link_where_it_is_drawn_follows_it() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let browser = Browser::start(&scratch.options());
    let tab = browser.open(&page("/links"));
    let link = browser.middle_of(&tab, "#next");
    browser.click(&tab, link);
    let (can_back, can_forward) = browser.arrive(&tab, &page("/second"));
    assert!(can_back && !can_forward);
    assert_eq!(browser.eval(&tab, "document.querySelector('#here').textContent"), "second");
}

#[test]
fn keys_pressed_after_clicking_a_field_land_in_it() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let browser = Browser::start(&scratch.options());
    let tab = browser.open(&page("/form"));
    let field = browser.middle_of(&tab, "#field");
    browser.click(&tab, field);
    browser.until(&tab, "document.activeElement.id === 'field'");
    let shift = Modifiers { shift: true, ..Modifiers::default() };
    let letter = |key: &str, modifiers| KeyPress {
        key: key.into(),
        code: format!("Key{}", key.to_uppercase()),
        key_code: u32::from(key.to_uppercase().as_bytes()[0]),
        text: Some(key.into()),
        modifiers,
    };
    for press in [letter("H", shift), letter("i", Modifiers::default()), letter("x", Modifiers::default())] {
        browser.engine.key(&tab, &press);
    }
    let backspace = KeyPress {
        key: "Backspace".into(),
        code: "Backspace".into(),
        key_code: 8,
        text: None,
        modifiers: Modifiers::default(),
    };
    browser.engine.key(&tab, &backspace);
    browser.engine.insert_text(&tab, " there");
    browser.until(&tab, "document.querySelector('#field').value === 'Hi there'");
}

#[test]
fn the_wheel_scrolls_the_page_down() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let browser = Browser::start(&scratch.options());
    let tab = browser.open(&page("/long"));
    assert_eq!(browser.eval(&tab, "scrollY"), json!(0));
    browser.engine.mouse(&tab, Mouse::Wheel { dx: 0.0, dy: 180.0 }, 200.0, 200.0, Modifiers::default());
    browser.until(&tab, "scrollY > 0");
}

#[test]
fn a_target_blank_link_opens_a_tab_with_its_opener() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let browser = Browser::start(&scratch.options());
    let tab = browser.open(&page("/blank"));
    let link = browser.middle_of(&tab, "#out");
    browser.click(&tab, link);
    let popup = browser.wait("the popup's tab", |event| match event {
        Event::TabOpened { tab: popup, opener: Some(opener), .. } if *opener == tab => Some(popup.clone()),
        _ => None,
    });
    browser.arrive(&popup, &page("/second"));
    assert_eq!(browser.eval(&popup, "document.title"), "Second");
}

#[test]
fn a_window_the_page_opens_is_a_tab_and_closing_itself_closes_it() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let browser = Browser::start(&scratch.options());
    let tab = browser.open(&page("/opener"));
    let button = browser.middle_of(&tab, "#open");
    browser.click(&tab, button);
    let popup = browser.wait("the window's tab", |event| match event {
        Event::TabOpened { tab: popup, opener: Some(opener), .. } if *opener == tab => Some(popup.clone()),
        _ => None,
    });
    browser.wait("the window closing itself", |event| match event {
        Event::TabClosed { tab: closed } if *closed == popup => Some(()),
        _ => None,
    });
}

#[test]
fn back_and_forward_walk_the_history_and_say_where_they_can_go() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let browser = Browser::start(&scratch.options());
    let tab = browser.open(&page("/links"));
    browser.engine.navigate(&tab, &page("/second"));
    assert_eq!(browser.arrive(&tab, &page("/second")), (true, false));
    browser.engine.back(&tab);
    assert_eq!(browser.arrive(&tab, &page("/links")), (false, true));
    browser.engine.forward(&tab);
    assert_eq!(browser.arrive(&tab, &page("/second")), (true, false));
    // At the end of the history, forward has nowhere to go and the tab stays.
    browser.engine.forward(&tab);
    browser.engine.navigate(&tab, &page("/third"));
    assert_eq!(browser.arrive(&tab, &page("/third")), (true, false));
    assert_eq!(browser.eval(&tab, "history.length"), json!(3));
}

#[test]
fn loading_titles_and_closing_are_reported() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let browser = Browser::start(&scratch.options());
    let tab = browser.open(&page("/links"));
    browser.engine.navigate(&tab, &page("/title"));
    // Everything the tab says until the script has changed the title, in order.
    let mut heard = Vec::new();
    browser.wait("the changed title", |event| match event {
        Event::Title { tab: from, title } if *from == tab => {
            heard.push(title.clone());
            (title == "After").then_some(())
        }
        Event::Loading { tab: from, loading } if *from == tab => {
            heard.push(format!("loading {loading}"));
            None
        }
        _ => None,
    });
    assert_eq!(heard, ["loading true", "Before", "loading false", "After"], "no empty title while the page is read");
    assert_eq!(browser.eval(&tab, "typeof qbrowserTitle"), "undefined", "the page cannot see the watcher");
    browser.engine.close_tab(&tab);
    browser.wait("the tab to close", |event| match event {
        Event::TabClosed { tab: closed } if *closed == tab => Some(()),
        _ => None,
    });
    assert!(browser.engine.evaluate(&tab, "1", super::fixture::PATIENCE).is_err(), "a closed tab answers nothing");
}

#[test]
fn a_crashed_tab_is_reported_and_comes_back_when_loaded_again() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let browser = Browser::start(&scratch.options());
    let tab = browser.open(&page("/second"));
    // What an out-of-memory kill does to a tab: its renderer disappears.
    let profile = browser.engine.profile().path.to_string_lossy().into_owned();
    for renderer in processes_mentioning(&profile) {
        let arguments = std::fs::read(format!("/proc/{renderer}/cmdline")).unwrap_or_default();
        if String::from_utf8_lossy(&arguments).contains("--type=renderer") {
            procfs::kill_process(renderer);
        }
    }
    browser.wait("the crash", |event| match event {
        Event::Crashed { tab: from } if *from == tab => Some(()),
        _ => None,
    });
    browser.engine.navigate(&tab, &page("/third"));
    browser.arrive(&tab, &page("/third"));
    assert_eq!(browser.eval(&tab, "document.title"), "Third");
}

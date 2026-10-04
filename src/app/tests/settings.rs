//! The settings screen from where the person's hands reach it: `ctrl+,` and the gear open it,
//! `Esc` and the gear close it, and what is on it is the framework's shared look rows, the
//! Quvyta-wide update notice and qbrowser's own search engine and start page. Each setting is
//! read from what the hands do, and a broken, unknown, unwritable or default-filled
//! `browser.conf` costs the screen nothing.

use std::time::Duration;

use qframe::color::{ColorDepth, Rgb};
use qframe::graphics::Graphics;
use qframe::icons::GlyphMode;
use qframe::runtime::Harness;
use qframe::storage::{Family, Scope, Shared};
use serde_json::json;

use super::{HEIGHT, PAGE_TOP, Scratch, Slot, WIDTH, eval, find_in_row, open, open_on, page, page_drawn, until};
use crate::app::Browser;
use crate::app::settings::SECTION;
use crate::cli::Start;

/// Cells the settings rows stand in from the left of the page area: the page's own padding.
const LEFT: i32 = 2;

/// Cells the framework keeps between a row's label and its control, and the cells a switch's
/// capsule takes; the middle of the capsule is where a click lands.
const GAP: i32 = 2;
const TRACK: i32 = 5;

/// Lets the screen settle where no Chromium runs: the start is answered at once.
fn settle(h: &mut Harness<Browser>) {
    for _ in 0..4 {
        h.advance(Duration::from_millis(100));
    }
}

/// The screen's words in one line: a sentence the screen wraps over two rows, and the pillar of a
/// focused row, do not stand between them.
fn words(h: &Harness<Browser>) -> String {
    h.screen().replace('▌', " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `browser.conf` as it stands.
fn conf(scratch: &Scratch) -> String {
    std::fs::read_to_string(scratch.path("config/browser.conf")).unwrap_or_default()
}

/// `quvyta.conf`, the ecosystem's shared file, as it stands.
fn shared(scratch: &Scratch) -> String {
    std::fs::read_to_string(scratch.path("config/quvyta.conf")).unwrap_or_default()
}

/// The words the framework's own language files give `key` under `quvyta.appearance`.
fn framework(h: &Harness<Browser>, key: &str) -> String {
    h.env().i18n().translate(&format!("quvyta.appearance.{key}"), &[])
}

/// The screen with the settings open, on a machine with no Chromium: the screen is drawn whatever
/// Chromium is doing, so no test here takes a Chromium slot.
fn screen(scratch: &Scratch) -> Harness<Browser> {
    let mut h = open_on(scratch.machine_without_chromium(), None);
    settle(&mut h);
    h.press("ctrl+,");
    h
}

/// The switch of the row `label`: a switch is drawn in colour alone, so its place is worked out
/// from the rows' own geometry — every control of a settings row stands at the right end of them,
/// a switch five cells wide and two cells short of the edge.
fn switch_of(h: &Harness<Browser>, label: &str) -> (i32, i32) {
    let (_, y) = h.find(label).unwrap_or_else(|| panic!("no {label} on the screen:\n{}", h.screen()));
    (LEFT + i32::from(SECTION) - GAP - TRACK / 2, y)
}

/// The picker in the row `label`, on the words it shows now (`chosen`): a picker is drawn with them,
/// so they are where a click opens it.
fn picker_of(h: &Harness<Browser>, label: &str, chosen: &str) -> (i32, i32) {
    let (_, row) = h.find(label).unwrap_or_else(|| panic!("no {label} on the screen:\n{}", h.screen()));
    let (x, _) = find_in_row(h, chosen, row.cast_unsigned() as usize)
        .unwrap_or_else(|| panic!("`{chosen}` is not on the {label} row:\n{}", h.screen()));
    (x, row)
}

/// Chooses `wanted` in the picker of the row `label`, which shows `chosen` now: a click opens the
/// list, a second click takes the option.
fn choose(h: &mut Harness<Browser>, label: &str, chosen: &str, wanted: &str) {
    let (x, y) = picker_of(h, label, chosen);
    h.click(x, y);
    h.click_text(wanted);
    settle(h);
}

/// The background of the page area's right margin, which no row of the settings reaches: it is
/// the theme's own ground, so it changes with the theme and with nothing else.
fn margin(h: &Harness<Browser>) -> Vec<Option<Rgb>> {
    let area = h.buffer().area;
    let top = u16::try_from(PAGE_TOP).unwrap_or_default();
    let from = SECTION.saturating_add(u16::try_from(LEFT).unwrap_or_default());
    (top..area.height).flat_map(|y| (from..area.width).map(move |x| (x, y))).map(|(x, y)| h.bg(x, y)).collect()
}

/// Whether two runs of [`margin`] hold the same colours, and where they first do not.
fn same(left: &[Option<Rgb>], right: &[Option<Rgb>]) -> Result<(), String> {
    match left.iter().zip(right).position(|(l, r)| l != r) {
        Some(at) => Err(format!("cell {at}: {l:?} against {r:?}", l = left[at], r = right[at])),
        None => Ok(()),
    }
}

/// The screen as a member of the ecosystem, through the runtime a real run starts, so it follows
/// `quvyta.conf` and carries qbrow's own languages and keys.
fn member(scratch: &Scratch) -> Harness<Browser> {
    let machine = scratch.machine_without_chromium();
    let folder = machine.config.clone().expect("the scratch machine has a Quvyta folder");
    // The very runtime `qbrow` runs, so its languages, keys and membership are what is tested.
    let mut h =
        crate::runtime(machine, &Start::default()).harness_in(folder, WIDTH, HEIGHT).expect("qbrow's own assets load");
    h.set_glyph_mode(GlyphMode::Unicode)
        .set_depth(ColorDepth::TrueColor)
        .set_graphics(Graphics::HalfBlock)
        .set_reduced_motion(true);
    h
}

#[test]
fn the_shared_look_rows_are_the_frames() {
    let scratch = Scratch::new();
    let mut h = screen(&scratch);
    let keys = ["heading", "language", "theme", "icons", "reduce-motion", "pillar", "updates"];
    let english = keys.map(|key| framework(&h, key));
    let drawn = h.screen();
    for words in &english {
        assert!(drawn.contains(words), "`{words}` is on the screen:\n{drawn}");
    }
    for words in ["Search engine", "Start page", "Settings"] {
        assert!(drawn.contains(words), "qbrowser's own `{words}` is on the screen:\n{drawn}");
    }
    // The rows speak the language in force, which a second copy of them, written into qbrowser's
    // own language files, could not: these are the framework's own keys, read as they stand.
    h.set_locale("tr");
    let turkish = h.screen();
    for (key, was) in keys.into_iter().zip(&english) {
        let now = framework(&h, key);
        assert_ne!(&now, was, "`{key}` is not the same word in Turkish");
        assert!(turkish.contains(&now), "`{now}` is on the Turkish screen:\n{turkish}");
    }
}

#[test]
fn changing_the_theme_through_these_rows_changes_the_screen() {
    let scratch = Scratch::new();
    let mut h = screen(&scratch);
    let themes = h.env().themes();
    let current = h.env().theme().id().to_owned();
    let (_, was) = themes.iter().find(|(id, _)| *id == current).expect("the theme in force is listed");
    let (id, name) = themes.iter().find(|(id, _)| *id != current).expect("more than one theme").clone();
    let before = margin(&h);
    choose(&mut h, "Theme", was, &name);
    assert_eq!(h.env().theme().id(), id, "{name} is in force");
    let after = margin(&h);
    assert_ne!(before, after, "the ground of the settings screen changed with the theme");
    // And it is the ground of that theme, read off a screen drawn in it from the start.
    let mut reference = screen(&scratch);
    reference.set_theme(&id);
    let expected = margin(&reference);
    assert_eq!(same(&after, &expected), Ok(()), "the screen is drawn in {id}");
    assert!(h.screen().contains(&name), "and the row says which:\n{}", h.screen());
}

#[test]
fn turning_the_update_notice_off_stops_the_question() {
    let scratch = Scratch::new();
    let mut h = open_on(scratch.machine_without_chromium(), None);
    assert_eq!(h.update_checks().len(), 1, "the notice is on by default, so it asks");
    h.press("ctrl+,");
    let (x, y) = switch_of(&h, &framework(&h, "updates"));
    h.click(x, y);
    settle(&mut h);
    assert!(shared(&scratch).contains("update-notice = false"), "{}", shared(&scratch));
    drop(h);
    assert!(
        open_on(scratch.machine_without_chromium(), None).update_checks().is_empty(),
        "the next start asks nothing"
    );
    // With the switch on again the question is asked, so the empty answer above is the switch.
    let mut h = open_on(scratch.machine_without_chromium(), None);
    h.press("ctrl+,");
    let (x, y) = switch_of(&h, &framework(&h, "updates"));
    h.click(x, y);
    settle(&mut h);
    drop(h);
    assert_eq!(
        open_on(scratch.machine_without_chromium(), None).update_checks().len(),
        1,
        "on again, the question is asked again"
    );
}

#[test]
fn the_update_notice_switch_is_the_ecosystems_own() {
    let scratch = Scratch::new();
    let mut h = member(&scratch);
    h.press("ctrl+,");
    let (x, y) = switch_of(&h, &framework(&h, "updates"));
    h.click(x, y);
    settle(&mut h);
    assert!(shared(&scratch).contains("update-notice = false"), "the shared file holds it: {}", shared(&scratch));
    assert!(!conf(&scratch).contains("update-notice"), "not qbrowser's own file: {:?}", conf(&scratch));
    // Another Quvyta application, started on the same folder, follows the same shared file.
    assert!(member(&scratch).update_checks().is_empty(), "a second application asks nothing either");
}

#[test]
fn another_application_changing_the_shared_look_is_seen_while_the_screen_is_open() {
    let scratch = Scratch::new();
    let folder = scratch.path("config");
    std::fs::write(folder.join("quvyta.conf"), "language = \"en\"\ntheme = \"nordic\"\nicons = \"unicode\"\n").unwrap();
    let mut h = member(&scratch);
    h.press("ctrl+,");
    assert_eq!(h.env().theme().id(), "nordic", "the screen starts in the shared theme");
    let languages = h.env().i18n().list();
    let active = h.env().i18n().active().to_owned();
    let (_, language) = languages.iter().find(|(code, _)| *code == active).expect("the language in force is listed");
    let (_, other) = languages.iter().find(|(code, _)| *code != active).expect("nine languages").clone();
    // The language list is open while the other application switches the theme for everyone.
    let (x, y) = picker_of(&h, &framework(&h, "language"), language);
    h.click(x, y);
    assert!(h.screen().contains(&other), "the list is open:\n{}", h.screen());
    Family::QUVYTA.set_in(&folder, "desk", Shared::Theme, "iris", Scope::Ecosystem).expect("saved");
    h.poll_preferences();
    settle(&mut h);
    assert_eq!(h.env().theme().id(), "iris", "the screen follows at once");
    assert!(h.screen().contains(&other), "an open list stays open:\n{}", h.screen());
    let heading = h.env().i18n().translate("browser.settings.heading", &[]);
    assert!(h.screen().contains(&heading), "the settings screen is still the one drawn:\n{}", h.screen());
    // The open list covers the theme row's value; closed, the row shows the new theme.
    h.press("esc");
    settle(&mut h);
    assert!(h.screen().contains("Iris"), "and the theme row says so:\n{}", h.screen());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_search_engine_is_the_one_the_address_bar_uses() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let words = "three words typed";
    let mut h = open(&scratch, &page("/links"));
    h.press("ctrl+,");
    choose(&mut h, "Search engine", "DuckDuckGo", "Brave");
    h.press("esc");
    // The search goes nowhere: the tests' Chromium sends every request for a real site to a proxy
    // nobody listens on. Where the address bar sent it is what the tab reports.
    h.press("ctrl+l").type_text(words).press("enter");
    let brave = "https://search.brave.com/search?q=three%20words%20typed";
    until(&mut h, "the search address in the address bar", |h| h.app().address() == brave);
    assert!(conf(&scratch).contains("search-engine = \"brave\""), "{:?}", conf(&scratch));
    drop(h);
    // The same three words with the default left alone go to the default's own address.
    let fresh = Scratch::new();
    let mut other = open(&fresh, &page("/links"));
    other.press("ctrl+l").type_text(words).press("enter");
    let duck = "https://duckduckgo.com/?q=three%20words%20typed";
    until(&mut other, "the default search address", |h| h.app().address() == duck);
    assert!(!conf(&fresh).contains("search-engine"), "the default is never written: {:?}", conf(&fresh));
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_start_page_that_cannot_be_used_says_so_in_its_row() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    // Chromium runs, so the new tab below is a tab: where it is missing, only the settings work.
    let mut h = open(&scratch, &page("/links"));
    h.press("ctrl+,");
    let (x, y) = h.find("Start page").unwrap_or_else(|| panic!("the start page row:\n{}", h.screen()));
    // Only spaces: the address bar asks nothing for that, so there is nothing to open.
    h.click(x, y).type_text("   ");
    assert!(
        words(&h).contains("Empty: a new tab opens a blank page with the keyboard in the address bar."),
        "the row says what it does instead:\n{}",
        h.screen()
    );
    assert!(!conf(&scratch).contains("start-page"), "and nothing is written: {:?}", conf(&scratch));
    h.press("esc").press("ctrl+t");
    assert_eq!(h.app().tab_count(), 2);
    assert_eq!(h.app().tab_url(), "about:blank", "the empty tab is a blank page");
    assert!(h.is_focused(crate::app::view::LOCATION), "with the keyboard in the address bar");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_start_page_is_used_by_a_new_empty_tab() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let second = page("/second");
    let mut h = open(&scratch, &page("/links"));
    h.press("ctrl+,");
    let (x, y) = h.find("Start page").unwrap_or_else(|| panic!("the start page row:\n{}", h.screen()));
    h.click(x, y).type_text(&second).press("enter");
    assert!(h.screen().contains(&format!("A new tab opens {second}")), "{}", h.screen());
    h.press("esc").press("ctrl+t");
    until(&mut h, "the new tab on the start page", |h| h.app().tab_url() == second && h.app().active_tab().is_some());
    until(&mut h, "the page that was set", |h| eval(h, "location.pathname") == json!("/second"));
}

#[test]
fn a_broken_browser_conf_does_not_crash_the_screen() {
    let scratch = Scratch::new();
    let file = scratch.path("config/browser.conf");
    // A key qbrowser no longer knows, a value of the wrong shape and a line cut in half.
    std::fs::write(&file, "gone = \"from an older qbrowser\"\nsearch-engine = 42\nstart-page = [1, 2]\nhalf = \"cut")
        .unwrap();
    let h = screen(&scratch);
    let drawn = h.screen();
    assert!(drawn.contains("DuckDuckGo"), "the search engine is the default:\n{drawn}");
    assert!(
        words(&h).contains("Empty: a new tab opens a blank page with the keyboard in the address bar."),
        "and the start page is the default:\n{drawn}"
    );
    let healed = std::fs::read_to_string(&file).unwrap_or_default();
    for key in ["gone", "search-engine", "start-page", "half"] {
        assert!(!healed.contains(key), "`{key}` does not come back: {healed:?}");
    }
    assert!(scratch.path("config/browser.conf.bak").exists(), "the file as it was is kept beside it");
    // A file that cannot be written: the change is applied anyway and the row says why.
    let scratch = Scratch::new();
    std::fs::create_dir_all(scratch.path("config/browser.conf")).unwrap();
    let mut h = screen(&scratch);
    choose(&mut h, "Search engine", "DuckDuckGo", "Brave");
    let drawn = h.screen();
    let said = framework(&h, "not-saved").split("{reason}").next().unwrap_or_default().to_owned();
    assert!(drawn.contains(&said), "the row says, in the framework's own words, `{said}`:\n{drawn}");
    assert!(drawn.contains("Brave"), "and the change is applied for this run:\n{drawn}");
}

#[test]
fn a_default_is_not_written_to_the_file() {
    let scratch = Scratch::new();
    let mut h = screen(&scratch);
    h.press("esc");
    let own = conf(&scratch);
    assert!(!own.contains("search-engine") && !own.contains("start-page"), "nothing of qbrowser's: {own:?}");

    let mut h = screen(&scratch);
    choose(&mut h, "Search engine", "DuckDuckGo", "Brave");
    assert!(conf(&scratch).contains("search-engine = \"brave\""), "a real change is written: {:?}", conf(&scratch));
    choose(&mut h, "Search engine", "Brave", "DuckDuckGo");
    assert!(!conf(&scratch).contains("search-engine"), "the default is taken out again: {:?}", conf(&scratch));

    let (x, y) = h.find("Start page").unwrap_or_else(|| panic!("the start page row:\n{}", h.screen()));
    h.click(x, y).type_text("quvyta.com/tr/");
    settle(&mut h);
    assert!(
        conf(&scratch).contains("start-page = \"quvyta.com/tr/\""),
        "a real change is written: {:?}",
        conf(&scratch)
    );
    h.press("ctrl+a").press("backspace");
    settle(&mut h);
    assert!(!conf(&scratch).contains("start-page"), "the default is taken out again: {:?}", conf(&scratch));
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn the_tabs_and_the_toolbar_stay_while_the_settings_screen_is_open() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/links"));
    let gear = h.env().icons().glyph("settings").into_owned();
    assert!(find_in_row(&h, &gear, 1).is_some(), "the gear is at the end of the toolbar:\n{}", h.screen());
    h.press("ctrl+,");
    let opened = h.screen();
    assert!(opened.contains("Settings"), "the settings screen is open:\n{opened}");
    assert!(find_in_row(&h, "Links", 0).is_some(), "the tab strip stays:\n{opened}");
    assert!(find_in_row(&h, &gear, 1).is_some(), "and the toolbar with the gear on it:\n{opened}");
    // qbrowser's own keys go on working from the settings screen.
    h.press("ctrl+t");
    assert_eq!(h.app().tab_count(), 2, "a new tab opens from there");
    assert!(h.screen().contains("Settings"), "and the settings screen is still open:\n{}", h.screen());
    h.press("esc");
    assert!(!h.screen().contains("Settings"), "Esc takes it back:\n{}", h.screen());
    assert!(find_in_row(&h, &gear, 1).is_some(), "the gear is still there to open it again");
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn esc_and_the_gear_and_the_key_all_leave_the_screen_the_same_way() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/links"));
    let gear = h.env().icons().glyph("settings").into_owned();
    let gear_cell = find_in_row(&h, &gear, 1).expect("the gear on the toolbar");
    // The key in, the key out.
    h.press("ctrl+,");
    assert!(h.screen().contains("Settings"), "{}", h.screen());
    h.press("ctrl+,");
    assert!(!h.screen().contains("Settings"), "the key closes it too:\n{}", h.screen());
    assert!(page_drawn(&h), "the page's picture is still there");
    // The gear in, the gear out.
    h.click(gear_cell.0, gear_cell.1);
    assert!(h.screen().contains("Settings"), "{}", h.screen());
    h.click(gear_cell.0, gear_cell.1);
    assert!(!h.screen().contains("Settings"), "the gear closes it too:\n{}", h.screen());
    assert!(page_drawn(&h), "and the page comes back as it was");
    // The gear in, Esc out.
    h.click(gear_cell.0, gear_cell.1);
    h.press("esc");
    assert!(!h.screen().contains("Settings"), "Esc closes what the gear opened:\n{}", h.screen());
    assert!(page_drawn(&h), "the picture never went away, so nothing is drawn again");
    assert!(find_in_row(&h, "Links", 0).is_some(), "the tabs are where they were:\n{}", h.screen());
    assert_eq!(h.app().tab_count(), 1);
}

#[test]
fn the_screen_is_readable_at_40_columns_and_in_ascii() {
    let scratch = Scratch::new();
    let mut h = screen(&scratch);
    // Tall enough for every row, so what is checked is the width alone.
    const TALL: u16 = 70;
    h.resize(40, TALL);
    readable(&h, 40);
    for label in ["Search engine", "Start page", "Appearance"] {
        assert!(words(&h).contains(label), "`{label}` is whole on a narrow screen:\n{}", h.screen());
    }
    let updates = framework(&h, "updates");
    assert!(words(&h).contains(&updates), "`{updates}` is whole too, over two rows at most:\n{}", h.screen());
    // Thirty columns is the narrowest the shell is drawn in at all; there the
    // framework's own answer is the right one — a label takes its own lines and its control the
    // line under it, and nothing is cut — so the tabs and the toolbar stay and the rows go on
    // being readable.
    h.resize(30, TALL);
    readable(&h, 30);
    assert!(
        h.screen().contains("Search engine") && h.screen().contains("Start page"),
        "the rows are still there:\n{}",
        h.screen()
    );
    // And the same screen in ASCII: every cell a character a terminal without the block glyphs
    // can still show.
    h.resize(40, TALL).set_glyph_mode(GlyphMode::Ascii);
    readable(&h, 40);
    let buffer = h.buffer();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let symbol = buffer[(x, y)].symbol();
            assert!(
                symbol.chars().all(|c| c.is_ascii_graphic() || c == ' ' || c == '\n'),
                "`{symbol}` at {x},{y} is not ASCII printable:\n{}",
                h.screen()
            );
        }
    }
}

/// Nothing on the screen of `h` is wider than `width`, and no line of the settings is cut in the
/// middle of a word. The tab strip and the toolbar above them shorten a long title or address with
/// `…`, as they do on every screen.
fn readable(h: &Harness<Browser>, width: u16) {
    let drawn = h.screen();
    for line in drawn.lines() {
        assert!(qframe::text::width(line) <= width, "no row is wider than {width}: {line:?}");
    }
    // The start page's field shows as much of its placeholder as it has room for, as a field does.
    let placeholder = h.env().i18n().translate("browser.settings.start-page-placeholder", &[]);
    let field = placeholder.chars().take(6).collect::<String>();
    let settings: Vec<&str> =
        drawn.lines().skip(usize::from(crate::app::view::CHROME_ROWS)).filter(|line| !line.contains(&field)).collect();
    assert!(!settings.iter().any(|line| line.contains('…')), "no label is cut in the middle of a word:\n{drawn}");
}

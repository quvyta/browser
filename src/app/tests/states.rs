//! What stands in the page's place, and the edges of qbrowser's life: Chromium missing and
//! installed only on a click, Chromium gone, a crashed tab, a second qbrowser on the same
//! profile, a terminal too small or without pictures, the update notice and quitting.

use std::ffi::OsString;
use std::time::Duration;

use qframe::graphics::Graphics;
use qframe::prelude::*;
use qframe::runtime::Termination;

use super::{Scratch, Slot, find_in_row, open, open_on, page, page_drawn, until, until_page};
use crate::app::{Browser, Msg};
use crate::engine::Event;
use crate::engine::tests::fixture::{processes_mentioning, wait_until_none_mention};

/// Lets the screen settle where no Chromium runs: the start is answered at once.
fn settle(h: &mut Harness<Browser>) {
    for _ in 0..4 {
        h.advance(Duration::from_millis(100));
    }
}

#[test]
fn without_chromium_the_screen_says_so_and_installs_only_when_asked() {
    let scratch = Scratch::new();
    scratch.program("pacman");
    let mut h = open_on(scratch.machine_without_chromium(), None);
    settle(&mut h);
    assert!(h.screen().contains("Chromium was not found"), "{}", h.screen());
    assert!(h.handoffs().is_empty(), "nothing is installed without a click");
    h.click_text("Install here");
    settle(&mut h);
    let asked = h.handoffs();
    assert_eq!(asked.len(), 1, "one install");
    let words: Vec<OsString> = ["-S", "chromium"].map(OsString::from).to_vec();
    assert_eq!((asked[0].program.as_os_str(), &asked[0].args), (OsString::from("pacman").as_os_str(), &words));
    assert!(h.screen().contains("Chromium is still not"), "looked for again afterwards:\n{}", h.screen());
}

#[test]
fn the_install_goes_through_the_administrators_gate_and_its_command_can_be_shown() {
    let scratch = Scratch::new();
    scratch.program("sudo");
    scratch.program("apt");
    let mut h = open_on(scratch.machine_without_chromium(), None);
    settle(&mut h);
    assert!(!h.screen().contains("sudo apt install chromium"));
    h.click_text("Show the command");
    settle(&mut h);
    assert!(h.screen().contains("sudo apt install chromium"), "{}", h.screen());
    assert!(h.handoffs().is_empty(), "showing is not installing");
}

#[test]
fn on_a_system_qbrowser_cannot_install_on_it_explains_the_way_by_hand() {
    let scratch = Scratch::new();
    let mut h = open_on(scratch.machine_without_chromium(), None);
    settle(&mut h);
    let screen = h.screen();
    assert!(screen.contains("Chromium was not found"), "{screen}");
    assert!(screen.contains("QBROW_CHROMIUM"), "{screen}");
    assert!(!screen.contains("Install here"), "{screen}");
}

#[test]
fn chromium_found_after_the_install_is_started() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    scratch.program("pacman");
    // The machine's Chromium is found on its search path only once "installed" there.
    let machine = crate::app::Machine { chromium: None, ..scratch.machine() };
    let mut h = open_on(machine, Some(&page("/links")));
    settle(&mut h);
    assert!(h.screen().contains("Chromium was not found"), "{}", h.screen());
    std::os::unix::fs::symlink(super::CHROMIUM, scratch.path("bin/chromium")).unwrap();
    h.click_text("Install here");
    until(&mut h, "the page after the install", page_drawn);
}

#[test]
fn chromium_ending_on_its_own_is_said_and_restarting_brings_the_tabs_back() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let links = page("/links");
    let mut h = open(&scratch, &links);
    let group = h.app().engine().map(crate::engine::Engine::chromium_pid).expect("Chromium runs");
    kill(&format!("-{group}"));
    until(&mut h, "the stopped screen", |h| h.screen().contains("Chromium stopped"));
    h.click_text("Restart");
    until(&mut h, "the page again", |h| h.app().address() == links && page_drawn(h));
}

#[test]
fn a_crashed_tab_says_so_and_reload_brings_it_back() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let links = page("/links");
    let mut h = open(&scratch, &links);
    let profile = h.app().engine().expect("Chromium runs").profile().path.to_string_lossy().into_owned();
    // What an out-of-memory kill does to a tab: its renderer disappears.
    for pid in processes_mentioning(&profile) {
        let arguments = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        if String::from_utf8_lossy(&arguments).contains("--type=renderer") {
            kill(&pid.to_string());
        }
    }
    until(&mut h, "the crashed tab", |h| h.screen().contains("This tab crashed"));
    // The button, not the word at the start of the explanation above it.
    let row = h.screen().lines().position(|line| line.trim() == "Reload").expect("the reload button");
    let (x, y) = find_in_row(&h, "Reload", row).unwrap();
    h.click(x, y);
    assert!(!h.screen().contains("This tab crashed"), "{}", h.screen());
    // The last picture stays until the page is back, so the page itself is asked.
    until_page(&mut h, "document.title === 'Links'");
}

/// Sends SIGKILL to `target`: a process id, or a process group as `-<id>`.
fn kill(target: &str) {
    let status = std::process::Command::new("kill").args(["-KILL", "--", target]).status().expect("kill runs");
    assert!(status.success(), "kill {target}");
}

#[test]
fn a_second_qbrowser_on_the_same_profile_runs_on_a_temporary_one_and_says_so() {
    let _first_slot = Slot::take();
    let scratch = Scratch::new();
    let first = open(&scratch, &page("/links"));
    assert!(!first.screen().contains("temporary profile"), "{}", first.screen());
    let _second_slot = Slot::take();
    let mut second = open(&scratch, &page("/second"));
    until(&mut second, "the temporary profile label", |h| find_in_row(h, "temporary profile", 1).is_some());
    let (x, y) = find_in_row(&second, "temporary profile", 1).unwrap();
    second.hover(x + 2, y).advance(Duration::from_secs(1));
    assert!(second.screen().contains("Another qbrowser is using your profile"), "{}", second.screen());
    drop(first);
}

#[test]
fn quitting_ends_every_chromium_process() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/links"));
    let profile = scratch.path("home");
    let profile = profile.to_string_lossy();
    assert!(!processes_mentioning(&profile).is_empty(), "Chromium runs on the profile");
    h.press("ctrl+q");
    assert!(h.quit_requested());
    assert!(processes_mentioning(&profile).is_empty(), "nothing runs on the profile once qbrowser has quit");
    // Held on purpose until here: the harness still has the screen, only quitting ended Chromium.
    drop(h);
}

#[test]
fn what_chromium_says_while_it_is_ended_leaves_the_last_screen_as_it_was() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/links"));
    let tab = h.app().active_tab().cloned().expect("the tab");
    h.press("ctrl+q");
    // Ending Chromium closes its tabs, and the news can still be on its way to the screen.
    h.send(Msg::Engine(Event::TabClosed { tab: tab.clone() }));
    assert_eq!(h.app().active_tab(), Some(&tab));
    assert!(find_in_row(&h, "Links", 0).is_some(), "{}", h.screen());
}

#[test]
fn a_hangup_ends_chromium_too() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/links"));
    let profile = scratch.path("home").to_string_lossy().into_owned();
    h.terminate(Termination::Hangup);
    assert!(h.quit_requested());
    wait_until_none_mention(&profile);
    drop(h);
}

#[test]
fn a_terminal_too_small_says_so() {
    let scratch = Scratch::new();
    let mut h = open_on(scratch.machine_without_chromium(), None);
    h.resize(29, 20);
    assert!(h.screen().contains("The terminal is too small"), "{}", h.screen());
    h.resize(60, 7);
    assert!(h.screen().contains("The terminal is too small"), "{}", h.screen());
    h.resize(60, 20);
    assert!(!h.screen().contains("The terminal is too small"), "{}", h.screen());
}

#[test]
fn a_terminal_without_pictures_says_why_the_page_is_not_drawn() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/links"));
    h.set_graphics(Graphics::None);
    settle(&mut h);
    assert!(h.screen().contains("This terminal cannot show pictures"), "{}", h.screen());
    assert!(!page_drawn(&h), "{}", h.screen());
    assert!(find_in_row(&h, "Links", 0).is_some(), "the title stays on the tab:\n{}", h.screen());
}

#[test]
fn a_newer_version_is_asked_for_once_at_start() {
    let scratch = Scratch::new();
    let mut h = open_on(scratch.machine_without_chromium(), None);
    let asked = h.update_checks().to_vec();
    assert_eq!(asked.len(), 1);
    assert_eq!((asked[0].package(), asked[0].current()), ("quvyta-browser", env!("CARGO_PKG_VERSION")));
    h.set_latest_version(Some("9.9.9"));
    settle(&mut h);
    assert!(h.screen().contains("quvyta-browser 9.9.9 is out"), "{}", h.screen());
}

#[test]
fn with_the_quvyta_wide_switch_off_nothing_is_asked() {
    let scratch = Scratch::new();
    std::fs::write(
        scratch.path("config/quvyta.conf"),
        "language = \"en\"\nicons = \"unicode\"\nupdate-notice = false\n",
    )
    .unwrap();
    let mut h = open_on(scratch.machine_without_chromium(), None);
    assert!(h.update_checks().is_empty());
    h.set_latest_version(Some("9.9.9"));
    settle(&mut h);
    assert!(!h.screen().contains("is out"), "{}", h.screen());
}

//! Starting and ending: a missing Chromium, the profile lock, the orphan a killed qbrowser leaves
//! and the processes left after shutdown.

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use super::fixture::{Browser, CHROMIUM, PATIENCE, Scratch, Slot, page, wait_until_none_mention};
use crate::engine::{Engine, Event, Options, StartError, procfs};

#[test]
fn without_chromium_starting_says_not_found() {
    let scratch = Scratch::new();
    let options = Options {
        chromium: Some(scratch.path().join("no-such-chromium")),
        path_var: Some(scratch.dir("empty-bin").into_os_string()),
        ..scratch.options()
    };
    assert_eq!(Engine::start(&options).err(), Some(StartError::NotFound));
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_second_engine_on_the_same_profile_runs_on_a_temporary_one_that_is_removed() {
    let _slot = Slot::take_many(2);
    let scratch = Scratch::new();
    let options = scratch.options();
    let first = Browser::start(&options);
    let second = Browser::start(&options);
    assert_eq!(first.engine.profile().path, options.profile_home.join("profile"));
    assert!(!first.engine.profile().temporary);
    let temporary = second.engine.profile().path.clone();
    assert!(second.engine.profile().temporary);
    assert!(temporary.starts_with(&options.temp_root) && temporary.is_dir());
    let one = first.open(&page("/second"));
    let two = second.open(&page("/third"));
    assert_eq!(first.eval(&one, "document.title"), "Second");
    assert_eq!(second.eval(&two, "document.title"), "Third");
    second.engine.shutdown();
    assert!(!temporary.exists(), "the temporary profile is removed on shutdown");
    assert_eq!(first.eval(&one, "document.title"), "Second", "the first engine goes on");
    first.engine.shutdown();
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn after_shutdown_no_process_runs_on_the_profile() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let browser = Browser::start(&scratch.options());
    let profile = browser.engine.profile().path.to_string_lossy().into_owned();
    browser.open(&page("/links"));
    browser.open(&page("/long"));
    assert!(super::fixture::processes_mentioning(&profile).len() > 1, "Chromium runs helpers on the profile");
    browser.engine.shutdown();
    wait_until_none_mention(&profile);
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn a_chromium_that_ignores_being_closed_is_killed_with_its_helpers() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let browser = Browser::start(&scratch.options());
    let profile = browser.engine.profile().path.to_string_lossy().into_owned();
    browser.open(&page("/links"));
    // A stopped browser process cannot answer Browser.close; only the kill ends it. The stop is
    // sent the way the product sends its signals, so this test does not need a program that is
    // not on every machine. It needs nothing to undo: SIGKILL is not caught and not ignored, and
    // a stopped process is still killed by it, so the group kill below reaches the browser even
    // though it never ran again.
    let pid = browser.engine.chromium_pid();
    let stopped = rustix::process::Pid::from_raw(pid.try_into().expect("a process number")).expect("not zero");
    rustix::process::kill_process(stopped, rustix::process::Signal::STOP).expect("the stop is sent");
    let (done_tx, done) = mpsc::channel();
    let started = Instant::now();
    thread::spawn(move || {
        browser.engine.shutdown();
        let _ = done_tx.send(());
    });
    assert!(done.recv_timeout(PATIENCE).is_ok(), "shutdown did not end a Chromium that ignores it");
    assert!(started.elapsed() < Duration::from_secs(10));
    wait_until_none_mention(&profile);
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn chromium_starts_with_no_way_to_reach_the_desk() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let browser = Browser::start(&scratch.options());
    let pid = browser.engine.chromium_pid();
    let environ = std::fs::read(format!("/proc/{pid}/environ")).expect("Chromium's environment");
    for name in ["DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "XDG_RUNTIME_DIR"] {
        // Only what this machine has can be checked: a variable nobody set reaches nothing.
        if std::env::var_os(name).is_some() {
            let held = environ.split(|byte| *byte == 0).any(|entry| entry.starts_with(format!("{name}=").as_bytes()));
            assert!(!held, "{name} reaches Chromium:\n{}", String::from_utf8_lossy(&environ));
        }
    }
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn chromium_ending_on_its_own_is_reported_as_gone() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let browser = Browser::start(&scratch.options());
    browser.open(&page("/links"));
    procfs::kill_group(browser.engine.chromium_pid());
    let reason = browser.wait("Chromium to be gone", |event| match event {
        Event::Gone { reason } => Some(reason.clone()),
        _ => None,
    });
    assert!(!reason.is_empty());
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn an_orphan_chromium_left_on_the_profile_is_ended_by_the_next_start() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let options = scratch.options();
    let profile: PathBuf = options.profile_home.join("profile");
    std::fs::create_dir_all(&profile).unwrap();
    // What a qbrowser killed with SIGKILL leaves behind: its Chromium, still running on the
    // profile in a group of its own, and its id in the lock file.
    let mut user_data_dir = std::ffi::OsString::from("--user-data-dir=");
    user_data_dir.push(&profile);
    let orphan = Command::new(CHROMIUM)
        .args(["--headless=new", "--remote-debugging-port=0", "--no-first-run", "--no-startup-window"])
        .arg(user_data_dir)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .unwrap();
    let mut orphan = Orphan(orphan);
    let orphan_pid = orphan.0.id();
    let deadline = Instant::now() + PATIENCE;
    while profile.join("SingletonLock").symlink_metadata().is_err() {
        assert!(Instant::now() < deadline, "the orphan did not take the profile");
        thread::sleep(Duration::from_millis(50));
    }
    std::fs::write(options.profile_home.join("profile.lock"), orphan_pid.to_string()).unwrap();
    let browser = Browser::start(&options);
    assert!(procfs::has_exited(orphan_pid), "the orphan was ended");
    let _ = orphan.0.wait();
    assert!(!browser.engine.profile().temporary, "the profile is ours again");
    let tab = browser.open(&page("/second"));
    assert_eq!(browser.eval(&tab, "document.title"), "Second");
}

/// The orphan's process, ended with its helpers even when the test fails before the engine does it.
struct Orphan(std::process::Child);

impl Drop for Orphan {
    fn drop(&mut self) {
        if !procfs::has_exited(self.0.id()) {
            procfs::kill_group(self.0.id());
        }
        let _ = self.0.wait();
    }
}

/// The inodes of the sockets the processes `pids` hold.
fn sockets_of(pids: &[u32]) -> Vec<String> {
    let mut inodes = Vec::new();
    for pid in pids {
        let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/fd")) else { continue };
        for entry in entries.flatten() {
            let Ok(target) = std::fs::read_link(entry.path()) else { continue };
            let target = target.to_string_lossy().into_owned();
            if let Some(inode) = target.strip_prefix("socket:[").and_then(|rest| rest.strip_suffix(']')) {
                inodes.push(inode.to_owned());
            }
        }
    }
    inodes
}

/// The inodes of the TCP sockets on this machine that listen for connections.
fn listening_tcp() -> Vec<String> {
    let mut inodes = Vec::new();
    for table in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(text) = std::fs::read_to_string(table) else { continue };
        for line in text.lines().skip(1) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            // The fourth column is the state; 0A is LISTEN. The tenth is the socket's inode.
            if fields.get(3) == Some(&"0A")
                && let Some(inode) = fields.get(9)
            {
                inodes.push((*inode).to_owned());
            }
        }
    }
    inodes
}

#[test]
#[cfg_attr(not(chromium), ignore = "needs Chromium")]
fn no_chromium_process_listens_on_a_port_anyone_on_the_machine_could_reach() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let browser = Browser::start(&scratch.options());
    let profile = browser.engine.profile().path.to_string_lossy().into_owned();
    let tab = browser.open(&page("/links"));
    assert_eq!(browser.eval(&tab, "document.title"), "Links", "the browser is driven all the same");
    let pids = super::fixture::processes_mentioning(&profile);
    assert!(!pids.is_empty(), "Chromium runs on the profile");
    let held = sockets_of(&pids);
    let listening: Vec<String> = listening_tcp().into_iter().filter(|inode| held.contains(inode)).collect();
    assert!(listening.is_empty(), "Chromium listens on a TCP port (socket inodes {listening:?})");
    browser.engine.shutdown();
}

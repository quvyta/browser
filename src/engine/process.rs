//! Starting Chromium out of sight, reading where its DevTools endpoint listens, and ending it with
//! every process it started.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use super::procfs;

/// How long Chromium may take to say where its DevTools endpoint listens. A cold start on a
/// loaded machine takes seconds; this only has to be finite.
const START_WITHIN: Duration = Duration::from_secs(20);

/// How long a closed browser gets to leave on its own before its process group is killed.
const CLOSE_WITHIN: Duration = Duration::from_secs(3);

/// How many of Chromium's last stderr lines are kept to explain a failure.
const TAIL_LINES: usize = 20;

/// Chromium's last words on stderr, kept to explain why it failed or went away.
#[derive(Clone, Default)]
pub(super) struct Tail(Arc<Mutex<VecDeque<String>>>);

impl Tail {
    fn push(&self, line: String) {
        let mut lines = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if lines.len() == TAIL_LINES {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    /// The last line that says something about Chromium itself. Its complaints about the
    /// desktop's message bus are left out: the bus is cut off on purpose (see [`launch`]), so
    /// they are expected and never the reason for anything.
    pub(super) fn last(&self) -> Option<String> {
        let lines = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        lines.iter().rev().find(|line| !line.contains(":ERROR:dbus/") && !line.trim().is_empty()).cloned()
    }
}

/// A running Chromium: its process, which leads a process group of its own, and its stderr.
pub(super) struct Chromium {
    child: Child,
    tail: Tail,
    ended: bool,
}

impl Chromium {
    /// The browser process's id, which is also its process group's id.
    pub(super) fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Its last stderr lines.
    pub(super) fn tail(&self) -> Tail {
        self.tail.clone()
    }

    /// Waits at most [`CLOSE_WITHIN`] for the browser process to leave on its own, then kills
    /// its whole process group and reaps it. The process is only reaped after the group is
    /// killed: until then it stays a zombie at worst, which keeps its group's number from being
    /// reused, so the kill cannot reach anyone else. Safe to call more than once.
    pub(super) fn end(&mut self, grace: bool) {
        if self.ended {
            return;
        }
        self.ended = true;
        let pid = self.pid();
        if grace {
            let deadline = Instant::now() + CLOSE_WITHIN;
            while !procfs::has_exited(pid) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(20));
            }
        }
        // Helpers (renderers, the GPU and network processes) can outlive the browser process
        // for a moment; the group kill takes them all.
        procfs::kill_group(pid);
        let _ = self.child.wait();
    }
}

impl Drop for Chromium {
    fn drop(&mut self) {
        self.end(false);
    }
}

/// The command line Chromium is started with, apart from the program.
fn arguments(profile: &Path) -> Vec<OsString> {
    let mut user_data_dir = OsString::from("--user-data-dir=");
    user_data_dir.push(profile);
    vec![
        // The new headless mode is the full browser without a window, so pages draw as they do
        // on a desktop.
        "--headless=new".into(),
        // The system picks a free port and Chromium prints it on stderr: no clash between two
        // browsers, no guessing.
        "--remote-debugging-port=0".into(),
        user_data_dir,
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        // Without these Chromium asks the desktop's keyring for a password store, which can pop
        // a dialog on the owner's desktop.
        "--password-store=basic".into(),
        "--use-mock-keychain".into(),
        // Otherwise a first tab opens on its own; tabs are only ever opened on request.
        "--no-startup-window".into(),
    ]
}

/// Starts `program` headless on `profile` in a process group of its own and returns it with the
/// address of its browser-wide DevTools endpoint, or why it did not start: the reason the
/// program could not be run, or Chromium's last stderr line.
pub(super) fn launch(program: &Path, profile: &Path) -> Result<(Chromium, String), String> {
    let mut child = Command::new(program)
        .args(arguments(profile))
        // Nothing may reach the desktop: without a display Chromium cannot open a window
        // anywhere, and without the session bus it cannot show notifications or media controls.
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        // Its own group, so every helper process it starts can be ended with one signal.
        .process_group(0)
        .spawn()
        .map_err(|error| format!("{}: {error}", program.display()))?;
    let tail = Tail::default();
    let (address_tx, address_rx) = mpsc::channel();
    if let Some(stderr) = child.stderr.take() {
        let tail = tail.clone();
        // Keeps draining stderr for Chromium's whole life, so a full pipe never stops it.
        thread::Builder::new()
            .name("qbrowser-chromium-stderr".into())
            .spawn(move || drain(stderr, &tail, &address_tx))
            .map_err(|error| error.to_string())?;
    }
    let mut chromium = Chromium { child, tail, ended: false };
    match address_rx.recv_timeout(START_WITHIN) {
        Ok(address) => Ok((chromium, address)),
        Err(wait) => {
            chromium.end(false);
            let why = match wait {
                mpsc::RecvTimeoutError::Timeout => {
                    format!("Chromium did not open its DevTools endpoint within {} seconds", START_WITHIN.as_secs())
                }
                mpsc::RecvTimeoutError::Disconnected => "Chromium exited while starting".to_owned(),
            };
            Err(chromium.tail.last().unwrap_or(why))
        }
    }
}

/// Reads stderr line by line until it closes, keeping the last lines and handing over the
/// DevTools address once it appears.
fn drain(stderr: ChildStderr, tail: &Tail, address: &mpsc::Sender<String>) {
    const MARK: &str = "DevTools listening on ";
    let mut reader = BufReader::new(stderr);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&line).trim_end().to_owned();
        if let Some(start) = text.find(MARK) {
            let _ = address.send(text[start + MARK.len()..].trim().to_owned());
        }
        tail.push(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tail_keeps_the_last_lines_and_skips_the_bus_complaints() {
        let tail = Tail::default();
        for n in 0..30 {
            tail.push(format!("line {n}"));
        }
        tail.push("[1:2:0924/154627.089730:ERROR:dbus/bus.cc:405] Failed to connect to the bus".into());
        tail.push(String::new());
        assert_eq!(tail.last().as_deref(), Some("line 29"));
        assert_eq!(tail.0.lock().unwrap().len(), TAIL_LINES);
    }

    #[test]
    fn a_program_that_is_not_chromium_fails_with_its_last_words() {
        let scratch = crate::engine::tests::fixture::Scratch::new();
        let fake = scratch.path().join("fake-chromium");
        std::fs::write(&fake, "#!/bin/sh\necho 'cannot open display' >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let error = launch(&fake, scratch.path()).err().unwrap();
        assert_eq!(error, "cannot open display");
    }
}

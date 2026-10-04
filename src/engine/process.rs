//! Starting Chromium out of sight with its DevTools connection on a pair of pipes, waiting for its
//! first answer, and ending it with every process it started.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::cdp::{self, Wire, Work};
use super::procfs;

/// How long Chromium may take to answer its first DevTools call. A cold start on a loaded machine
/// takes seconds; this only has to be finite.
const START_WITHIN: Duration = Duration::from_secs(20);

/// How long a closed browser gets to leave on its own before its process group is killed.
const CLOSE_WITHIN: Duration = Duration::from_secs(3);

/// How long a Chromium that failed to start is given for its last stderr lines to be read.
const STDERR_WITHIN: Duration = Duration::from_secs(2);

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
        // The DevTools connection runs over descriptors 3 and 4 (see `launch`) rather than a
        // port: a port on 127.0.0.1 has no password, and every process and every user on the
        // machine could connect to it, read the tabs, take the cookies and drive the pages.
        "--remote-debugging-pipe".into(),
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

/// What makes descriptor 3 of the program the pipe qbrowser writes into and descriptor 4 the one
/// it reads from: the shell moves its standard input and output there, points them at
/// `/dev/null` and runs the program in its own place, with the same process id. The standard
/// library can only hand a child its first three descriptors, and moving more of them by hand
/// takes `unsafe`, which this crate forbids.
const PIPES: &str = "exec 3<&0 4>&1 0</dev/null 1>/dev/null; exec \"$0\" \"$@\"";

/// How `program` is started on `profile`: headless, out of the desktop's reach, in a process
/// group of its own, its DevTools connection on descriptors 3 and 4.
fn command(program: &Path, profile: &Path, extra: &[OsString]) -> Command {
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(PIPES)
        .arg(program)
        .args(arguments(profile))
        .args(extra)
        // Nothing may reach the desktop: without a display Chromium cannot open a window
        // anywhere, and without the session bus it cannot show notifications or media controls.
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        // Without the runtime folder it cannot find a Wayland socket either.
        .env_remove("XDG_RUNTIME_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Its own group, so every helper process it starts can be ended with one signal.
        .process_group(0);
    command
}

/// A started Chromium, ready to be driven: the process, the wire its calls go out on, and the
/// channel its messages come in on, which the engine's commands share.
pub(super) struct Launched {
    pub(super) chromium: Chromium,
    pub(super) wire: Wire,
    pub(super) work: Sender<Work>,
    pub(super) incoming: Receiver<Work>,
}

/// Starts `program` headless on `profile` in a process group of its own and waits for its first
/// DevTools answer, or says why it did not start: the reason the program could not be run, or
/// Chromium's last stderr line.
pub(super) fn launch(program: &Path, profile: &Path, extra: &[OsString]) -> Result<Launched, String> {
    let mut child =
        command(program, profile, extra).spawn().map_err(|error| format!("{}: {error}", program.display()))?;
    let tail = Tail::default();
    let (Some(to_chromium), Some(from_chromium)) = (child.stdin.take(), child.stdout.take()) else {
        return Err("the DevTools pipes were not made".to_owned());
    };
    // Says when stderr has closed, so a failed start is explained by its last line.
    let (drained_tx, drained) = mpsc::channel::<()>();
    if let Some(stderr) = child.stderr.take() {
        let tail = tail.clone();
        // Keeps draining stderr for Chromium's whole life, so a full pipe never stops it.
        thread::Builder::new()
            .name("qbrowser-chromium-stderr".into())
            .spawn(move || {
                drain(stderr, &tail);
                drop(drained_tx);
            })
            .map_err(|error| error.to_string())?;
    }
    let mut chromium = Chromium { child, tail, ended: false };
    let (work, incoming) = mpsc::channel();
    {
        let work = work.clone();
        thread::Builder::new()
            .name("qbrowser-devtools-read".into())
            .spawn(move || cdp::read(from_chromium, &work))
            .map_err(|error| error.to_string())?;
    }
    let mut wire = Wire::new(to_chromium);
    let first = wire.call(None, "Browser.getVersion", json!({}));
    match first_answer(&incoming, first) {
        Ok(()) => Ok(Launched { chromium, wire, work, incoming }),
        Err(why) => {
            chromium.end(false);
            // Its pipes can close a moment before its last words are read.
            let _ = drained.recv_timeout(STDERR_WITHIN);
            Err(chromium.tail.last().unwrap_or(why))
        }
    }
}

/// Waits at most [`START_WITHIN`] for the answer to call `id`; what else comes before it is
/// Chromium's business and passed over.
fn first_answer(incoming: &Receiver<Work>, id: u64) -> Result<(), String> {
    let deadline = Instant::now() + START_WITHIN;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match incoming.recv_timeout(left) {
            Ok(Work::Message(message)) if message.get("id").and_then(Value::as_u64) == Some(id) => return Ok(()),
            Ok(Work::Message(_) | Work::Command(_)) => {}
            Ok(Work::Closed(_)) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("Chromium exited while starting".to_owned());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(format!("Chromium did not answer within {} seconds", START_WITHIN.as_secs()));
            }
        }
    }
}

/// Reads stderr line by line until it closes, keeping the last lines.
fn drain(stderr: ChildStderr, tail: &Tail) {
    let mut reader = BufReader::new(stderr);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        tail.push(String::from_utf8_lossy(&line).trim_end().to_owned());
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
        let error = launch(&fake, scratch.path(), &[]).err().unwrap();
        assert_eq!(error, "cannot open display");
    }

    /// The value `command` gives `name` in the program's environment: `Some(None)` when it takes
    /// it away.
    fn env_of(command: &Command, name: &str) -> Option<Option<String>> {
        command
            .get_envs()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.map(|value| value.to_string_lossy().into_owned()))
    }

    #[test]
    fn chromium_starts_headless_away_from_the_desktop_and_without_a_port() {
        let command = command(Path::new("/opt/chromium"), Path::new("/home/p/profile"), &["--extra".into()]);
        for name in ["DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS"] {
            assert_eq!(env_of(&command, name), Some(None), "{name} is taken away from Chromium");
        }
        let words: Vec<String> = command.get_args().map(|word| word.to_string_lossy().into_owned()).collect();
        assert_eq!(words[..3], ["-c", PIPES, "/opt/chromium"], "the shell hands the pipes over and runs Chromium");
        for flag in
            ["--headless=new", "--remote-debugging-pipe", "--user-data-dir=/home/p/profile", "--no-startup-window"]
        {
            assert!(words.iter().any(|word| word == flag), "{flag} in {words:?}");
        }
        assert_eq!(words.last().map(String::as_str), Some("--extra"), "the caller's switches come last");
        assert!(
            !words.iter().any(|word| word.starts_with("--remote-debugging-port")),
            "no DevTools port anyone on the machine could reach: {words:?}"
        );
    }
}

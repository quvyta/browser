//! Chromium, driven from outside: finding it, starting it on a profile of its own, speaking the
//! DevTools protocol to it and ending it with every process it started.
//!
//! [`Engine::start`] runs Chromium headless and hands back the engine with a channel of
//! [`Event`]s. Every command goes to one socket thread and returns at once; only
//! [`Engine::evaluate`] waits, and never longer than it is told.

mod cdp;
mod connection;
mod find;
mod history;
pub mod input;
mod process;
mod procfs;
mod profile;
mod reader;
mod tabs;

#[cfg(test)]
pub(crate) mod tests;

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Value, json};

pub use find::find_chromium;
pub use history::Entry;
pub use input::{Button, KeyPress, Modifiers, Mouse};
pub use profile::Profile;
pub use reader::Reading;
// The size a zoom makes of a page area, for the screen's own tests, which ask what the engine
// hands Chromium without a browser to hand it to.
#[cfg(test)]
pub(crate) use tabs::{ZOOM_BOUNDS, css_pixels};

#[cfg(test)]
// The application's tests end a process the way the product does. `procfs` is private to the
// engine, and these are its only two signals, so they are lent out for as long as tests are built.
pub(crate) use procfs::{kill_group, kill_process};

use cdp::Work;
use process::Chromium;
use profile::Claim;
use tabs::Command;

/// A tab: the DevTools protocol's target id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TabId(pub String);

/// Where Chromium is and which profile it runs on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// The Chromium to run; `None` searches `path_var` (see [`find_chromium`]).
    pub chromium: Option<PathBuf>,
    /// The program search path; `None` finds nothing there.
    pub path_var: Option<OsString>,
    /// The folder holding the persistent profile (`<folder>/profile`) and its lock
    /// (`<folder>/profile.lock`).
    pub profile_home: PathBuf,
    /// Where a temporary profile is made when the persistent one is in use.
    pub temp_root: PathBuf,
    /// More command-line switches for Chromium, after qbrow's own; empty for the person's own
    /// browser. The tests give theirs a proxy nobody listens on, so no page and none of
    /// Chromium's own background requests reach the internet.
    pub extra_arguments: Vec<OsString>,
}

/// Why the engine did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    /// No Chromium was found (see [`find_chromium`]).
    NotFound,
    /// Chromium was found but did not start: its last stderr line, or why it could not be run.
    Failed(String),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str("Chromium was not found"),
            Self::Failed(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for StartError {}

/// What Chromium reports, in the order it happened.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A tab opened: asked for with [`Engine::open_tab`] (`opener` is `None`), or opened by a
    /// page (`target=_blank`, `window.open`) with the tab that opened it as `opener`.
    TabOpened {
        /// The new tab.
        tab: TabId,
        /// The tab whose page opened it.
        opener: Option<TabId>,
        /// Its address as Chromium knows it at the moment; [`Event::Navigated`] follows.
        url: String,
    },
    /// A tab closed: asked for, or the page closed itself.
    TabClosed {
        /// The tab.
        tab: TabId,
    },
    /// A new picture of the shown tab's viewport, as JPEG.
    Frame {
        /// The tab.
        tab: TabId,
        /// The picture.
        jpeg: Vec<u8>,
    },
    /// The tab is at a new address, or its history changed.
    Navigated {
        /// The tab.
        tab: TabId,
        /// Where it is.
        url: String,
        /// Whether there is somewhere to go back to.
        can_back: bool,
        /// Whether there is somewhere to go forward to.
        can_forward: bool,
    },
    /// The tab's history changed: every step, and which one the tab is at now.
    History {
        /// The tab.
        tab: TabId,
        /// Which of the `entries` the tab is at.
        current: usize,
        /// Every step of the tab's own history, the oldest first.
        entries: Vec<Entry>,
    },
    /// The page's title changed.
    Title {
        /// The tab.
        tab: TabId,
        /// The new title; empty when the page has none.
        title: String,
    },
    /// The page's text selection changed.
    Selection {
        /// The tab.
        tab: TabId,
        /// What the page has selected, exactly as it reports it; empty when it has no selection.
        text: String,
    },
    /// The tab's page answered an [`Engine::ask`], with its value or why there is none.
    Answered {
        /// The tab.
        tab: TabId,
        /// The value the expression gave, or the exception it threw.
        value: Result<Value, String>,
    },
    /// The page began or finished loading.
    Loading {
        /// The tab.
        tab: TabId,
        /// Whether it is loading now.
        loading: bool,
    },
    /// The tab's renderer died; reloading brings it back.
    Crashed {
        /// The tab.
        tab: TabId,
    },
    /// The page opened one of its own dialogs (`alert`, `confirm`, `prompt`, or the question
    /// before leaving a page). The page waits, doing nothing, until it is answered with
    /// [`Engine::answer_dialog`].
    Dialog {
        /// The tab.
        tab: TabId,
        /// Which dialog.
        kind: DialogKind,
        /// What the page says in it; empty for the question before leaving, whose words browsers
        /// no longer let a page choose.
        message: String,
        /// The text a `prompt` offers to start with.
        default_text: String,
    },
    /// The tab's dialog closed without an answer from qbrowser: the page went away under it.
    DialogClosed {
        /// The tab.
        tab: TabId,
    },
    /// Chromium ended without being asked to, and why: its last stderr line when it left one.
    Gone {
        /// Why.
        reason: String,
    },
}

/// Which of a page's own dialogs is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    /// `alert()`: a message and one button.
    Alert,
    /// `confirm()`: a question, yes or no.
    Confirm,
    /// `prompt()`: a question with a line of text to answer.
    Prompt,
    /// The page asks whether to leave it, when it may lose what was typed in it.
    BeforeUnload,
}

/// A running Chromium and the connection that drives it.
pub struct Engine {
    commands: Option<Sender<Work>>,
    socket: Option<JoinHandle<()>>,
    chromium: Chromium,
    claim: Claim,
    closing: Arc<AtomicBool>,
    pid: u32,
}

impl Engine {
    /// Starts Chromium headless and connects; no tab is open yet.
    ///
    /// # Errors
    ///
    /// [`StartError::NotFound`] when no Chromium is found, [`StartError::Failed`] when it does
    /// not start or cannot be connected to.
    pub fn start(options: &Options) -> Result<(Engine, Receiver<Event>), StartError> {
        let program =
            find_chromium(options.chromium.as_deref(), options.path_var.as_deref()).ok_or(StartError::NotFound)?;
        let mut claim = profile::claim(&options.profile_home, &options.temp_root).map_err(StartError::Failed)?;
        let process::Launched { chromium, mut wire, work, incoming } =
            process::launch(&program, &claim.profile().path, &options.extra_arguments).map_err(StartError::Failed)?;
        let pid = chromium.pid();
        claim.record(pid);
        // On an early return from here on, dropping `chromium` ends it and dropping `claim`
        // releases the profile, in that order.
        wire.call(None, "Target.setDiscoverTargets", json!({ "discover": true }));
        let (events, events_rx) = mpsc::channel();
        let closing = Arc::new(AtomicBool::new(false));
        let socket = {
            let closing = Arc::clone(&closing);
            let tail = chromium.tail();
            thread::Builder::new()
                .name("qbrowser-devtools".into())
                .spawn(move || connection::run(wire, &incoming, events, &closing, &tail))
                .map_err(|error| StartError::Failed(error.to_string()))?
        };
        let commands = work;
        let engine = Engine { commands: Some(commands), socket: Some(socket), chromium, claim, closing, pid };
        Ok((engine, events_rx))
    }

    /// The profile Chromium runs on.
    #[must_use]
    pub fn profile(&self) -> &Profile {
        self.claim.profile()
    }

    /// The browser process's id; it leads a process group holding every process Chromium starts.
    #[must_use]
    pub fn chromium_pid(&self) -> u32 {
        self.pid
    }

    fn send(&self, command: Command) {
        if let Some(commands) = &self.commands {
            let _ = commands.send(Work::Command(command));
        }
    }

    /// A way of asking the engine something from another thread: work the drawing thread must not
    /// wait for, such as reading a page, takes one of these with it.
    #[must_use]
    pub fn client(&self) -> Client {
        Client(self.commands.clone())
    }

    /// Opens a tab on `url`; answered by [`Event::TabOpened`] with no opener.
    pub fn open_tab(&self, url: &str) {
        self.send(Command::Open(url.to_owned()));
    }

    /// Closes the tab; answered by [`Event::TabClosed`].
    pub fn close_tab(&self, tab: &TabId) {
        self.send(Command::Close(tab.clone()));
    }

    /// Loads `url` in the tab.
    pub fn navigate(&self, tab: &TabId, url: &str) {
        self.send(Command::Call(tab.clone(), "Page.navigate", json!({ "url": url })));
    }

    /// Goes back one step in the tab's history, if there is a step to go.
    pub fn back(&self, tab: &TabId) {
        self.send(Command::History(tab.clone(), -1));
    }

    /// Goes forward one step in the tab's history, if there is a step to go.
    pub fn forward(&self, tab: &TabId) {
        self.send(Command::History(tab.clone(), 1));
    }

    /// Goes `by` steps through the tab's history, back when it is negative, the way the arrows go
    /// one: a step chosen from the list beside an arrow is a step through the tab's own history,
    /// not a new page laid on top of it.
    pub fn step(&self, tab: &TabId, by: i64) {
        self.send(Command::History(tab.clone(), by));
    }

    /// Loads the tab's page again.
    pub fn reload(&self, tab: &TabId) {
        self.send(Command::Call(tab.clone(), "Page.reload", json!({})));
    }

    /// Stops the tab's loading.
    pub fn stop(&self, tab: &TabId) {
        self.send(Command::Call(tab.clone(), "Page.stopLoading", json!({})));
    }

    /// Lays the tab's page out for a viewport of `width` × `height` CSS pixels at scale 1; the
    /// frames take that size.
    ///
    /// `width` and `height` are the page area's own pixels and the zoom in force divides them: at
    /// 100% that is the scale-1 case above, and at any other level the page is laid out for
    /// `width / z` CSS pixels and drawn `z` times as large (see [`Engine::set_zoom`]).
    pub fn set_viewport(&self, tab: &TabId, width: u32, height: u32) {
        self.send(Command::Viewport(tab.clone(), width, height));
    }

    /// Draws the tab's page at `percent` of its size: the page lays out for a viewport this many
    /// times smaller and the frame comes back the size the page area is.
    ///
    /// A `percent` outside the range the ladder allows is brought inside it, the way a person's own
    /// zoom control never goes past its ends; the level in force is the one the tab is then drawn
    /// at, and it is the tab's own until it is changed again.
    pub fn set_zoom(&self, tab: &TabId, percent: u32) {
        self.send(Command::Zoom(tab.clone(), percent));
    }

    /// Keeps every [`Event::Frame`] within `width` × `height` pixels, or with `None` lets it be the
    /// page area's own size again. The page is laid out as before; only the picture Chromium
    /// sends is smaller.
    ///
    /// A screen that draws a cell as two pixels, one above the other, can show no more than two
    /// pixels a cell: a frame of the page area's full size is encoded by Chromium, carried, decoded
    /// and then thrown away all but a sliver of it.
    pub fn set_picture_limit(&self, limit: Option<(u32, u32)>) {
        self.send(Command::PictureLimit(limit));
    }

    /// Makes `tab` the one whose [`Event::Frame`]s flow; the others' stop.
    pub fn show(&self, tab: &TabId) {
        self.send(Command::Show(tab.clone()));
    }

    /// Hands the tab's page what the mouse did at (`x`, `y`) in CSS pixels.
    pub fn mouse(&self, tab: &TabId, mouse: Mouse, x: f64, y: f64, modifiers: Modifiers) {
        self.send(Command::Call(tab.clone(), "Input.dispatchMouseEvent", input::mouse_params(mouse, x, y, modifiers)));
    }

    /// Presses and releases a key in the tab's page.
    pub fn key(&self, tab: &TabId, press: &KeyPress) {
        for params in input::key_params(press) {
            self.send(Command::Call(tab.clone(), "Input.dispatchKeyEvent", params));
        }
    }

    /// Puts `text` where the tab's page has its caret, as a paste does.
    pub fn insert_text(&self, tab: &TabId, text: &str) {
        self.send(Command::Call(tab.clone(), "Input.insertText", json!({ "text": text })));
    }

    /// Answers the page's open dialog: `accept` is OK (or "Leave"), otherwise Cancel (or "Stay");
    /// `text` is a prompt's answer.
    pub fn answer_dialog(&self, tab: &TabId, accept: bool, text: Option<&str>) {
        let mut params = json!({ "accept": accept });
        if let Some(text) = text {
            params["promptText"] = json!(text);
        }
        self.send(Command::Call(tab.clone(), "Page.handleJavaScriptDialog", params));
    }

    /// Runs `expression` in the tab and waits at most `within` for its value (returnByValue).
    ///
    /// # Errors
    ///
    /// The exception the expression threw, or why no value came: no such tab, Chromium gone, or
    /// `within` passed.
    pub fn evaluate(&self, tab: &TabId, expression: &str, within: Duration) -> Result<Value, String> {
        self.client().evaluate(tab, expression, within)
    }

    /// The tab's page's own text, as [`Reading`], waiting at most `within` for it.
    ///
    /// The script runs in a world of the tab's own that the page's scripts cannot see or reach, so
    /// a page cannot tell that it was read, and it changes nothing while it is read.
    ///
    /// # Errors
    ///
    /// Why there is no reading: the tab is not there, Chromium is gone, `within` passed, the
    /// script threw, or the page is not a document at all.
    pub fn read_page(&self, tab: &TabId, within: Duration) -> Result<Reading, String> {
        self.client().read_page(tab, within)
    }

    /// Runs `expression` in the tab's isolated world, where the page's own scripts can neither see
    /// it nor change what it finds, and waits up to `within` for its value.
    ///
    /// # Errors
    ///
    /// The exception the expression threw, or why no value came: no such tab, no isolated world
    /// yet in its document, Chromium gone, or `within` passed.
    pub fn evaluate_isolated(&self, tab: &TabId, expression: &str, within: Duration) -> Result<Value, String> {
        let (reply, answer) = mpsc::channel();
        self.send(Command::EvaluateIsolated(tab.clone(), expression.to_owned(), reply));
        match answer.recv_timeout(within) {
            Ok(value) => value,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(format!("no value within {} ms", within.as_millis())),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err("Chromium is gone".to_owned()),
        }
    }

    /// Runs `expression` in the tab and reports its value as [`Event::Answered`], without waiting.
    pub fn ask(&self, tab: &TabId, expression: &str) {
        self.send(Command::Ask(tab.clone(), expression.to_owned()));
    }

    /// `Browser.close`, a bounded wait, then SIGKILL to Chromium's process group; removes a
    /// temporary profile. Drop does the same.
    pub fn shutdown(mut self) {
        self.finish();
    }

    fn finish(&mut self) {
        let Some(commands) = self.commands.take() else { return };
        self.closing.store(true, Ordering::SeqCst);
        let _ = commands.send(Work::Command(Command::Quit));
        self.chromium.end(true);
        // With Chromium dead its pipe closes and the thread hears so; in case a straggler still
        // held the pipe, the thread is told itself that the connection is over.
        let _ = commands.send(Work::Closed("qbrowser closed Chromium".to_owned()));
        drop(commands);
        if let Some(socket) = self.socket.take() {
            let _ = socket.join();
        }
        self.claim.release();
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.finish();
    }
}

/// The engine's own way of being asked something from another thread: the way to the socket
/// thread and nothing else, so it can be cloned and carried into work that must not hold up
/// drawing while it waits for an answer.
#[derive(Debug, Clone)]
pub struct Client(Option<Sender<Work>>);

impl Client {
    fn send(&self, command: Command) {
        if let Some(commands) = &self.0 {
            let _ = commands.send(Work::Command(command));
        }
    }

    /// Runs `expression` in the tab and waits at most `within` for its value (returnByValue).
    ///
    /// # Errors
    ///
    /// The exception the expression threw, or why no value came: no such tab, Chromium gone, or
    /// `within` passed.
    pub fn evaluate(&self, tab: &TabId, expression: &str, within: Duration) -> Result<Value, String> {
        let (reply, answer) = mpsc::channel();
        self.send(Command::Evaluate(tab.clone(), expression.to_owned(), reply));
        wait(&answer, within)
    }

    /// The tab's page's own text, as [`Reading`], waiting at most `within` for it; see
    /// [`Engine::read_page`].
    ///
    /// # Errors
    ///
    /// Why there is no reading: the tab is not there, Chromium is gone, `within` passed, the
    /// script threw, or the page is not a document at all.
    pub fn read_page(&self, tab: &TabId, within: Duration) -> Result<Reading, String> {
        let (reply, answer) = mpsc::channel();
        self.send(Command::Read(tab.clone(), reply));
        wait(&answer, within)
    }
}

/// The answer of a call the socket thread owed, or why it never came within the time allowed.
fn wait<T>(answer: &Receiver<Result<T, String>>, within: Duration) -> Result<T, String> {
    match answer.recv_timeout(within) {
        Ok(answer) => answer,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!("no value within {} ms", within.as_millis())),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err("Chromium is gone".to_owned()),
    }
}

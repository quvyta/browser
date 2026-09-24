//! Chromium, driven from outside: finding it, starting it on a profile of its own, speaking the
//! DevTools protocol to it and ending it with every process it started.
//!
//! [`Engine::start`] runs Chromium headless and hands back the engine with a channel of
//! [`Event`]s. Every command goes to one socket thread and returns at once; only
//! [`Engine::evaluate`] waits, and never longer than it is told.

mod cdp;
mod connection;
mod find;
pub mod input;
mod process;
mod procfs;
mod profile;
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
pub use input::{Button, KeyPress, Modifiers, Mouse};
pub use profile::Profile;

use cdp::Wire;
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
    /// The page's title changed.
    Title {
        /// The tab.
        tab: TabId,
        /// The new title; empty when the page has none.
        title: String,
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
    /// Chromium ended without being asked to, and why: its last stderr line when it left one.
    Gone {
        /// Why.
        reason: String,
    },
}

/// A running Chromium and the connection that drives it.
pub struct Engine {
    commands: Option<Sender<Command>>,
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
        let (chromium, address) = process::launch(&program, &claim.profile().path).map_err(StartError::Failed)?;
        let pid = chromium.pid();
        claim.record(pid);
        // On an early return from here on, dropping `chromium` ends it and dropping `claim`
        // releases the profile, in that order.
        let mut wire = Wire::connect(&address).map_err(StartError::Failed)?;
        wire.call(None, "Target.setDiscoverTargets", json!({ "discover": true }));
        let (commands, commands_rx) = mpsc::channel();
        let (events, events_rx) = mpsc::channel();
        let closing = Arc::new(AtomicBool::new(false));
        let socket = {
            let closing = Arc::clone(&closing);
            let tail = chromium.tail();
            thread::Builder::new()
                .name("qbrowser-devtools".into())
                .spawn(move || connection::run(wire, &commands_rx, events, &closing, &tail))
                .map_err(|error| StartError::Failed(error.to_string()))?
        };
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
            let _ = commands.send(command);
        }
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
    pub fn set_viewport(&self, tab: &TabId, width: u32, height: u32) {
        self.send(Command::Viewport(tab.clone(), width, height));
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

    /// Runs `expression` in the tab and waits at most `within` for its value (returnByValue).
    ///
    /// # Errors
    ///
    /// The exception the expression threw, or why no value came: no such tab, Chromium gone, or
    /// `within` passed.
    pub fn evaluate(&self, tab: &TabId, expression: &str, within: Duration) -> Result<Value, String> {
        let (reply, answer) = mpsc::channel();
        self.send(Command::Evaluate(tab.clone(), expression.to_owned(), reply));
        match answer.recv_timeout(within) {
            Ok(value) => value,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(format!("no value within {} ms", within.as_millis())),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err("Chromium is gone".to_owned()),
        }
    }

    /// `Browser.close`, a bounded wait, then SIGKILL to Chromium's process group; removes a
    /// temporary profile. Drop does the same.
    pub fn shutdown(mut self) {
        self.finish();
    }

    fn finish(&mut self) {
        let Some(commands) = self.commands.take() else { return };
        self.closing.store(true, Ordering::SeqCst);
        let _ = commands.send(Command::Quit);
        self.chromium.end(true);
        // With Chromium dead the socket is closed, and without its sender the thread stops at
        // its next look at the commands in any case.
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

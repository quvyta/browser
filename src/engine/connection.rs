//! The socket thread: the only owner of the DevTools connection. It sleeps until the engine asks
//! for something or Chromium says something, and serves them in the order they came.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};

use super::Event;
use super::cdp::{Wire, Work};
use super::process::Tail;
use super::tabs::Tabs;

/// Runs until the connection closes or everyone who could hand it work is gone. An end nobody
/// asked for (Chromium crashed or was killed) is reported as [`Event::Gone`], with Chromium's last
/// stderr line when there is one; an end after `closing` was set is not.
pub(super) fn run(
    mut wire: Wire,
    work: &Receiver<Work>,
    events: Sender<Event>,
    closing: &Arc<AtomicBool>,
    tail: &Tail,
) {
    let mut tabs = Tabs::new(events);
    while let Ok(next) = work.recv() {
        match next {
            Work::Command(command) => tabs.command(&mut wire, command),
            Work::Message(message) => tabs.message(&mut wire, &message),
            Work::Closed(why) => {
                tabs.abandon();
                if !closing.load(Ordering::SeqCst) {
                    tabs.emit(Event::Gone { reason: tail.last().unwrap_or(why) });
                }
                return;
            }
        }
    }
    tabs.abandon();
}

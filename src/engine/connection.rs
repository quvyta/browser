//! The socket thread: the only owner of the DevTools connection. It serves the engine's commands
//! and Chromium's messages in turn, never waiting long on either.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use super::Event;
use super::cdp::{Incoming, Wire};
use super::process::Tail;
use super::tabs::{Command, Tabs};

/// Runs until the connection closes or the engine lets go of its end of `commands`. An end
/// nobody asked for (Chromium crashed or was killed) is reported as [`Event::Gone`], with
/// Chromium's last stderr line when there is one; an end after `closing` was set is not.
pub(super) fn run(
    mut wire: Wire,
    commands: &Receiver<Command>,
    events: Sender<Event>,
    closing: &Arc<AtomicBool>,
    tail: &Tail,
) {
    let mut tabs = Tabs::new(events);
    loop {
        loop {
            match commands.try_recv() {
                Ok(command) => tabs.command(&mut wire, command),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    tabs.abandon();
                    return;
                }
            }
        }
        match wire.receive() {
            Incoming::Message(message) => tabs.message(&mut wire, &message),
            Incoming::Idle => {}
            Incoming::Closed(why) => {
                tabs.abandon();
                if !closing.load(Ordering::SeqCst) {
                    tabs.emit(Event::Gone { reason: tail.last().unwrap_or(why) });
                }
                return;
            }
        }
    }
}

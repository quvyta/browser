//! Between the engine and the screen: starting Chromium off the drawing thread, the task that
//! carries its events to the screen and says when a cell's pixel size changed, and the size of
//! the page in pixels.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use qframe::geometry::Size;
use qframe::runtime::{Command, RecvWait, Task, TaskCx, TaskId};
use qframe::widgets::ImageData;

use super::{Machine, Msg};
use crate::engine::{Engine, Event, Options, StartError, TabId};

/// How long the event task waits for Chromium before it looks again whether the cell's pixel size
/// changed. What Chromium sends wakes it at once; only that look needs the clock (see
/// [`SeenCell`]), and a font size change seen a twentieth of a second late is not noticed.
const CELL_LOOK: Duration = Duration::from_millis(50);

/// The largest picture decoded, far past any screen, so a frame is never shrunk on the way.
const LARGEST: (u32, u32) = (16_384, 16_384);

/// A value moved once from a background thread to the application. Messages are cloned by the
/// widgets that send them, and neither the engine nor its channel can be, so the message
/// carries a shared slot the first reader empties.
pub struct Handover<T>(Arc<Mutex<Option<T>>>);

impl<T> Handover<T> {
    fn new(value: T) -> Self {
        Self(Arc::new(Mutex::new(Some(value))))
    }

    /// The value, the first time it is asked for.
    pub(super) fn take(&self) -> Option<T> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

impl<T> Clone for Handover<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T> std::fmt::Debug for Handover<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Handover")
    }
}

/// A started engine and the channel of its events.
pub(super) type Started = (Engine, Receiver<Event>);

/// Starts Chromium off the drawing thread, which it would hold for a second or more.
pub(super) fn start(machine: &Machine) -> Command<Msg> {
    let options = Options {
        chromium: machine.chromium.clone(),
        path_var: machine.path_var.clone(),
        profile_home: machine.profile_home.clone(),
        temp_root: machine.temp_root.clone(),
        extra_arguments: machine.chromium_arguments.clone(),
    };
    Command::perform(move || Msg::Started(Handover::new(Engine::start(&options))))
}

/// The result of [`start`], as the message carries it.
pub(super) type StartResult = Handover<Result<Started, StartError>>;

/// The task that carries the engine's events to the screen until the engine is gone or the task
/// is cancelled. Of the frames that came since it last looked only the newest of each tab is
/// decoded, here rather than on the drawing thread.
pub(super) fn pump(events: Receiver<Event>, seen: SeenCell, laid_out: (u32, u32)) -> (TaskId, Command<Msg>) {
    let task = Task::new("chromium", move |cx: &TaskCx<Msg>| {
        let mut laid_out = laid_out;
        loop {
            let cell = seen.pixels();
            if cell != laid_out {
                laid_out = cell;
                cx.send(Msg::Cell(cell));
            }
            let first = match cx.recv_timeout(&events, CELL_LOOK) {
                Ok(event) => event,
                Err(RecvWait::Timeout) => continue,
                Err(RecvWait::Closed) => return Err("the engine is gone".to_owned()),
                Err(_) => return Err("stopped".to_owned()),
            };
            // What else came meanwhile is taken in the same turn, so a tab that sent frames faster
            // than they are decoded has only its newest one decoded.
            let mut frames: HashMap<TabId, Vec<u8>> = HashMap::new();
            let mut next = Some(first);
            let ended = loop {
                match next.take().map_or_else(|| events.try_recv(), Ok) {
                    Ok(Event::Frame { tab, jpeg }) => {
                        frames.insert(tab, jpeg);
                    }
                    Ok(event) => cx.send(Msg::Engine(event)),
                    Err(TryRecvError::Empty) => break false,
                    Err(TryRecvError::Disconnected) => break true,
                }
            };
            for (tab, jpeg) in frames {
                // A frame cut short is skipped; the next one replaces it.
                if let Ok(picture) = ImageData::decode_bytes(&jpeg, LARGEST) {
                    cx.send(Msg::Picture(tab, picture));
                }
            }
            if ended {
                return Err("the engine is gone".to_owned());
            }
        }
    });
    let id = task.id();
    (id, Command::task(task))
}

/// The pixel size of a cell when the terminal reports none: a common terminal font's.
pub(super) const FALLBACK_CELL: (u32, u32) = (10, 20);

/// The pixel size of one cell as the framework last reported it, `None` where the terminal
/// reports none. The screen writes it each time it is drawn, the only place the framework's
/// `Env::cell_pixels` can be read; the event task compares it with the cell the page was laid
/// out for and says when it changed.
///
/// A change of font size alone keeps the columns and rows, so no resize reports it; the page is
/// laid out for the new cell all the same, else pictures are drawn at the wrong scale and clicks
/// land beside what was clicked. A framework hook for this change is requested (request 14 in
/// `browser-istekleri.md`); until it comes the event task looks between Chromium's events, at
/// least every [`CELL_LOOK`], and carries it.
#[derive(Debug, Clone, Default)]
pub(super) struct SeenCell(Arc<Mutex<Option<(u16, u16)>>>);

impl SeenCell {
    /// Records what the framework reports now.
    pub(super) fn set(&self, cell: Option<(u16, u16)>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = cell;
    }

    /// The cell in pixels the page is to be laid out for: the reported one, else [`FALLBACK_CELL`].
    pub(super) fn pixels(&self) -> (u32, u32) {
        let cell = *self.0.lock().unwrap_or_else(PoisonError::into_inner);
        cell.filter(|(width, height)| *width > 0 && *height > 0)
            .map_or(FALLBACK_CELL, |(width, height)| (u32::from(width), u32::from(height)))
    }
}

/// The viewport in CSS pixels for a page area of `area` cells, each `cell` pixels.
pub(super) fn viewport(area: Size, cell: (u32, u32)) -> (u32, u32) {
    (u32::from(area.width).max(1) * cell.0, u32::from(area.height).max(1) * cell.1)
}

/// The middle of cell (`column`, `row`) of the page area, in CSS pixels.
pub(super) fn cell_middle(column: u16, row: u16, cell: (u32, u32)) -> (f64, f64) {
    ((f64::from(column) + 0.5) * f64::from(cell.0), (f64::from(row) + 0.5) * f64::from(cell.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_is_as_many_pixels_as_its_cells_and_a_cell_is_hit_in_its_middle() {
        assert_eq!(viewport(Size::new(80, 22), (10, 20)), (800, 440));
        assert_eq!(viewport(Size::new(0, 0), (9, 18)), (9, 18), "never an empty viewport");
        assert_eq!(cell_middle(0, 0, (10, 20)), (5.0, 10.0));
        assert_eq!(cell_middle(12, 3, (8, 16)), (100.0, 56.0));
    }
}

//! Between the engine and the screen: starting Chromium off the drawing thread, the task that
//! carries its events to the screen, and the size of the page in pixels.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use qframe::geometry::Size;
use qframe::runtime::{Command, Task, TaskCx, TaskId};
use qframe::widgets::ImageData;

use super::{Machine, Msg};
use crate::engine::{Engine, Event, Options, StartError, TabId};

/// How often the event task looks for what Chromium sent: once a frame at sixty frames a second,
/// the most the screen draws.
///
/// A framework task can only wait by sleeping, and a sleep is what its cancel wakes; blocking on
/// the engine's channel would keep a cancelled task alive until Chromium spoke again.
const PACE: Duration = Duration::from_millis(16);

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
    };
    Command::perform(move || Msg::Started(Handover::new(Engine::start(&options))))
}

/// The result of [`start`], as the message carries it.
pub(super) type StartResult = Handover<Result<Started, StartError>>;

/// The task that carries the engine's events to the screen until the engine is gone or the task
/// is cancelled. Of the frames that came since it last looked only the newest of each tab is
/// decoded, here rather than on the drawing thread.
pub(super) fn pump(events: Receiver<Event>) -> (TaskId, Command<Msg>) {
    let task = Task::new("chromium", move |cx: &TaskCx<Msg>| {
        loop {
            let mut frames: HashMap<TabId, Vec<u8>> = HashMap::new();
            let ended = loop {
                match events.try_recv() {
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
            if !cx.sleep(PACE) {
                return Err("stopped".to_owned());
            }
        }
    });
    let id = task.id();
    (id, Command::task(task))
}

/// The pixel size of one cell: the one the machine fixes, else the terminal's.
pub(super) fn cell_pixels(machine: &Machine) -> (u32, u32) {
    machine.cell.unwrap_or_else(terminal_cell)
}

/// The pixel size of one of this terminal's cells: its window's pixels over its cells, or 10 × 20
/// when it reports no pixels.
///
/// A stopgap until the framework hands out the cell's pixel size itself (`Env::cell_pixels`,
/// framework request 1 in `browser-istekleri.md`); the framework asks the terminal already and
/// would answer the same everywhere.
fn terminal_cell() -> (u32, u32) {
    const FALLBACK: (u32, u32) = (10, 20);
    let Ok(size) = crossterm::terminal::window_size() else { return FALLBACK };
    if size.width == 0 || size.height == 0 || size.columns == 0 || size.rows == 0 {
        return FALLBACK;
    }
    let width = u32::from(size.width) / u32::from(size.columns);
    let height = u32::from(size.height) / u32::from(size.rows);
    if width == 0 || height == 0 { FALLBACK } else { (width, height) }
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

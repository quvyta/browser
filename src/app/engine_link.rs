//! Between the engine and the screen: starting Chromium off the drawing thread, the task that
//! carries its events to the screen, and the size of the page in pixels.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};

use qframe::geometry::Size;
use qframe::runtime::{Command, Task, TaskCx, TaskId};
use qframe::widgets::ImageData;

use super::{Machine, Msg};
use crate::engine::{Engine, Event, Options, StartError, TabId};

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
pub(super) fn pump(events: Receiver<Event>) -> (TaskId, Command<Msg>) {
    let task = Task::new("chromium", move |cx: &TaskCx<Msg>| {
        loop {
            // Nothing comes when the engine is gone or the task is cancelled.
            let Some(first) = cx.recv(&events) else {
                return Err(if cx.is_cancelled() { "stopped" } else { "the engine is gone" }.to_owned());
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

/// The cell in pixels the page is to be laid out for: the one the terminal reports, else
/// [`FALLBACK_CELL`].
pub(super) fn cell_pixels(reported: Option<(u16, u16)>) -> (u32, u32) {
    reported
        .filter(|(width, height)| *width > 0 && *height > 0)
        .map_or(FALLBACK_CELL, |(width, height)| (u32::from(width), u32::from(height)))
}

/// The viewport in CSS pixels for a page area of `area` cells, each `cell` pixels.
pub(super) fn viewport(area: Size, cell: (u32, u32)) -> (u32, u32) {
    (u32::from(area.width).max(1) * cell.0, u32::from(area.height).max(1) * cell.1)
}

/// The smallest picture of a page area of `area` cells, each `cell` pixels, that still gives every
/// half block a pixel of its own: the viewport's own shape shrunk until one side has just a pixel a
/// column or two a row. The framework draws a half block as a cell's width and half its height, so
/// a picture of the viewport's shape fills the area edge to edge; a picture of one pixel a column
/// and two a row would have that shape only for cells exactly twice as tall as wide.
pub(super) fn half_block_picture(area: Size, cell: (u32, u32)) -> (u32, u32) {
    let (columns, rows) = (u64::from(area.width.max(1)), u64::from(area.height.max(1)));
    let (cell_width, cell_height) = (u64::from(cell.0.max(1)), u64::from(cell.1.max(1)));
    let (wide, high) = (columns * cell_width, rows * cell_height);
    // Shrunk by 2 / cell_height every row has its two pixels, and by 1 / cell_width every column
    // its one; the larger shrink factor of the two meets both.
    let (width, height) = if 2 * cell_width >= cell_height {
        ((wide * 2).div_ceil(cell_height), rows * 2)
    } else {
        (columns, high.div_ceil(cell_width))
    };
    let fit = |value: u64, most: u64| u32::try_from(value.min(most)).unwrap_or(u32::MAX);
    (fit(width, wide), fit(height, high))
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

    #[test]
    fn a_half_block_picture_has_the_viewports_shape_and_a_pixel_for_every_half_block() {
        assert_eq!(half_block_picture(Size::new(80, 20), (10, 20)), (80, 40), "two pixels a cell for cells of 1 : 2");
        // A tall cell: every column has its pixel and the rows get more than two.
        assert_eq!(half_block_picture(Size::new(90, 20), (9, 20)), (90, 45));
        // A wide one: every row has its two and the columns get more than one.
        assert_eq!(half_block_picture(Size::new(80, 20), (12, 20)), (96, 40));
        assert_eq!(half_block_picture(Size::new(0, 0), (9, 20)), (1, 3), "never an empty picture");
    }

    #[test]
    fn a_cell_the_terminal_does_not_report_is_a_common_fonts() {
        assert_eq!(cell_pixels(Some((9, 19))), (9, 19));
        assert_eq!(cell_pixels(None), FALLBACK_CELL);
        assert_eq!(cell_pixels(Some((0, 19))), FALLBACK_CELL, "a cell of no width is no report");
    }
}

//! The size the page's writing is drawn at: the ladder of levels the person's own control walks,
//! the level each tab is at, and the toolbar button and its list that show and change it.
//!
//! The level is a number of per cent, the same in every language, and it is written nowhere: a
//! run starts every tab at [`HOME`] again. Remembering the level per site, as desktop browsers do,
//! is a piece of work of its own; this one does not carry it.

use std::cmp::Ordering;

use qframe::prelude::*;
use qframe::widgets::{IconButton, Popover};

use super::{Browser, Msg};

/// The levels the page is drawn at, as whole per cent. They are a desktop browser's own zoom
/// ladder — Firefox's, which runs from 30% to 200% and is the one the others are copies of — with
/// 100% on it, the size the page area already is. The two ends are where `In` and `Out` stop
/// rather than pass, and where the button that would go past them is disabled.
pub(super) const LADDER: [u32; 12] = [30, 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200];

/// The level a tab is at until the person says otherwise: 100%, the size of the area the page is
/// drawn in.
pub(super) const HOME: u32 = 100;

/// Which way along the ladder a message moves, and whether it comes back to where it started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Move {
    /// One step towards the writing being bigger.
    In,
    /// One step towards the writing being smaller.
    Out,
    /// Back to the level the page area already is.
    Reset,
}

/// The level one step `way` from `level`, or `None` where the ladder ends.
fn step(level: u32, way: Move) -> Option<u32> {
    let at = LADDER.iter().position(|&each| each == level)?;
    let next = match way {
        Move::In => at.checked_add(1)?,
        Move::Out => at.checked_sub(1)?,
        Move::Reset => return Some(HOME),
    };
    LADDER.get(next).copied()
}

/// The level as it is written on screen: a number and a sign, the same in every language.
fn shown(level: u32) -> String {
    format!("{level}%")
}

impl Browser {
    /// The level the tab on screen is drawn at.
    pub(super) fn level(&self) -> u32 {
        self.tab().level
    }

    /// Whether the ladder has a step `way` from the tab on screen's level.
    fn can(&self, way: Move) -> bool {
        step(self.level(), way).is_some()
    }

    /// Moves the tab on screen's level `way`, or leaves it where the ladder ends.
    pub(super) fn move_level(&mut self, way: Move) {
        let Some(level) = step(self.level(), way) else { return };
        // The button's list is a choice between levels, not a place to stay: the level it chose
        // is on the button now.
        self.zoom_menu = false;
        self.tabs[self.active].level = level;
        if let Some(id) = self.tab().id.clone() {
            self.size_tab(&id);
        }
    }

    /// The zoom button on the toolbar and the list it opens: the level in force, `In`, `Out`,
    /// `Reset` and the note that the level is not remembered.
    pub(super) fn zoom(&self, ui: &mut View<'_, Msg>) {
        let level = self.level();
        // The glyph says the level is not 100% and which way it went, so the change is not told by
        // colour alone; at 100% the button is at rest.
        let icon = match level.cmp(&HOME) {
            Ordering::Greater => "browser.zoom.larger",
            Ordering::Less => "browser.zoom.smaller",
            Ordering::Equal => "browser.zoom",
        };
        let can_in = self.can(Move::In);
        let can_out = self.can(Move::Out);
        let mark = ui.env().icons().glyph("dot").into_owned();
        Popover::new(self.zoom_menu)
            .on_dismiss(Msg::ZoomMenu(false))
            .anchor(|ui| {
                ui.add(
                    IconButton::new(icon)
                        .tooltip(t!("browser.zoom.level", n = level))
                        .on_press(Msg::ZoomMenu(!self.zoom_menu)),
                )
                .id("zoom");
            })
            .content(|ui| {
                ui.column(|ui| {
                    // The level in force, marked with a full stop before it rather than told by
                    // colour, so the list answers "how big is it now" on its own.
                    ui.add(Text::rich([Span::new(mark.clone()), Span::new(" "), Span::new(shown(level))]));
                    ui.add(
                        Button::new(t!("browser.zoom.in")).icon("stepper-plus").disabled(!can_in).on_press(Msg::ZoomIn),
                    )
                    .id("zoom-in");
                    ui.add(
                        Button::new(t!("browser.zoom.out"))
                            .icon("stepper-minus")
                            .disabled(!can_out)
                            .on_press(Msg::ZoomOut),
                    )
                    .id("zoom-out");
                    ui.add(
                        Button::new(t!("browser.zoom.reset"))
                            .icon("check")
                            .disabled(level == HOME)
                            .on_press(Msg::ZoomReset),
                    )
                    .id("zoom-reset");
                    ui.add(Text::new(t!("browser.zoom.forgotten")).role("faint"));
                })
                .width(Length::Cells(NOTE_CELLS));
            })
            .show(ui);
    }
}

/// The cells the list is as wide as, so the note under it wraps instead of making the list as wide
/// as the note.
const NOTE_CELLS: u16 = 24;

#[cfg(test)]
mod tests {
    use super::{HOME, LADDER, Move, step};

    /// The ladder is a ladder: sorted, with 100% on it, and every level reachable from 100% by
    /// walking one way and never past either end.
    #[test]
    fn the_ladder_is_walked_from_both_ends_and_stops_at_them() {
        assert!(LADDER.contains(&HOME), "100% is one of the levels: {LADDER:?}");
        assert!(LADDER.windows(2).all(|pair| pair[0] < pair[1]), "the levels climb: {LADDER:?}");
        assert_eq!(step(LADDER[0], Move::Out), None, "the bottom is an end, not a dead key");
        assert_eq!(step(*LADDER.last().unwrap(), Move::In), None, "and so is the top");
        assert_eq!(step(HOME, Move::In), Some(110));
        assert_eq!(step(110, Move::In), Some(125));
        assert_eq!(step(HOME, Move::Out), Some(90));
        assert_eq!(step(90, Move::Out), Some(80));
        assert_eq!(step(125, Move::Reset), Some(HOME), "reset comes back from anywhere");
        assert_eq!(step(30, Move::Reset), Some(HOME));
        // Every level is a level the person can arrive at, and the walk down reaches every one.
        let mut level = *LADDER.last().unwrap();
        let mut walked = vec![level];
        while let Some(next) = step(level, Move::Out) {
            level = next;
            walked.push(level);
        }
        walked.reverse();
        assert_eq!(walked, LADDER.to_vec(), "the ladder has no rung that cannot be reached");
    }
}

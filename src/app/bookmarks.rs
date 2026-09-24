//! Bookmarks on the screen: the star in the toolbar and ctrl+d that keep the page or let it go,
//! the bar of bookmarks below the toolbar with the ones that do not fit behind "More", and the
//! bookmarks the address bar suggests while the person types. The list on disk is
//! [`crate::bookmarks`]'s.
//!
//! The suggestions are qbrowser's own small layer, kept whole in this file: the framework's text
//! field has no list of suggestions yet. [`AddressField`] is the framework's `TextInput` with the
//! list's arrows in front of it, and the list is a framework `List` in a `Popover` under the
//! field.

use qframe::event::{Event, KeyKind, MouseButton, MouseKind};
use qframe::keymap::Key;
use qframe::prelude::*;
use qframe::text;
use qframe::widget::{EventCx, MeasureCx, PaintCx, Widget};
use qframe::widgets::{ContextItem, ContextMenu, IconButton, Popover, TextInput, Toast, Tooltip};

use super::{Browser, Msg, view};
use crate::bookmarks::{Bookmark, Bookmarks};

/// The widest a bookmark's name is on the bar, in cells; a longer one ends in `…`.
const NAME_CELLS: u16 = 24;

/// The toast of the bookmarks, so a quick second change replaces the first one's note.
const TOAST: &str = "bookmarks";

/// The bookmarks the address bar suggests for what was typed, and the one the arrows chose.
#[derive(Debug, Clone, Default)]
pub(super) struct Suggestions {
    items: Vec<Bookmark>,
    highlight: Option<usize>,
}

impl Suggestions {
    /// Whether the list shows.
    fn open(&self) -> bool {
        !self.items.is_empty()
    }
}

impl Browser {
    /// The rows of bookmarks between the toolbar and the page: one while there are any.
    pub(super) fn bookmark_rows(&self) -> u16 {
        u16::from(!self.bookmarks.is_empty())
    }

    /// Whether the page on screen is kept.
    fn bookmarked(&self) -> bool {
        self.bookmarks.contains(&self.tab().url)
    }

    /// Keeps the page on screen, or lets it go when it is kept already, and says which.
    pub(super) fn toggle_bookmark(&mut self) -> Command<Msg> {
        let tab = self.tab();
        let url = tab.url.clone();
        if tab.address().is_empty() || !Bookmark::storable(&url) {
            return Command::none();
        }
        if self.bookmarks.contains(&url) {
            return self.remove_bookmark(&url);
        }
        let bookmark = Bookmark::new(url, &tab.title);
        let toast = Toast::success(t!("browser.bookmarks.added")).body(bookmark.label().to_owned());
        self.change_bookmarks(|bookmarks| bookmarks.add(bookmark), toast)
    }

    /// Lets the bookmark of `url` go.
    pub(super) fn remove_bookmark(&mut self, url: &str) -> Command<Msg> {
        let Some(label) = self.bookmarks.all().iter().find(|b| b.url == url).map(|b| b.label().to_owned()) else {
            return Command::none();
        };
        let toast = Toast::info(t!("browser.bookmarks.removed")).body(label);
        self.change_bookmarks(|bookmarks| bookmarks.remove(url), toast)
    }

    /// Changes the list with `change` and writes it; says `done`, or that the file could not be
    /// written. The page area is laid out again when the bar came or went.
    fn change_bookmarks(&mut self, change: impl FnOnce(&mut Bookmarks) -> bool, done: Toast<Msg>) -> Command<Msg> {
        let rows = self.bookmark_rows();
        if !change(&mut self.bookmarks) {
            return Command::none();
        }
        if self.bookmark_rows() != rows {
            self.more_bookmarks = false;
            self.resized(self.size);
        }
        // A few lines, written at once rather than in the background: two quick changes then
        // land in the order they were made.
        let written = self.machine.bookmarks.as_deref().map_or(Ok(()), |file| self.bookmarks.write(file));
        let toast = match written {
            Ok(()) => done,
            Err(error) => Toast::warning(t!("browser.bookmarks.not-saved")).body(error.to_string()),
        };
        Command::toast(toast.key(TOAST))
    }

    /// Opens the bookmark of `url` in the tab on screen, or in a new tab.
    pub(super) fn open_bookmark(&mut self, url: &str, new_tab: bool) -> Command<Msg> {
        self.more_bookmarks = false;
        if new_tab { self.new_tab(url) } else { self.go(url) }
    }

    /// The suggestions for `typed`, the arrows' choice forgotten.
    pub(super) fn suggest(&mut self, typed: &str) {
        self.suggestions = Suggestions { items: self.bookmarks.matching(typed), highlight: None };
    }

    /// Closes the list of suggestions; the typed text stays.
    pub(super) fn close_suggestions(&mut self) {
        self.suggestions = Suggestions::default();
    }

    /// Moves the arrows' choice `step` rows, from nothing to the first or the last row and back
    /// to nothing past the ends, where Enter goes to the typed text again.
    pub(super) fn step_suggestion(&mut self, step: isize) {
        let count = self.suggestions.items.len();
        if count == 0 {
            return;
        }
        let rows = count.cast_signed() + 1;
        let at = self.suggestions.highlight.map_or(count, |row| row).cast_signed();
        let next = (at + step).rem_euclid(rows).cast_unsigned();
        self.suggestions.highlight = (next < count).then_some(next);
    }

    /// Where Enter in the address bar goes: the suggestion the arrows chose, else the typed text.
    pub(super) fn submit(&mut self, typed: &str) -> Command<Msg> {
        let chosen = self.suggestions.highlight.and_then(|row| self.suggestions.items.get(row)).map(|b| b.url.clone());
        self.go(chosen.as_deref().unwrap_or(typed))
    }

    /// Opens the suggestion on row `row`.
    pub(super) fn open_suggestion(&mut self, row: usize) -> Command<Msg> {
        let Some(url) = self.suggestions.items.get(row).map(|b| b.url.clone()) else { return Command::none() };
        self.go(&url)
    }

    /// The star: filled while the page is kept, open while it is not, and not there to press on an
    /// empty tab.
    pub(super) fn star(&self, ui: &mut View<'_, Msg>) {
        let kept = self.bookmarked();
        let (icon, tip) = if kept {
            ("browser.starred", t!("browser.bookmarks.remove"))
        } else {
            ("browser.star", t!("browser.bookmarks.add"))
        };
        ui.add(IconButton::new(icon).tooltip(tip).disabled(self.tab().address().is_empty()).on_press(Msg::Bookmark))
            .id("star");
    }

    /// The address field while the person types in it, with the bookmarks it suggests below it.
    pub(super) fn address_field(&self, ui: &mut View<'_, Msg>, text: &str) {
        let suggestions = &self.suggestions;
        let field = AddressField {
            input: TextInput::new(text)
                .placeholder(t!("browser.address.placeholder"))
                .select_all_on_focus()
                .on_change(Msg::LocationTyped)
                .on_submit(Msg::Go),
            open: suggestions.open(),
        };
        // As wide as the field: the toolbar's buttons, gaps and edges take the rest of the row, and
        // the popover's padding goes round the list.
        let padding = ui.env().theme().style("popover", None, &[]).pair("padding").map_or(0, |(_, sides)| sides * 2);
        let width = self.size.width.saturating_sub(view::TOOLBAR_CELLS + padding).max(20);
        Popover::new(suggestions.open())
            .on_dismiss(Msg::CloseSuggestions)
            .anchor(|ui| {
                ui.add(field).fill_width().id(view::LOCATION);
            })
            .content(|ui| {
                let rows = suggestions.items.iter().map(|b| ListItem::new(b.label()).detail(b.url.clone()));
                let count = u16::try_from(suggestions.items.len()).unwrap_or(u16::MAX);
                ui.add(List::new(rows).selected(suggestions.highlight).on_activate(Msg::OpenSuggestion))
                    .width(Length::Cells(width))
                    .height(Length::Cells(count))
                    .id("suggestions");
            })
            .show(ui)
            .fill_width();
    }

    /// The bar of bookmarks, while there are any: each one a button with its name, and "More"
    /// holding the ones that do not fit.
    pub(super) fn bookmarks_bar(&self, ui: &mut View<'_, Msg>) {
        if self.bookmarks.is_empty() {
            return;
        }
        let padding = ui.env().theme().style("button", None, &[]).pair("padding").map_or(2, |(_, sides)| sides);
        let button = |label: &str| text::width(label).saturating_add(padding.saturating_mul(2));
        let all = self.bookmarks.all();
        let names: Vec<String> = all.iter().map(|b| text::truncate(b.label(), NAME_CELLS).into_owned()).collect();
        let more = t!("browser.bookmarks.more");
        // The row's two edge cells, and a cell between the buttons.
        let room = self.size.width.saturating_sub(2);
        let fits = fitting(&names.iter().map(|name| button(name)).collect::<Vec<_>>(), button(&more), room);
        ui.row(|ui| {
            for (bookmark, name) in all.iter().zip(&names).take(fits) {
                bookmark_button(ui, bookmark, name.clone());
            }
            if fits < all.len() {
                ui.spacer();
                Popover::new(self.more_bookmarks)
                    .on_dismiss(Msg::MoreBookmarks(false))
                    .anchor(|ui| {
                        ui.add(Button::new(more).on_press(Msg::MoreBookmarks(!self.more_bookmarks)))
                            .id("more-bookmarks");
                    })
                    .content(|ui| {
                        ui.column(|ui| {
                            for (bookmark, name) in all.iter().zip(&names).skip(fits) {
                                bookmark_button(ui, bookmark, name.clone());
                            }
                        });
                    })
                    .show(ui);
            }
        })
        .gap(1)
        .padding(Padding::symmetric(0, 1))
        .height(Length::Cells(1))
        .fill_width()
        .id("bookmarks");
    }
}

/// How many of the buttons `widths` fit in `room` cells with a cell between them, leaving room
/// for the "More" button of width `more` and the gaps around the spacer before it when not all of
/// them fit.
fn fitting(widths: &[u16], more: u16, room: u16) -> usize {
    let used = |buttons: &[u16]| buttons.iter().map(|w| u32::from(*w) + 1).sum::<u32>();
    if used(widths) <= u32::from(room) + 1 {
        return widths.len();
    }
    // The spacer before "More" stands between two gaps.
    let room = u32::from(room).saturating_sub(u32::from(more) + 1);
    (0..widths.len()).rev().find(|n| used(&widths[..*n]) <= room).unwrap_or(0)
}

/// One bookmark as a button: a click opens it in the tab on screen, a middle click in a new tab,
/// and a right click offers both and taking it out. Its address shows under the pointer.
fn bookmark_button(ui: &mut View<'_, Msg>, bookmark: &Bookmark, name: String) {
    let url = bookmark.url.clone();
    let menu = [
        ContextItem::new(t!("browser.bookmarks.open-new-tab"), Msg::OpenBookmark(url.clone(), true)),
        ContextItem::gap(),
        ContextItem::new(t!("browser.bookmarks.delete"), Msg::RemoveBookmark(url.clone())),
    ];
    ui.add_with(Tooltip::new(url.clone()), |ui| {
        ui.add_with(ContextMenu::new(menu), |ui| {
            ui.add(BarButton {
                button: Button::new(name).on_press(Msg::OpenBookmark(url.clone(), false)),
                on_middle: Msg::OpenBookmark(url, true),
            });
        });
    });
}

/// The framework's `Button` that also answers a middle click, as a bookmark does by opening in a
/// new tab. Everything else is the button's own.
struct BarButton {
    button: Button<Msg>,
    on_middle: Msg,
}

impl Widget<Msg> for BarButton {
    fn measure(&self, cx: &mut MeasureCx<'_>, available: Size) -> Size {
        self.button.measure(cx, available)
    }

    fn paint(&self, cx: &mut PaintCx<'_>, area: Rect) {
        self.button.paint(cx, area);
    }

    fn event(&self, cx: &mut EventCx<'_, Msg>, event: &Event) -> bool {
        if let Event::Mouse(mouse) = event
            && mouse.kind == MouseKind::Down(MouseButton::Middle)
        {
            cx.emit(self.on_middle.clone());
            return true;
        }
        self.button.event(cx, event)
    }

    fn focusable(&self) -> bool {
        self.button.focusable()
    }
}

/// The framework's `TextInput` with the list of suggestions' arrows in front of it: while the list
/// is open ↑ and ↓ move through it. Every other key and all drawing are the field's own; Esc,
/// which the field leaves alone, reaches the popover around it, which closes the list and keeps
/// the typed text.
struct AddressField {
    input: TextInput<Msg>,
    open: bool,
}

impl Widget<Msg> for AddressField {
    fn measure(&self, cx: &mut MeasureCx<'_>, available: Size) -> Size {
        self.input.measure(cx, available)
    }

    fn paint(&self, cx: &mut PaintCx<'_>, area: Rect) {
        self.input.paint(cx, area);
    }

    fn paint_overlay(&self, cx: &mut PaintCx<'_>, anchor: Rect) {
        self.input.paint_overlay(cx, anchor);
    }

    fn event(&self, cx: &mut EventCx<'_, Msg>, event: &Event) -> bool {
        if self.open
            && let Event::Key(key) = event
            && key.kind != KeyKind::Release
        {
            let msg = if key.is_plain(Key::Down) {
                Some(Msg::Suggest(1))
            } else if key.is_plain(Key::Up) {
                Some(Msg::Suggest(-1))
            } else {
                None
            };
            if let Some(msg) = msg {
                cx.emit(msg);
                return true;
            }
        }
        self.input.event(cx, event)
    }

    fn focusable(&self) -> bool {
        self.input.focusable()
    }
}

#[cfg(test)]
mod tests {
    use super::fitting;

    #[test]
    fn buttons_fit_with_a_cell_between_them_and_more_takes_its_room_when_some_do_not() {
        assert_eq!(fitting(&[10, 10, 10], 6, 32), 3, "10 + 1 + 10 + 1 + 10 is 32");
        assert_eq!(fitting(&[10, 10, 10], 6, 31), 2, "two, a spacer and More: 10 + 1 + 10 + 1 + 0 + 1 + 6 is 29");
        assert_eq!(fitting(&[10, 10, 10], 6, 28), 1);
        assert_eq!(fitting(&[40], 6, 20), 0, "only More is left");
        assert_eq!(fitting(&[], 6, 20), 0);
    }
}

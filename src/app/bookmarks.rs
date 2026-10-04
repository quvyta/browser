//! Bookmarks on the screen: the star in the toolbar and ctrl+d that keep the page or let it go,
//! the bar of bookmarks below the toolbar with the ones that do not fit behind "More", and the
//! bookmarks the address bar suggests while the person types. The list on disk is
//! [`crate::bookmarks`]'s.
//!
//! The suggestions are the framework's own list under its text field: qbrow hands it the matching
//! bookmarks and hears which one was chosen.

use qframe::prelude::*;
use qframe::text;
use qframe::widgets::{ContextItem, ContextMenu, IconButton, Popover, Suggestion, TextInput, Toast, Tooltip};

use super::{Browser, Msg, view};
use crate::bookmarks::{Bookmark, Bookmarks};

/// The widest a bookmark's name is on the bar, in cells; a longer one ends in `…`.
const NAME_CELLS: u16 = 24;

/// The toast of the bookmarks, so a quick second change replaces the first one's note.
const TOAST: &str = "bookmarks";

/// The bookmarks the address bar suggests for what was typed.
#[derive(Debug, Clone, Default)]
pub(super) struct Suggestions {
    items: Vec<Bookmark>,
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
        self.change_bookmarks(|bookmarks| bookmarks.add(bookmark.clone()), toast)
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
    /// written. The change is made on the file as it is now, so what another qbrowser window kept
    /// meanwhile stays, and this window's list becomes that file's. The page area is laid out
    /// again when the bar came or went.
    fn change_bookmarks(&mut self, change: impl Fn(&mut Bookmarks) -> bool, done: Toast<Msg>) -> Command<Msg> {
        let rows = self.bookmark_rows();
        // A few lines, written at once rather than in the background: two quick changes then
        // land in the order they were made.
        let outcome = match self.machine.bookmarks.as_deref() {
            None => Ok(change(&mut self.bookmarks)),
            Some(file) => match Bookmarks::update(file, &change) {
                Ok((list, changed)) => {
                    self.bookmarks = list;
                    Ok(changed)
                }
                // The change still holds for this run, so the person sees it made.
                Err(error) => {
                    change(&mut self.bookmarks);
                    Err(error)
                }
            },
        };
        if self.bookmark_rows() != rows {
            self.more_bookmarks = false;
            self.resized(self.size);
        }
        let toast = match outcome {
            Ok(true) => done,
            // The file already was as asked, another window having done it: nothing to say.
            Ok(false) => return Command::none(),
            Err(error) => Toast::warning(t!("browser.bookmarks.not-saved")).body(error.to_string()),
        };
        Command::toast(toast.key(TOAST))
    }

    /// Opens the bookmark of `url` in the tab on screen, or in a new tab.
    pub(super) fn open_bookmark(&mut self, url: &str, new_tab: bool) -> Command<Msg> {
        self.more_bookmarks = false;
        if new_tab { self.new_tab(url) } else { self.go(url) }
    }

    /// The suggestions for `typed`.
    pub(super) fn suggest(&mut self, typed: &str) {
        self.suggestions = Suggestions { items: self.bookmarks.matching(typed) };
    }

    /// Closes the list of suggestions; the typed text stays.
    pub(super) fn close_suggestions(&mut self) {
        self.suggestions = Suggestions::default();
    }

    /// Where Enter in the address bar goes when no suggestion was chosen: the typed text. A chosen
    /// row comes as [`Msg::OpenSuggestion`] instead.
    pub(super) fn submit(&mut self, typed: &str) -> Command<Msg> {
        self.go(typed)
    }

    /// Opens the suggestion on row `row`.
    pub(super) fn open_suggestion(&mut self, row: usize) -> Command<Msg> {
        let Some(url) = self.suggestions.items.get(row).map(|b| b.url.clone()) else { return Command::none() };
        self.go(&url)
    }

    /// The star: filled and in the accent colour while the page is kept, so the state shows in its
    /// shape and its colour alike, open while it is not, and not there to press on an empty tab.
    pub(super) fn star(&self, ui: &mut View<'_, Msg>) {
        let kept = self.bookmarked();
        let (icon, tip) = if kept {
            ("browser.starred", t!("browser.bookmarks.remove"))
        } else {
            ("browser.star", t!("browser.bookmarks.add"))
        };
        ui.add(
            IconButton::new(icon)
                .tooltip(tip)
                .selected(kept)
                .disabled(self.tab().address().is_empty())
                .on_press(Msg::Bookmark),
        )
        .id("star");
    }

    /// The address field while the person types in it, with the bookmarks it suggests below it.
    pub(super) fn address_field(&self, ui: &mut View<'_, Msg>, text: &str) {
        let rows = self.suggestions.items.iter().map(|b| Suggestion::new(b.label()).detail(b.url.clone()));
        ui.add(
            TextInput::new(text)
                .placeholder(t!("browser.address.placeholder"))
                .select_all_on_focus()
                .on_change(Msg::LocationTyped)
                .on_submit(Msg::Go)
                .suggestions(rows)
                .on_suggestion(Msg::OpenSuggestion),
        )
        .fill_width()
        .id(view::LOCATION);
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
            ui.add(
                Button::new(name)
                    .on_press(Msg::OpenBookmark(url.clone(), false))
                    .on_middle_press(Msg::OpenBookmark(url, true)),
            );
        });
    });
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

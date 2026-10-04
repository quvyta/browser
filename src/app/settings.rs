//! qbrowser's settings: the search engine the address bar searches with and the start page a new
//! empty tab opens, both in `browser.conf`, and the page that shows them beside the framework's
//! shared look rows and the Quvyta-wide update notice.
//!
//! A value is written only while it differs from the default: a file of defaults would pin it, and
//! a later change of the default would never reach the person.

use qframe::prelude::*;
use qframe::storage::{Schema, Setting, SettingKind};
use qframe::widgets::{ScrollView, Select, SettingRow, SettingsList, Text as Words, TextInput};

use super::{Browser, Msg, view};
use crate::address::{Going, SearchEngine, going};

/// The key of the search engine: `duckduckgo`, `brave` or `mojeek`, of which only the last two are
/// ever written, since the first is the default.
pub(super) const SEARCH_ENGINE: &str = "search-engine";

/// The key of the start page: any text the address bar would take. Absent is the default, an
/// empty page with the keyboard in the address bar.
pub(super) const START_PAGE: &str = "start-page";

/// The widest the rows grow: beyond it a label and its control drift too far apart.
pub(super) const SECTION: u16 = 76;

/// The search engine picker is as wide as the framework's own choice rows, so the two columns of
/// controls line up.
const CHOICE: u16 = 18;

/// The start page field, wide enough for a common address.
const FIELD: u16 = 24;

/// Which of qbrowser's own rows a change or a failure belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// The search engine the address bar searches with.
    SearchEngine,
    /// The start page a new empty tab opens.
    StartPage,
}

/// What `browser.conf` may hold, so a file that cannot be read, a key this qbrowser no longer
/// knows and a value of the wrong shape each cost nothing: loading checks the file against this
/// and, with self-healing on, repairs it — an unknown key is dropped and an invalid value falls
/// back to the default. A key that is merely missing is never written into the file.
#[must_use]
pub(super) fn schema() -> Schema {
    Schema::builtin()
        .optional(SEARCH_ENGINE, SettingKind::choice(SearchEngine::ALL.map(SearchEngine::name)))
        .optional(START_PAGE, SettingKind::text())
}

/// What `page` is kept under: the text the address bar would take, or nothing when it leads
/// nowhere, so a value that cannot be used is not held in the file as if it could.
fn stored_page(page: &str, engine: SearchEngine) -> String {
    if going(page, engine).address().is_empty() { String::new() } else { page.trim().to_owned() }
}

/// The name the picker shows for `engine`, from the language files.
fn engine_name(engine: SearchEngine) -> String {
    t!(&format!("browser.search-engine.{}", engine.name()))
}

/// The address a brand-new empty tab opens for `page` searched with `engine`: where the same rule
/// the address bar follows leads, or [`super::tabs::BLANK`] when it leads nowhere, which is the
/// empty page with the keyboard in the address bar that qbrowser opened before there was a start
/// page.
pub(super) fn start_address(page: &str, engine: SearchEngine) -> String {
    let going = going(page, engine);
    if going.address().is_empty() { super::tabs::BLANK.to_owned() } else { going.address().to_owned() }
}

impl Browser {
    /// Keeps `value` under `key`, or nothing while it is `default`, and writes the file off the
    /// drawing thread.
    fn store<T: Setting + PartialEq>(&mut self, key: &str, value: T, default: &T, row: Row) -> Command<Msg> {
        let changed = if value == *default { self.settings.remove(key) } else { self.settings.set(key, value) };
        if !changed {
            return Command::none();
        }
        self.settings.save_command(move |result| Msg::Saved(row, result))
    }

    /// The settings screen, shown or taken back. The page area keeps its size and its picture
    /// either way, so the person never loses the tabs or the way back.
    pub(super) fn open_settings(&mut self, open: bool) -> Command<Msg> {
        self.settings_open = open;
        if !open {
            return Command::focus(view::PAGE);
        }
        // The screen takes the keyboard; the address bar gives it up, so the toolbar and the
        // settings list cannot both hold it.
        self.location = None;
        self.close_suggestions();
        Command::focus(view::SETTINGS_ROWS)
    }

    /// Searches for words with `engine` from now on.
    pub(super) fn set_search_engine(&mut self, engine: SearchEngine) -> Command<Msg> {
        self.search_engine = engine;
        self.not_saved = None;
        self.store(
            SEARCH_ENGINE,
            engine.name().to_owned(),
            &SearchEngine::DuckDuckGo.name().to_owned(),
            Row::SearchEngine,
        )
    }

    /// Opens `page` in every new empty tab from now on. A blank value, or one that leads nowhere,
    /// is the default: it is taken out of the file and the row says so instead of holding a value
    /// that does nothing.
    pub(super) fn set_start_page(&mut self, page: String) -> Command<Msg> {
        let stored = stored_page(&page, self.search_engine);
        self.start_page = page;
        self.not_saved = None;
        self.store(START_PAGE, stored, &String::new(), Row::StartPage)
    }

    /// What the row of `which` says when its last change could not be saved, in the framework's
    /// own words: it is the same sentence every Quvyta application uses.
    fn unsaved(&self, which: Row) -> Option<String> {
        self.not_saved
            .as_ref()
            .filter(|(failed, _)| *failed == which)
            .map(|(_, reason)| t!("quvyta.appearance.not-saved", reason = reason.as_str()))
    }

    /// What the start page row says under its label: what a new empty tab will really open, so a
    /// value that cannot be used never passes for one that can.
    fn start_page_note(&self) -> String {
        match going(&self.start_page, self.search_engine) {
            Going::Nothing => t!("browser.settings.start-page-blank"),
            Going::Address(address) => t!("browser.settings.start-page-opens", address = address),
            Going::Search(engine, _) => {
                t!("browser.settings.start-page-searches", engine = engine_name(engine))
            }
        }
    }

    /// A row of qbrowser's own, saying under its label why its last change could not be saved, or
    /// what it does.
    fn own_row(&self, which: Row, label: String, note: String) -> SettingRow<Msg> {
        let row = SettingRow::new(label);
        match self.unsaved(which) {
            Some(failure) => row.description(failure),
            None => row.description(note),
        }
    }

    /// The settings screen, in the place of the page.
    pub(super) fn settings_page(&self, ui: &mut View<'_, Msg>) {
        let page = |ui: &mut View<'_, Msg>| {
            ui.column(|ui| {
                ui.add(Words::new(t!("browser.settings.title")).role("title"));
                let rows = SettingsList::show(ui, |list| {
                    list.heading(t!("browser.settings.heading"));
                    let names: Vec<String> = SearchEngine::ALL.into_iter().map(engine_name).collect();
                    let chosen = SearchEngine::ALL.iter().position(|engine| *engine == self.search_engine);
                    let note = t!("browser.settings.search-engine-text");
                    let row = self.own_row(Row::SearchEngine, t!("browser.settings.search-engine"), note);
                    list.row(row, |ui| {
                        ui.add(Select::new(names).selected(chosen).on_select(|index| {
                            Msg::SearchEngine(SearchEngine::ALL.get(index).copied().unwrap_or(SearchEngine::DuckDuckGo))
                        }))
                        .width(Length::Cells(CHOICE));
                    });
                    let note = self.start_page_note();
                    let row = self.own_row(Row::StartPage, t!("browser.settings.start-page"), note);
                    let page = self.start_page.clone();
                    list.row(row, |ui| {
                        ui.add(
                            TextInput::new(page)
                                .placeholder(t!("browser.settings.start-page-placeholder"))
                                .on_change(Msg::StartPage)
                                .on_submit(Msg::StartPage),
                        )
                        .width(Length::Cells(FIELD))
                        .id(view::START_PAGE);
                    });
                    // The rows the whole ecosystem shares come from the framework, not from here:
                    // one copy of the language, theme, icon, motion and pillar gate is what keeps
                    // every Quvyta application changing together. The box that says "in every
                    // Quvyta application" belongs to them, so it is not added a second time here.
                    self.appearance.section(list, Msg::Appearance);
                    // The switch is Quvyta-wide; where qbrowser asks for its updates there is one
                    // to offer, and where it does not a switch there would do nothing.
                    if self.machine.updates.is_some() {
                        self.appearance.updates(list, Msg::Appearance);
                    }
                });
                rows.width(Length::Cells(SECTION)).id(view::SETTINGS_ROWS);
            })
            .padding(Padding { top: 1, right: 2, bottom: 1, left: 2 })
            .fill_width();
        };
        ui.add_with(ScrollView::new(), page).fill().id(view::SETTINGS);
    }
}

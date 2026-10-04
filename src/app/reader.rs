//! Reading mode: the page's own text in the terminal's own writing, in the place of the page, a
//! tab at a time.
//!
//! The page is read off the drawing thread ([`crate::engine::Engine::read_page`], run through a
//! [`crate::engine::Client`]) while the framework's delayed spinner turns in the page area, and
//! what came back is drawn with the terminal's own letters: no picture of the page, so the
//! writing is as large as the terminal draws it, on a terminal that draws no picture at all too.

use std::borrow::Cow;
use std::time::Duration;

use qframe::prelude::*;
use qframe::widgets::{Button, EmptyState, Markdown, Spinner, ToastKind};

use super::scroller::Scroller;
use super::{Browser, Msg, view};
use crate::engine::{Reading, TabId};

/// How long a page is given to hand over its words before qbrowser says it could not read them.
const WAIT: Duration = Duration::from_secs(10);

/// Main content shorter than this is no reading: a page of a few words is a form or a menu, and
/// what is on it would be noise rather than text.
const TOO_SHORT: usize = 200;

/// What the engine's script marks a picture's line with, when the picture has words of its own.
/// The word for a picture is the application's own, in the language on screen.
const PICTURE: &str = "[image: ";

/// The widest the reading's column grows, in cells: a page of prose is read as a column, and a
/// line wider than this is a line the eye loses its place in.
const WIDEST: u16 = 80;

/// What reading mode is doing in a tab.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) enum Mode {
    /// The page is drawn as it is; reading mode is closed.
    #[default]
    Closed,
    /// The page's words are on their way.
    Loading,
    /// The page's own words, as Markdown.
    Shown {
        /// The page's own title, shown above the words.
        title: String,
        /// The main content as Markdown.
        text: String,
    },
    /// The page has too little text to be read.
    Nothing,
    /// The page could not be read, and why.
    Failed(String),
}

impl Browser {
    /// Whether the tab on screen is being read instead of drawn.
    pub(super) fn reading(&self) -> bool {
        self.tab().reading != Mode::Closed
    }

    /// The name of the widget that holds the keyboard in the tab on screen: the page, or the
    /// reading where the page is being read as text.
    pub(super) fn page_focus(&self) -> &'static str {
        if self.reading() { view::READING } else { view::PAGE }
    }

    /// Reads the tab on screen as text, or puts its page back.
    ///
    /// The reading runs off the drawing thread: a page that takes its time to hand over its words
    /// does not hold up the screen, and the page area says it is waiting meanwhile.
    pub(super) fn show_reading(&mut self, open: bool) -> Command<Msg> {
        if !open {
            self.tabs[self.active].reading = Mode::Closed;
            return Command::focus(view::PAGE);
        }
        let Some(engine) = &self.engine else { return Command::none() };
        let Some(tab) = self.tab().id.clone() else { return Command::none() };
        self.tabs[self.active].reading = Mode::Loading;
        let client = engine.client();
        Command::batch([
            Command::focus(view::READING),
            Command::perform(move || Msg::Read { tab: tab.clone(), result: client.read_page(&tab, WAIT) }),
        ])
    }

    /// The words of the page that was read, or the reason there are none. A tab navigated
    /// elsewhere while its page was being read is left alone: the answer is to a question about a
    /// page that is no longer there.
    pub(super) fn read(&mut self, tab: &TabId, result: Result<Reading, String>) {
        let Some(index) = self.tabs.iter().position(|open| open.id.as_ref() == Some(tab)) else { return };
        if self.tabs[index].reading != Mode::Loading {
            return;
        }
        self.tabs[index].reading = match result {
            Ok(reading) if reading.text.trim().chars().count() < TOO_SHORT => Mode::Nothing,
            Ok(reading) => Mode::Shown { title: reading.title, text: worded(&reading.text) },
            Err(reason) => Mode::Failed(reason),
        };
    }

    /// The page area while the tab is read as text: the page's own words, or why there are none.
    pub(super) fn reading_page(&self, ui: &mut View<'_, Msg>, mode: &Mode) {
        match mode {
            Mode::Loading => {
                // The framework's delayed spinner, so a page that answers at once shows no sign
                // that it was ever read.
                ui.column(|ui| {
                    ui.add(Spinner::new().delayed(true));
                })
                .justify(Align::Center)
                .align(Align::Center)
                .fill()
                .id(view::READING);
            }
            Mode::Shown { title, text } => self.words(ui, title, text),
            Mode::Nothing => {
                ui.add(
                    EmptyState::new(t!("browser.reader.nothing"))
                        .message(t!("browser.reader.nothing-message"))
                        .action(Button::new(t!("browser.reader.back")).variant("primary").on_press(Msg::Reader(false))),
                )
                .fill();
            }
            Mode::Failed(reason) => {
                ui.add(
                    EmptyState::new(t!("browser.reader.failed"))
                        .tone(ToastKind::Danger)
                        .message(t!("browser.reader.reason", reason = reason.as_str()))
                        .action(Button::new(t!("browser.reader.back")).on_press(Msg::Reader(false))),
                )
                .fill();
            }
            Mode::Closed => {}
        }
    }

    /// The page's own words: its title above them, its main content as the framework's Markdown in
    /// a column centred on the page area, scrolled with the keys and with the wheel.
    fn words(&self, ui: &mut View<'_, Msg>, title: &str, text: &str) {
        let page = |ui: &mut View<'_, Msg>| {
            ui.column(|ui| {
                if !title.trim().is_empty() {
                    ui.add(Text::new(title.to_owned()).role("title"));
                }
                ui.add(Markdown::new(text));
            })
            .padding(Padding::symmetric(1, 1))
            .fill_width();
        };
        ui.column(|ui| {
            ui.add_with(Scroller::new(), page).width(Length::Cells(WIDEST));
        })
        .fill()
        .justify(Align::Center)
        .id(view::READING);
    }
}

/// The reading with a word for a picture in the language on screen, in place of the marker the
/// engine's script writes: the script cannot know what a person calls a picture.
fn worded(text: &str) -> String {
    text.lines().map(named).collect::<Vec<_>>().join("\n")
}

/// One line of a reading: the line of a picture, named as the language on screen names it.
fn named(line: &str) -> Cow<'_, str> {
    let Some(alt) = line.strip_prefix(PICTURE).and_then(|rest| rest.strip_suffix(']')) else {
        return Cow::Borrowed(line);
    };
    Cow::Owned(format!("{}: {alt}", t!("browser.reader.image")))
}

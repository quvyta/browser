//! History in two places, kept apart on purpose.
//!
//! The lists beside the arrows are the tab's own steps, read from the tab itself: what Chromium says
//! the tab walked through, never a record qbrowser made up. They last as long as the tab does.
//!
//! The screen behind ctrl+h is the other thing: the pages this profile has seen, kept in a file
//! beside the profile so that they are there tomorrow. One visit is one line — the address, a tab,
//! the title, a tab, and the time in seconds since the epoch — and the file is written whole after
//! every change through a temporary file and a rename, the way [`crate::bookmarks`] writes its own,
//! so a crash never leaves half a list. A line that is not a visit is skipped rather than refused:
//! one broken line must not cost the rest.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use qframe::date::{Date, DateTime, local_offset_minutes};
use qframe::event::{Event, KeyKind, MouseButton, MouseEvent, MouseKind};
use qframe::keymap::Key;
use qframe::prelude::*;
use qframe::runtime::{Command, Confirm};
use qframe::widget::{EventCx, MeasureCx, PaintCx, Widget};
use qframe::widgets::{IconButton, List, ListItem, Popover, Toast};

use super::{Browser, Msg};
use crate::bookmarks::Bookmark;
use crate::engine::{Entry, Profile};

/// The name of the history's file beside the profile.
pub(super) const FILE: &str = "history";

/// The toast of the history, so a quick second change replaces the first one's note.
const TOAST: &str = "history";

/// The most visits kept, the oldest going first. The whole file is written again after every
/// page, on the drawing thread; without an end it would grow with every day of use and the write
/// with it.
const MOST: usize = 5000;

/// The cells a list beside an arrow is as wide as, so an address can be read in it; a narrower
/// screen takes what it has and the row is cut by the screen, never by the list.
const LIST_CELLS: u16 = 44;

/// The most rows a list beside an arrow shows at once; a longer list scrolls.
const LIST_ROWS: u16 = 12;

/// The name of the list on the history screen, which takes the keyboard while the screen is on.
pub(super) const SCREEN: &str = "history";

/// Which arrow's list is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Arrow {
    /// Neither list is open.
    #[default]
    None,
    /// The steps back from where the tab is.
    Back,
    /// The steps forward from where the tab is.
    Forward,
}

impl Arrow {
    /// Whether the tab can go this way, which is the whole of what the button beside the arrow
    /// answers: nowhere to go, nowhere to show.
    fn possible(self, back: bool, forward: bool) -> bool {
        match self {
            Self::None => false,
            Self::Back => back,
            Self::Forward => forward,
        }
    }

    /// The name of the button beside the arrow, which is also what it is looked for by.
    fn id(self) -> &'static str {
        match self {
            Self::Back => "back-list",
            Self::Forward => "forward-list",
            Self::None => "steps",
        }
    }
}

/// The list beside the arrows: which one is open, and the row the arrows chose in it.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Steps {
    /// Which list is open.
    pub(super) which: Arrow,
    /// The row chosen, as the place in the list it stands for. Opening a list puts it on the
    /// nearest step, so Enter alone steps once, exactly as the arrow beside the list does.
    pub(super) chosen: Option<usize>,
}

/// One page this profile has seen, and when it was last seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Visit {
    /// The address, as Chromium reported it.
    url: String,
    /// The page's title then, which Chromium may have left empty.
    title: String,
    /// Seconds since the epoch, as the file holds it.
    seconds: u64,
}

impl Visit {
    /// What the screen calls the page: its title, or its address when it has none.
    fn label(&self) -> &str {
        if self.title.trim().is_empty() { &self.url } else { &self.title }
    }

    /// A title made to fit the line it is kept on: its tabs and line breaks become spaces, the same
    /// rule `crate::bookmarks` keeps a name by, so what is in the list reads back as it was written.
    fn one_line(title: &str) -> String {
        title.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().to_owned()
    }

    /// The visit a line of the file spells: the address, the title and the time, in that order.
    /// Whether it is one decides.
    fn parse(line: &str) -> Option<Self> {
        let mut fields = line.split('\t');
        let url = fields.next()?;
        let title = fields.next().unwrap_or_default();
        let seconds = fields.next()?.parse().ok()?;
        // An address that cannot be kept on a line is not a visit, and the line is skipped.
        Bookmark::storable(url).then(|| Self { url: url.to_owned(), title: Self::one_line(title), seconds })
    }
}

/// The pages this profile has seen, newest first, each address once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Visits {
    list: Vec<Visit>,
}

impl Visits {
    /// The list kept in `file`. A file that is not there is an empty list; so is one that cannot be
    /// read, since the browser must start either way and a broken file costs the person their
    /// history, not the application.
    pub(super) fn read(file: &Path) -> Self {
        std::fs::read(file).map(|bytes| Self::parse(&bytes)).unwrap_or_default()
    }

    /// The visits on the lines of `bytes`, skipping the lines that hold none and the addresses
    /// already read. The file is written newest first, so the first of a repeated address is the
    /// newest one and is the one kept.
    fn parse(bytes: &[u8]) -> Self {
        let mut visits = Self::default();
        let mut seen = std::collections::HashSet::new();
        for line in bytes.split(|byte| *byte == b'\n') {
            if visits.list.len() == MOST {
                break;
            }
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            // Bytes that are not text cost only themselves: the address before them may still be
            // good.
            if let Some(visit) = Visit::parse(&String::from_utf8_lossy(line))
                && seen.insert(visit.url.clone())
            {
                visits.list.push(visit);
            }
        }
        visits
    }

    /// Writes the list to `file`, one visit per line, making its folder when it is not there yet.
    ///
    /// # Errors
    ///
    /// Returns the error of making the folder or of writing the file.
    pub(super) fn write(&self, file: &Path) -> io::Result<()> {
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let mut text = String::new();
        for visit in &self.list {
            text.push_str(&visit.url);
            text.push('\t');
            text.push_str(&visit.title);
            text.push('\t');
            text.push_str(&visit.seconds.to_string());
            text.push('\n');
        }
        qframe::storage::atomic_write(file, text.as_bytes())
    }

    /// The visits, newest first.
    pub(super) fn all(&self) -> &[Visit] {
        &self.list
    }

    /// Records that `url` was seen at `seconds`; true when the list changed.
    ///
    /// The same address seen again is one entry with the newest visit first: this is a list of the
    /// pages a profile has seen, not a log of every click.
    pub(super) fn visit(&mut self, url: &str, title: &str, seconds: u64) -> bool {
        if !Bookmark::storable(url) {
            return false;
        }
        self.list.retain(|visit| visit.url != url);
        self.list.insert(0, Visit { url: url.to_owned(), title: Visit::one_line(title), seconds });
        self.list.truncate(MOST);
        true
    }

    /// Takes the visit of `url` out; true when there was one.
    pub(super) fn remove(&mut self, url: &str) -> bool {
        let before = self.list.len();
        self.list.retain(|visit| visit.url != url);
        self.list.len() != before
    }
}

/// Seconds since the epoch, from the system clock; a clock standing before the epoch reads as zero
/// rather than as a time that has not happened.
fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

/// The file the visits of `profile` are kept in: beside the profile folder Chromium runs on.
///
/// The persistent profile is `<profile_home>/profile`, so the file is `<profile_home>/history`; it
/// is beside the profile and not in the Quvyta data folder where the bookmarks are. A temporary
/// profile is a folder of its own under the temporary root, one per window, and it is removed when
/// qbrowser closes: a window that fell back to a temporary profile takes its history with it,
/// exactly as it takes its cookies and its sign-ins, and a second window never writes into the
/// first window's record. The file is never sent anywhere.
fn beside(profile: &Profile) -> PathBuf {
    if profile.temporary { profile.path.join(FILE) } else { profile.path.parent().unwrap_or(&profile.path).join(FILE) }
}

/// A day's name in the language's own words: today and yesterday where the language has words for
/// them, and the framework's own date for every other day.
fn day_words(day: Date) -> String {
    // A visit's day is never after today, so yesterday is one day before it.
    match day.to_days() - Date::today_local().to_days() {
        0 => t!("browser.history.today"),
        -1 => t!("browser.history.yesterday"),
        _ => day.written(),
    }
}

/// One row of the history screen: a day's heading, or a visit under it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    /// The heading of a day.
    Day(String),
    /// A visit: which one of the list, and how it is called and addressed.
    Visit(usize, String, String),
}

impl Browser {
    /// Takes the profile Chromium runs on and reads the visits of that profile; from here on they
    /// are written back to the file beside it.
    pub(super) fn open_history(&mut self, profile: &Profile) {
        let file = beside(profile);
        self.visits = Visits::read(&file);
        self.history_file = Some(file);
    }

    /// Records a visit to `url`, called `title`, at the time this runs. The title is Chromium's
    /// own, which may be empty; the screen falls back to the address rather than the engine filling
    /// one in. A file that cannot be written is said in a corner.
    pub(super) fn visit(&mut self, url: &str, title: &str) -> Option<Command<Msg>> {
        if !self.visits.visit(url, title, now()) {
            return None;
        }
        self.save_history()
    }

    /// Writes the visits out.
    fn save_history(&mut self) -> Option<Command<Msg>> {
        let file = self.history_file.as_deref()?;
        // A few lines, written at once rather than in the background: two quick changes then land
        // in the order they were made.
        self.visits.write(file).err().map(|error| {
            Command::toast(Toast::warning(t!("browser.history.not-saved")).body(error.to_string()).key(TOAST))
        })
    }

    /// The steps the list beside `which` holds, nearest first: the steps to that side of the one the
    /// tab is at, then the one it is at itself, drawn as the one it is rather than as a place to go.
    fn side(&self, which: Arrow) -> Vec<(usize, &Entry)> {
        let Some((current, entries)) = self.tab().history.as_ref() else { return Vec::new() };
        let steps: Vec<usize> = match which {
            Arrow::Back => (0..*current).rev().collect(),
            Arrow::Forward => (current + 1..entries.len()).collect(),
            Arrow::None => return Vec::new(),
        };
        let mut here: Vec<(usize, &Entry)> = steps.into_iter().map(|index| (index, &entries[index])).collect();
        if let Some(entry) = entries.get(*current) {
            here.push((*current, entry));
        }
        here
    }

    /// The button beside the arrow that shows this way's steps, and the list it opens.
    ///
    /// The button is disabled exactly when the arrow beside it is. A tab that is still opening has
    /// no list yet, so it says so by being disabled rather than by showing a list with nothing in
    /// it.
    pub(super) fn steps_button(&self, ui: &mut View<'_, Msg>, which: Arrow) {
        let open = self.steps.which == which;
        let (can_back, can_forward, chosen) = (self.tab().can_back, self.tab().can_forward, self.steps.chosen);
        let current = self.tab().history.as_ref().map_or(0, |(current, _)| *current);
        let listed = which.possible(can_back, can_forward) && self.tab().history.is_some();
        let rows = self.side(which);
        let count = u16::try_from(rows.len()).unwrap_or(LIST_ROWS).min(LIST_ROWS);
        let width = self.size.width.saturating_sub(2).min(LIST_CELLS);
        let tip = match which {
            Arrow::Back => t!("browser.history.back-list"),
            Arrow::Forward => t!("browser.history.forward-list"),
            Arrow::None => return,
        };
        let list = List::new(rows.into_iter().map(|(at, entry)| step_item(at, entry, current)))
            .selected(chosen)
            .on_select(Msg::FocusStep)
            .on_activate(Msg::OpenStep);
        Popover::new(open)
            .on_dismiss(Msg::Steps(Arrow::None))
            .focus_inside(open)
            .anchor(|ui| {
                ui.add(IconButton::new("chevron-down").tooltip(tip).disabled(!listed).on_press(Msg::Steps(if open {
                    Arrow::None
                } else {
                    which
                })))
                .id(which.id());
            })
            .content(|ui| {
                ui.add(StepList(list)).width(Length::Cells(width)).height(Length::Cells(count));
            })
            .show(ui);
    }

    /// Goes to the step on row `row` of the list beside `which`, in a new tab when a middle press
    /// asked for one.
    pub(super) fn open_step(&mut self, which: Arrow, row: usize, new_tab: bool) -> Command<Msg> {
        self.steps.which = Arrow::None;
        let Some((at, url)) = self.side(which).into_iter().nth(row).map(|(at, entry)| (at, entry.url.clone())) else {
            return Command::none();
        };
        // The step the tab is at is where the tab already is: choosing it closes the list and leaves
        // the page as it was, rather than loading the same address over itself.
        let Some(current) = self.tab().history.as_ref().map(|(current, _)| *current) else { return Command::none() };
        if current == at {
            return Command::none();
        }
        if new_tab {
            return self.new_tab(&url);
        }
        // Stepped to, as the arrow steps: going there as a new page would cut off every step past
        // it and add one more, and the list would no longer be the tab's own walk.
        let (Ok(at), Ok(current)) = (i64::try_from(at), i64::try_from(current)) else { return Command::none() };
        self.on_page(|engine, id| engine.step(id, at - current));
        Command::none()
    }

    /// Opens `url` in the tab on screen, or in a new one beside it.
    fn open_address(&mut self, url: &str, new_tab: bool) -> Command<Msg> {
        if new_tab { self.new_tab(url) } else { self.go(url) }
    }

    /// Opens the list beside `which`, or closes the one that is open when it is that one. Opening a
    /// list puts the arrows on the nearest step.
    pub(super) fn toggle_steps(&mut self, which: Arrow) {
        if which == Arrow::None || self.steps.which == which {
            self.steps = Steps::default();
        } else {
            self.steps = Steps { which, chosen: Some(0) };
        }
    }

    /// The arrows moved in the list beside an arrow, or a press chose a row in it.
    pub(super) fn focus_step(&mut self, row: usize) {
        self.steps.chosen = Some(row);
    }

    /// The history screen: what this profile has seen, newest first, under the day each visit fell
    /// on. With nothing seen it says so in the language's own words rather than showing an empty
    /// list with no explanation.
    pub(super) fn history_screen(&self, ui: &mut View<'_, Msg>) {
        let (rows, chosen) = (self.history_rows(), self.history_at);
        let list = List::new(rows.iter().map(row_item))
            .selected(chosen)
            .empty_text(t!("browser.history.empty"))
            .on_select(Msg::FocusVisit)
            .on_activate(Msg::OpenVisit);
        ui.column(|ui| {
            ui.add(Text::new(t!("browser.history.title"))).height(Length::Cells(1)).id("history-title");
            ui.add(VisitList { list, at: chosen }).fill_width().id(SCREEN);
        })
        .fill();
    }

    /// The rows of the history screen: a heading for each day the visits fall on and the visits
    /// under it, newest first. A day with nothing in it is not drawn, because a heading is only
    /// written for a day that has visits under it.
    fn history_rows(&self) -> Vec<Row> {
        let offset = local_offset_minutes();
        let mut rows = Vec::new();
        let mut day = None;
        for (at, visit) in self.visits.all().iter().enumerate() {
            let seconds = i64::try_from(visit.seconds).unwrap_or(i64::MAX);
            let date = DateTime::from_unix(seconds, offset).date.to_days();
            if day != Some(date) {
                day = Some(date);
                rows.push(Row::Day(day_words(Date::from_days(date))));
            }
            rows.push(Row::Visit(at, visit.label().to_owned(), visit.url.clone()));
        }
        rows
    }

    /// Which visit of the list is on row `row` of the history screen, the day's headings counted
    /// out.
    fn visit_at(&self, row: usize) -> Option<usize> {
        match self.history_rows().get(row) {
            Some(Row::Visit(at, ..)) => Some(*at),
            Some(Row::Day(_)) | None => None,
        }
    }

    /// Opens the visit on row `row` of the history screen, in a new tab when a middle press asked
    /// for one, and leaves the screen: the page comes back.
    pub(super) fn open_visit(&mut self, row: usize, new_tab: bool) -> Command<Msg> {
        self.history_open = false;
        let Some(url) = self.visit_at(row).and_then(|at| self.visits.all().get(at)).map(|visit| visit.url.clone())
        else {
            return Command::none();
        };
        self.open_address(&url, new_tab)
    }

    /// Asks whether the visit on row `row` may be taken out. A history nobody can take a page out
    /// of is not a history, it is a record held against the person.
    pub(super) fn forget_visit(&mut self, row: usize) -> Command<Msg> {
        let Some(at) = self.visit_at(row) else { return Command::none() };
        let Some(visit) = self.visits.all().get(at) else { return Command::none() };
        let question = Confirm::new(t!("browser.history.delete"), Msg::RemoveVisit(at))
            .message(t!("browser.history.delete-message", page = visit.label()))
            .confirm_label(t!("quvyta.file-manager.delete"))
            .danger();
        Command::confirm(question)
    }

    /// The question was answered yes: the visit goes out of the list and out of the file, and the
    /// rest of the list is left as it was.
    pub(super) fn remove_visit(&mut self, at: usize) -> Option<Command<Msg>> {
        let url = self.visits.all().get(at)?.url.clone();
        // The focus was on the visit taken out, and nothing stands there now: it is not handed to
        // the next visit, so a second Delete pressed out of habit takes out nothing it was not
        // asked about.
        self.history_at = None;
        if !self.visits.remove(&url) {
            return None;
        }
        self.save_history()
    }
}

/// One step of a tab's own history as a row: what the page was called and its address beside it. The
/// step the tab is at closes the list and is drawn as the one it is rather than as a place to go.
fn step_item(at: usize, entry: &Entry, current: usize) -> ListItem {
    let label = if entry.title.trim().is_empty() { entry.url.as_str() } else { entry.title.as_str() };
    let item = ListItem::new(label).detail(entry.url.clone());
    // Marked by a sign as well as drawn faint, so where the tab is does not rest on colour alone.
    if at == current { item.icon("dot", None).faint(true) } else { item }
}

/// One row of the history screen as a row of the list: a day's heading, or what the page was called
/// and its address beside it.
fn row_item(row: &Row) -> ListItem {
    match row {
        Row::Day(day) => ListItem::header(day.clone()),
        Row::Visit(_, label, url) => ListItem::new(label.clone()).detail(url.clone()),
    }
}

/// A middle press on a list row, which opens a row in a new tab everywhere else in qbrow and
/// nowhere in the framework.
///
/// A list answers a press with the row under the pointer and knows nothing else about the pointer,
/// so a middle press is offered to it as the press it does answer, and the message before it says
/// which kind of press this was; the message that follows opens the row and forgets the flag. This
/// is qbrowser's own small layer over that gap, in the same spirit as the address field's list of
/// suggestions, until the framework has a row of its own that opens in a new tab.
fn middle(cx: &mut EventCx<'_, Msg>, list: &List<Msg>, event: &Event) -> Option<bool> {
    let Event::Mouse(mouse) = event else { return None };
    let down = MouseKind::Down(MouseButton::Middle);
    (mouse.kind == down).then(|| {
        cx.emit(Msg::MiddleStep);
        list.event(cx, &Event::Mouse(MouseEvent { kind: MouseKind::Down(MouseButton::Left), ..*mouse }))
    })
}

/// The list beside an arrow, whose rows also answer a middle press.
struct StepList(List<Msg>);

impl Widget<Msg> for StepList {
    fn measure(&self, cx: &mut MeasureCx<'_>, available: Size) -> Size {
        self.0.measure(cx, available)
    }

    fn paint(&self, cx: &mut PaintCx<'_>, area: Rect) {
        self.0.paint(cx, area);
    }

    fn event(&self, cx: &mut EventCx<'_, Msg>, event: &Event) -> bool {
        middle(cx, &self.0, event).unwrap_or_else(|| self.0.event(cx, event))
    }

    fn focusable(&self) -> bool {
        self.0.focusable()
    }
}

/// The list of visits, which answers a middle press as the list beside an arrow does, and takes Esc
/// and Delete itself: the framework's list has neither a key for taking a row out nor any way of
/// knowing that the screen it is on should close.
struct VisitList {
    list: List<Msg>,
    /// The row the arrows are on, which the list itself keeps to itself.
    at: Option<usize>,
}

impl Widget<Msg> for VisitList {
    fn measure(&self, cx: &mut MeasureCx<'_>, available: Size) -> Size {
        self.list.measure(cx, available)
    }

    fn paint(&self, cx: &mut PaintCx<'_>, area: Rect) {
        self.list.paint(cx, area);
    }

    fn event(&self, cx: &mut EventCx<'_, Msg>, event: &Event) -> bool {
        if let Some(answered) = middle(cx, &self.list, event) {
            return answered;
        }
        if let Event::Key(key) = event
            && key.kind != KeyKind::Release
        {
            if key.is_plain(Key::Esc) {
                cx.emit(Msg::CloseHistory);
                return true;
            }
            if key.is_plain(Key::Delete)
                && let Some(row) = self.at
            {
                cx.emit(Msg::ForgetVisit(row));
                return true;
            }
        }
        self.list.event(cx, event)
    }

    fn focusable(&self) -> bool {
        self.list.focusable()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lines of `visits`, as the file holds them.
    fn lines(visits: &Visits) -> Vec<String> {
        visits.all().iter().map(|visit| format!("{}\t{}\t{}", visit.url, visit.title, visit.seconds)).collect()
    }

    #[test]
    fn today_and_yesterday_have_their_words_and_other_days_their_date() {
        let i18n = std::sync::Arc::new(crate::locales::i18n());
        qframe::i18n::scope(i18n, || {
            let today = Date::today_local();
            assert_eq!(day_words(today), t!("browser.history.today"));
            assert_eq!(day_words(today.add_days(-1)), t!("browser.history.yesterday"));
            let earlier = today.add_days(-2);
            assert_eq!(day_words(earlier), earlier.written());
        });
    }

    #[test]
    fn only_the_newest_visits_are_kept_past_the_most() {
        let mut visits = Visits::default();
        for n in 0..=MOST {
            visits.visit(&format!("http://{n}.example/"), "", n as u64);
        }
        assert_eq!(visits.all().len(), MOST);
        assert_eq!(visits.all()[0].url, format!("http://{MOST}.example/"), "the newest is first");
        assert!(!visits.all().iter().any(|visit| visit.url == "http://0.example/"), "the oldest went");
        let file: String = (0..=MOST).map(|n| format!("http://{n}.example/\t\t{n}\n")).collect();
        assert_eq!(Visits::parse(file.as_bytes()).all().len(), MOST, "a longer file is read only so far");
    }

    #[test]
    fn a_page_seen_again_is_one_visit_at_the_newest_time() {
        let mut visits = Visits::default();
        assert!(visits.visit("http://a.example/", "A", 100));
        assert!(visits.visit("http://b.example/", "B", 200));
        assert!(visits.visit("http://a.example/", "A again", 300));
        assert_eq!(
            lines(&visits),
            ["http://a.example/\tA again\t300", "http://b.example/\tB\t200"],
            "a list of pages, not a log of every click"
        );
        assert!(!visits.visit("not an address", "", 400), "what cannot be kept on a line is not a visit");
        assert!(visits.remove("http://a.example/"));
        assert_eq!(lines(&visits), ["http://b.example/\tB\t200"]);
        assert!(!visits.remove("http://a.example/"), "there is nothing there to take out");
    }

    #[test]
    fn a_written_list_reads_back_the_same_and_a_broken_line_costs_only_itself() {
        let folder = std::env::temp_dir().join(format!("qbrowser-history-{}", std::process::id()));
        let file = folder.join("deeper").join(FILE);
        let mut visits = Visits::default();
        visits.visit("https://a.example/", "Tabs\there,\nlines too", 1_700_000_000);
        visits.visit("http://127.0.0.1:8080/?q=1#top", "", 1_700_000_001);
        visits.write(&file).unwrap();
        let read = Visits::read(&file);
        std::fs::remove_dir_all(&folder).unwrap();
        assert_eq!(read, visits);
        let title_of = |visits: &Visits, url: &str| {
            visits.all().iter().find(|visit| visit.url == url).map(|visit| visit.label().to_owned())
        };
        assert_eq!(title_of(&read, "https://a.example/").as_deref(), Some("Tabs here, lines too"));
        assert_eq!(
            title_of(&read, "http://127.0.0.1:8080/?q=1#top").as_deref(),
            Some("http://127.0.0.1:8080/?q=1#top"),
            "a visit with no title is named by its address"
        );

        let broken = b"just words\nhttps://b.example/\tB\t17\n\tNameless\nhttps://c.example/\tC\n\xff\xfe\n\
                      https://d.example/\tD\tnot a time\nhttps://b.example/\tB again\t18\n\n";
        let read = Visits::parse(broken);
        assert_eq!(lines(&read), ["https://b.example/\tB\t17"], "the newest of a repeated address is kept once");
        assert_eq!(lines(&Visits::parse(b"")), Vec::<String>::new(), "an empty file is an empty history");
        assert_eq!(lines(&Visits::parse("\u{feff}not text at all".as_bytes())), Vec::<String>::new());
    }
}

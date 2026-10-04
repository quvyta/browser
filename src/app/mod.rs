//! The screen: the tab strip, the toolbar with the address, and the page Chromium draws below
//! them, or what stands in the page's place when Chromium is missing, failed or gone.
//!
//! Chromium runs in the engine ([`crate::engine`]); the screen starts it off the drawing thread,
//! a background task carries its events here, and every click, key and address goes back
//! through it.

use std::ffi::OsString;
use std::path::PathBuf;

use qframe::graphics::Graphics;
use qframe::prelude::*;
use qframe::runtime::{HandoffOutcome, TaskId, Termination, Update, UpdateCheck};
use qframe::storage::{Family, Preferences, Settings, data_dir};
use qframe::widgets::{Appearance, AppearanceChange, ImageData};

use crate::address::SearchEngine;
use crate::bookmarks::Bookmarks;
use crate::cli::Start;
use crate::engine::{Button, Engine, Event, Mouse, Reading, StartError, TabId, find_chromium};
use crate::page_view::PageInput;

mod bookmarks;
mod dialog;
mod engine_link;
mod history;
pub mod install;
mod menu;
mod reader;
mod scroller;
mod select;
mod session;
mod settings;
mod tabs;
mod view;
mod zoom;

pub use engine_link::Handover;
use engine_link::StartResult;
use install::InstallCommand;
use settings::{Row, SEARCH_ENGINE, START_PAGE};
use tabs::{BLANK, Tab};

/// qbrowser's name among the Quvyta apps: its settings file is `browser.conf` in the shared
/// Quvyta folder and its profile lives in the Quvyta state folder under `browser`.
pub const APP: &str = "browser";

/// What qbrowser knows about the machine it runs on: where Chromium, the profile, the settings and
/// the bookmarks are.
///
/// Everything the screen reads from the environment is here, so a test hands it a machine made
/// of temporary folders and nothing reaches the person's own profile, settings or desktop.
#[derive(Debug, Clone)]
pub struct Machine {
    /// The Chromium to run, from `QBROW_CHROMIUM`; `None` searches `path_var`.
    pub chromium: Option<PathBuf>,
    /// The program search path, for Chromium and for the package manager that installs it.
    pub path_var: Option<OsString>,
    /// The folder of the profile and its lock.
    pub profile_home: PathBuf,
    /// Where a temporary profile is made when the profile is in use by another qbrowser.
    pub temp_root: PathBuf,
    /// More command-line switches for Chromium; none on a real machine (see
    /// [`crate::engine::Options::extra_arguments`]).
    pub chromium_arguments: Vec<OsString>,
    /// The shared Quvyta folder, which holds `browser.conf` and the Quvyta-wide switches; `None`
    /// keeps every change in memory.
    pub config: Option<PathBuf>,
    /// Where the update notice is read and the last question remembered; `None` asks nothing.
    pub updates: Option<UpdateFolders>,
    /// The file the bookmarks are kept in, in the Quvyta data folder; `None` keeps them in memory.
    pub bookmarks: Option<PathBuf>,
}

impl Machine {
    /// This machine, as the environment describes it, with the profile in `profile` when the
    /// command line names one.
    #[must_use]
    pub fn here(profile: Option<PathBuf>) -> Self {
        let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
        let family = Family::QUVYTA;
        let temp_root = var("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
        // Without a home folder there is nowhere to keep a profile; one beside the temporary
        // ones still lets the browser work.
        let profile_home = profile
            .or_else(|| family.state_dir(APP))
            .unwrap_or_else(|| temp_root.join(format!("qbrowser-{}", std::process::id())));
        Self {
            chromium: var("QBROW_CHROMIUM").map(PathBuf::from),
            path_var: var("PATH"),
            profile_home,
            temp_root,
            chromium_arguments: Vec::new(),
            config: family.config_dir(),
            updates: UpdateFolders::here(),
            bookmarks: data_dir(family.id()).map(|folder| folder.join(APP).join(crate::bookmarks::FILE)),
        }
    }
}

/// Where the Quvyta-wide update notice is kept and where qbrowser remembers when it last asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateFolders {
    /// The shared Quvyta configuration folder, whose shared file holds the switch.
    pub config: PathBuf,
    /// qbrowser's state folder under the Quvyta one, which remembers when the question was last
    /// asked.
    pub state: PathBuf,
}

impl UpdateFolders {
    /// This machine's folders, or `None` without a home folder, where nothing could remember the
    /// switch or the last question and so nothing is asked.
    #[must_use]
    pub fn here() -> Option<Self> {
        let family = Family::QUVYTA;
        family.config_dir().zip(family.state_dir(APP)).map(|(config, state)| Self { config, state })
    }
}

/// Where Chromium is in its life.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    /// Being started.
    Starting,
    /// Running.
    Running,
    /// Not found on this machine.
    Missing(Missing),
    /// Found, but it did not start, and why.
    Failed(String),
    /// It ended without being asked to, and why.
    Gone(String),
}

/// What the Chromium-missing screen shows beyond its title.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Missing {
    /// The command that installs Chromium here, when qbrowser knows this system's package manager.
    command: Option<InstallCommand>,
    /// The command is shown as text.
    command_shown: bool,
    /// The install ran and Chromium is still not found.
    still_missing: bool,
}

/// The application's state.
pub struct Browser {
    machine: Machine,
    phase: Phase,
    engine: Option<Engine>,
    /// The task carrying the engine's events.
    pump: Option<TaskId>,
    /// Whether Chromium runs on a temporary profile because another qbrowser has the profile.
    temporary: bool,
    tabs: Vec<Tab>,
    active: usize,
    /// The text of the address bar while the person types in it.
    location: Option<String>,
    /// The screen's size, in cells.
    size: Size,
    graphics: Graphics,
    /// Whether the keyboard starts in the address bar rather than on the page.
    start_in_location: bool,
    /// The person's bookmarks.
    bookmarks: Bookmarks,
    /// Whether the bookmarks that do not fit on the bar are shown.
    more_bookmarks: bool,
    /// Whether the zoom button's list is open.
    zoom_menu: bool,
    /// The bookmarks the address bar suggests while the person types.
    suggestions: bookmarks::Suggestions,
    /// The pixel size of a cell as the screen last saw it.
    seen_cell: engine_link::SeenCell,
    /// The pixel size of a cell the pages are laid out for.
    cell: (u32, u32),
    /// Tabs closed while Chromium was still opening them: the next tabs Chromium reports as opened
    /// on qbrow's own request are closed at once instead of taking a place on the strip.
    closed_unopened: usize,
    /// Whether this window keeps the open tabs for the next start: it runs on the persistent
    /// profile.
    keeps_session: bool,
    /// `browser.conf`, qbrowser's own file: the shared look's keys and the two of its own.
    settings: Settings,
    /// The rows the whole ecosystem shares. It is held rather than built per frame, so what the
    /// person is doing on them — an open list, a reason a change was not saved — survives another
    /// application changing the shared file.
    appearance: Appearance,
    /// Which search engine the address bar searches words with.
    search_engine: SearchEngine,
    /// The address a new empty tab opens, as it was typed; empty is the default.
    start_page: String,
    /// Whether the settings screen is open in the place of the page.
    settings_open: bool,
    /// Whether the key overview is on screen.
    help_open: bool,
    /// Which of qbrowser's own rows a change could not be saved under, and why, until the next
    /// change.
    not_saved: Option<(Row, String)>,
    /// Whether the pages this profile has seen are on the screen.
    history_open: bool,
    /// The row the arrows are on in the history screen, its day's headings counted out.
    history_at: Option<usize>,
    /// The pages this profile has seen, newest first.
    visits: history::Visits,
    /// The file the visits are kept in, beside the profile; `None` keeps them in memory.
    history_file: Option<PathBuf>,
    /// The list beside an arrow, and the row the arrows chose in it.
    steps: history::Steps,
    /// Whether the press now opening a step or a visit is a middle one, which opens it in a new tab.
    /// The framework's list answers a press with the row under the pointer and nothing else about
    /// it, so the press before it says which kind of press this was and the one after opens the
    /// row.
    middle: bool,
    /// The address of the link under the pointer of the last right press, and the tab the page
    /// answered it for: another tab's answer is not this screen's, and the address is dropped
    /// when that tab navigates or goes.
    link: Option<(TabId, String)>,
    /// The list of a page's select that qbrow shows itself, while it is open.
    drop_down: Option<select::DropDown>,
    /// A left press on a select went to its list and not to the page, so the release that ends
    /// it is not sent either: the page never hears half a click.
    held_by_list: bool,
}

/// What the screen starts with: qbrowser itself, its settings file and the shared Quvyta look
/// the runtime opens in.
pub struct Opening {
    /// The screen.
    pub browser: Browser,
    /// `browser.conf`, for the runtime's saved look.
    pub settings: Settings,
    /// The shared Quvyta language, theme and icons, in force from the first frame.
    pub preferences: Preferences,
}

impl Opening {
    /// The screen on `machine`, opening `start`'s address.
    #[must_use]
    pub fn new(machine: Machine, start: &Start) -> Self {
        let family = Family::QUVYTA;
        let i18n = crate::locales::i18n();
        let (settings, preferences) = match &machine.config {
            Some(folder) => (
                Settings::open(folder.join(format!("{APP}.conf")))
                    .member_of(&family)
                    .schema(settings::schema())
                    .self_heal(true),
                family.preferences_in(folder, APP, &i18n),
            ),
            None => (Settings::in_memory(), family.preferences(APP, &i18n)),
        };
        let appearance = match &machine.config {
            Some(folder) => Appearance::new(family, APP, preferences.clone()).in_folder(folder),
            None => Appearance::new(family, APP, preferences.clone()).without_saving(),
        };
        let browser = Browser::new(machine, start, settings.clone(), appearance);
        Self { browser, settings, preferences }
    }
}

impl Browser {
    fn new(machine: Machine, start: &Start, settings: Settings, appearance: Appearance) -> Self {
        let search_engine = SearchEngine::named(&settings.get_or(SEARCH_ENGINE, String::new()));
        let start_page: String = settings.get_or(START_PAGE, String::new());
        // An address on the command line wins; without one the start page opens, and the empty
        // page with the keyboard in the address bar when there is no start page to open.
        let typed = start.address.clone();
        let first = typed.clone().unwrap_or_else(|| settings::start_address(&start_page, search_engine));
        let bookmarks = machine.bookmarks.as_deref().map(Bookmarks::read).unwrap_or_default();
        Self {
            machine,
            phase: Phase::Starting,
            engine: None,
            pump: None,
            temporary: false,
            tabs: vec![Tab::opening(&first)],
            active: 0,
            location: None,
            size: Size::new(0, 0),
            graphics: Graphics::HalfBlock,
            start_in_location: typed.is_none() && first == BLANK,
            bookmarks,
            more_bookmarks: false,
            zoom_menu: false,
            suggestions: bookmarks::Suggestions::default(),
            seen_cell: engine_link::SeenCell::default(),
            cell: engine_link::FALLBACK_CELL,
            keeps_session: false,
            closed_unopened: 0,
            settings,
            appearance,
            search_engine,
            start_page,
            settings_open: false,
            help_open: false,
            not_saved: None,
            history_open: false,
            history_at: None,
            visits: history::Visits::default(),
            history_file: None,
            steps: history::Steps::default(),
            middle: false,
            link: None,
            drop_down: None,
            held_by_list: false,
        }
    }

    /// The engine, while Chromium runs.
    #[cfg(test)]
    pub(crate) fn engine(&self) -> Option<&Engine> {
        self.engine.as_ref()
    }

    /// The Chromium tab on screen, once Chromium has opened it.
    #[cfg(test)]
    pub(crate) fn active_tab(&self) -> Option<&TabId> {
        self.tab().id.as_ref()
    }

    /// How many tabs are open.
    #[cfg(test)]
    pub(crate) fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// The address the address bar shows while nobody types in it.
    #[cfg(test)]
    pub(crate) fn address(&self) -> &str {
        self.tab().address()
    }

    /// The address the tab on screen was opened on, the blank page included, which is what a test
    /// asks for about the start page.
    #[cfg(test)]
    pub(crate) fn tab_url(&self) -> &str {
        &self.tab().url
    }

    /// The page area's size in cells: the screen below the tab strip, the toolbar and the bar of
    /// bookmarks.
    fn page_cells(&self) -> Size {
        let above = view::CHROME_ROWS + self.bookmark_rows();
        Size::new(self.size.width, self.size.height.saturating_sub(above))
    }

    /// The page area's size in CSS pixels.
    fn viewport(&self) -> (u32, u32) {
        engine_link::viewport(self.page_cells(), self.cell)
    }

    /// The largest picture of the page this screen can show: two pixels a cell where the
    /// terminal draws with half blocks or not at all, the page area's full pixels where it draws
    /// real pixels.
    fn picture_limit(&self) -> Option<(u32, u32)> {
        let cells = self.page_cells();
        match self.graphics {
            Graphics::Kitty | Graphics::Sixel => None,
            _ => Some((u32::from(cells.width), u32::from(cells.height) * 2)),
        }
    }

    /// Tells Chromium how large a picture is worth sending.
    fn limit_pictures(&self) {
        if let Some(engine) = &self.engine {
            engine.set_picture_limit(self.picture_limit());
        }
    }

    /// The question for a newer version of qbrowser, when the Quvyta-wide update notice is on. A
    /// machine where it is off asks nothing at all.
    fn ask_for_update(&self) -> Command<Msg> {
        let Some(folders) = &self.machine.updates else { return Command::none() };
        if !Family::QUVYTA.update_notice_in(&folders.config) {
            return Command::none();
        }
        let check =
            UpdateCheck::new(Family::QUVYTA, APP, env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"), Msg::NewVersion)
                .in_folders(folders.config.clone(), folders.state.clone());
        Command::check_for_update(check)
    }

    /// Chromium started, or why not.
    fn started(&mut self, result: &StartResult) -> Command<Msg> {
        let Some(result) = result.take() else { return Command::none() };
        match result {
            Ok((engine, events)) => {
                self.temporary = engine.profile().temporary;
                self.open_history(engine.profile());
                let restored = !self.temporary && self.restore_session();
                self.engine = Some(engine);
                self.phase = Phase::Running;
                self.limit_pictures();
                let (id, pump) = engine_link::pump(events, self.seen_cell.clone(), self.cell);
                self.pump = Some(id);
                for index in 0..self.tabs.len() {
                    self.ask_chromium(index);
                }
                if restored {
                    return Command::batch([pump, Command::focus(view::PAGE)]);
                }
                pump
            }
            Err(StartError::NotFound) => {
                let command = InstallCommand::find(self.machine.path_var.as_deref());
                self.phase = Phase::Missing(Missing { command, ..Missing::default() });
                Command::none()
            }
            Err(StartError::Failed(reason)) => {
                self.phase = Phase::Failed(reason);
                Command::none()
            }
        }
    }

    /// Ends Chromium and the task that listened to it.
    fn stop_engine(&mut self) -> Command<Msg> {
        if let Some(engine) = self.engine.take() {
            engine.shutdown();
        }
        self.pump.take().map_or_else(Command::none, Command::cancel_task)
    }

    /// Chromium ended by itself: its processes and a temporary profile are cleared away, and the
    /// screen says why.
    fn chromium_gone(&mut self, reason: String) -> Command<Msg> {
        let stop = self.stop_engine();
        self.phase = Phase::Gone(reason);
        stop
    }

    /// Starts Chromium again with the tabs that were open, each on the address it was at.
    fn restart(&mut self) -> Command<Msg> {
        let stop = self.stop_engine();
        self.tabs = self.tabs.iter().map(|tab| Tab::opening(&tab.url)).collect();
        self.closed_unopened = 0;
        self.phase = Phase::Starting;
        self.temporary = false;
        Command::batch([stop, engine_link::start(&self.machine)])
    }

    /// The install command ran: Chromium is looked for again and started when it is there.
    fn installed(&mut self) -> Command<Msg> {
        if find_chromium(self.machine.chromium.as_deref(), self.machine.path_var.as_deref()).is_some() {
            self.phase = Phase::Starting;
            return engine_link::start(&self.machine);
        }
        if let Phase::Missing(missing) = &mut self.phase {
            missing.still_missing = true;
        }
        Command::none()
    }

    /// The address bar becomes a field holding the address, the keyboard in it.
    fn open_location(&mut self) -> Command<Msg> {
        self.location = Some(self.tab().address().to_owned());
        self.close_suggestions();
        Command::focus(view::LOCATION)
    }

    /// The address bar goes back to showing the address, the keyboard back on the page.
    fn close_location(&mut self) -> Command<Msg> {
        self.location = None;
        self.close_suggestions();
        Command::focus(view::PAGE)
    }

    /// Goes where the address bar's text leads, in the active tab.
    fn go(&mut self, typed: &str) -> Command<Msg> {
        let destination = crate::address::destination(typed, self.search_engine);
        if destination.is_empty() {
            return Command::none();
        }
        let engine = self.engine.as_ref();
        let tab = &mut self.tabs[self.active];
        match (&tab.id, engine) {
            (Some(id), Some(engine)) => engine.navigate(id, &destination),
            // Chromium is still opening the tab: it goes there once it has.
            _ if tab.requested.is_some() => tab.wanted = Some(destination.clone()),
            // Chromium is still starting: the tab opens there.
            _ => {}
        }
        tab.url = destination;
        tab.title.clear();
        self.close_location()
    }

    /// Runs `act` with the engine and the active tab, when both are there.
    fn on_page(&self, act: impl FnOnce(&Engine, &TabId)) {
        if let (Some(engine), Some(id)) = (&self.engine, &self.tab().id) {
            act(engine, id);
        }
    }

    /// Hands the page what the person did on it.
    fn page_input(&mut self, input: PageInput) -> Command<Msg> {
        // A select's list is drawn by qbrow, since headless Chromium never draws it: a left press
        // or a list key on one opens it instead of reaching the page. Every other press goes on as
        // it came, in the order it came, with its own click count.
        match &input {
            PageInput::Mouse { mouse: Mouse::Pressed { button: Button::Left, .. }, column, row, .. } => {
                if let Some(opened) = self.open_drop_down(Some((*column, *row))) {
                    self.held_by_list = true;
                    return opened;
                }
            }
            PageInput::Mouse { mouse: Mouse::Moved { held: Some(Button::Left) }, .. } if self.held_by_list => {
                return Command::none();
            }
            PageInput::Mouse { mouse: Mouse::Released { button: Button::Left, .. }, .. } if self.held_by_list => {
                self.held_by_list = false;
                return Command::none();
            }
            PageInput::Key(press) if select::opens_list(press) => {
                if let Some(opened) = self.open_drop_down(None) {
                    return opened;
                }
            }
            _ => {}
        }
        let cell = self.cell;
        self.on_page(|engine, id| match input {
            PageInput::Mouse { mouse, column, row, modifiers } => {
                let (x, y) = engine_link::cell_middle(column, row, cell);
                engine.mouse(id, mouse, x, y, modifiers);
            }
            // Esc included: Chromium stops a loading page on it by itself, as desktop browsers do,
            // and the page hears it too.
            PageInput::Key(press) => engine.key(id, &press),
            PageInput::Paste(text) => engine.insert_text(id, &text),
        });
        Command::none()
    }

    /// The page area took a new size: every tab is laid out for it.
    fn resized(&mut self, size: Size) {
        self.size = size;
        self.limit_pictures();
        for id in self.tabs.iter().filter_map(|tab| tab.id.as_ref()) {
            self.size_tab(id);
        }
    }
}

/// Everything that can happen on this screen.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The screen has this many cells.
    Resized(Size),
    /// The terminal draws pictures this way.
    Graphics(Graphics),
    /// A cell is this many pixels now, wide and high.
    Cell((u32, u32)),
    /// Chromium started, or why not.
    Started(StartResult),
    /// Chromium reported something; frames come as [`Msg::Picture`].
    Engine(Event),
    /// A new picture of a tab's page, decoded.
    Picture(TabId, ImageData),
    /// The person did something on the page.
    Page(PageInput),
    /// A tab on the strip was clicked.
    SelectTab(usize),
    /// A tab's close mark was clicked.
    CloseTab(usize),
    /// Close the tab on screen.
    CloseActive,
    /// Open an empty tab.
    NewTab,
    /// Open the tab this many places along.
    StepTab(isize),
    /// Back through the tab's history.
    Back,
    /// Forward through the tab's history.
    Forward,
    /// Load the page again.
    Reload,
    /// Stop loading the page.
    Stop,
    /// The page's dialog was answered: OK (or "Leave") when true.
    DialogAnswer(bool),
    /// The text of a page's prompt changed.
    DialogTyped(String),
    /// Reading mode in the place of the page, or the page back.
    Reader(bool),
    /// A page was read, or why it could not be: its words, read off the drawing thread.
    Read {
        /// The tab whose page was read.
        tab: TabId,
        /// What the page had to say, or the reason there is no reading.
        result: Result<Reading, String>,
    },
    /// The address bar becomes a field, or goes back to the address.
    Location(bool),
    /// The text of the address field changed.
    LocationTyped(String),
    /// Enter in the address field.
    Go(String),
    /// Esc outside the page.
    Cancel,
    /// Keep the page on screen as a bookmark, or let it go when it is one.
    Bookmark,
    /// Open the bookmark of this address, in a new tab when true.
    OpenBookmark(String, bool),
    /// Take the bookmark of this address out.
    RemoveBookmark(String),
    /// Show or hide the bookmarks that do not fit on the bar.
    MoreBookmarks(bool),
    /// Open the address bar's suggestion on this row.
    OpenSuggestion(usize),
    /// Run the command that installs Chromium.
    Install,
    /// Show the command that installs Chromium.
    ShowCommand,
    /// The install command ended.
    Installed(HandoffOutcome),
    /// Start Chromium again.
    Restart,
    /// Draw the page one step larger.
    ZoomIn,
    /// Draw the page one step smaller.
    ZoomOut,
    /// Draw the page the size of the area it is drawn in.
    ZoomReset,
    /// Show or hide the zoom button's list.
    ZoomMenu(bool),
    /// The settings screen is shown, or taken back.
    Settings(bool),
    /// The key overview is shown, or taken back.
    Help(bool),
    /// A row of the framework's shared look was changed.
    Appearance(AppearanceChange),
    /// The search engine the address bar searches with was chosen.
    SearchEngine(SearchEngine),
    /// The start page a new empty tab opens was typed.
    StartPage(String),
    /// Another Quvyta application changed the shared look.
    Preferences(Preferences),
    /// A change of one of qbrowser's own rows was written, or could not be.
    Saved(Row, std::result::Result<(), String>),
    /// ctrl+h: show the pages this profile has seen, and close them again when they are on the
    /// screen.
    History,
    /// Esc: the history screen goes back to the page as it was.
    CloseHistory,
    /// The list beside an arrow opens, or the one that is open closes; the `None` side closes
    /// whichever is open.
    Steps(history::Arrow),
    /// The arrows moved in the list beside an arrow, or a press chose a row in it.
    FocusStep(usize),
    /// Enter or a press on a row of the list beside an arrow: go to that step of the tab's history.
    OpenStep(usize),
    /// A middle press, said before the row under it: a middle click opens in a new tab.
    MiddleStep,
    /// The history screen's focus moved to this row.
    FocusVisit(usize),
    /// Enter or a press on a row of the history screen: open the visit on that row.
    OpenVisit(usize),
    /// Delete on the history screen: ask whether the visit on that row may be taken out.
    ForgetVisit(usize),
    /// The question was answered: the visit is taken out of the list and out of the file.
    RemoveVisit(usize),
    /// Copy the page's selection as it would be pasted into a field.
    Copy,
    /// Copy the page's selection exactly as the page reported it.
    CopyRaw,
    /// Copy a link's address to the clipboard.
    CopyAddress(String),
    /// Open a link's address in a new tab.
    OpenLink(String),
    /// A right press on the page, on this cell of the page area.
    PageRight {
        /// The column, from the page area's left edge.
        column: u16,
        /// The row, from the page area's top edge.
        row: u16,
    },
    /// The arrows moved to this row of a select's list.
    HighlightOption(usize),
    /// This row of a select's list was chosen, by Enter or a click.
    ChooseOption(usize),
    /// A letter was typed in a select's list.
    JumpToOption(char),
    /// A select's list closes with nothing chosen: Esc or a press outside it.
    CloseOptions,
    /// A newer version of qbrowser is out.
    NewVersion(Update),
    /// qbrowser is ending: Chromium is ended first.
    Quit,
}

impl std::fmt::Debug for Browser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Browser")
            .field("phase", &self.phase)
            .field("tabs", &self.tabs.len())
            .field("active", &self.active)
            .finish_non_exhaustive()
    }
}

impl App for Browser {
    type Msg = Msg;

    fn init(&mut self) -> Command<Msg> {
        let focus = if self.start_in_location { self.open_location() } else { Command::focus(view::PAGE) };
        Command::batch([engine_link::start(&self.machine), focus, self.ask_for_update()])
    }

    fn resized(&self, size: Size) -> Option<Msg> {
        Some(Msg::Resized(size))
    }

    fn graphics(&self, graphics: Graphics) -> Option<Msg> {
        Some(Msg::Graphics(graphics))
    }

    fn update(&mut self, msg: Msg) -> Command<Msg> {
        match msg {
            Msg::Resized(size) => self.resized(size),
            Msg::Graphics(graphics) => {
                self.graphics = graphics;
                self.limit_pictures();
            }
            Msg::Cell(cell) => {
                self.cell = cell;
                self.resized(self.size);
            }
            Msg::Started(result) => return self.started(&result),
            Msg::Engine(event) => return self.engine_event(event),
            Msg::Picture(id, picture) => self.picture(&id, picture),
            Msg::Page(input) => return self.page_input(input),
            Msg::SelectTab(index) => return self.select_tab(index),
            Msg::CloseTab(index) => return self.close_tab(index),
            Msg::CloseActive => return self.close_tab(self.active),
            Msg::NewTab => return self.new_empty_tab(),
            Msg::StepTab(step) => return self.step_tab(step),
            Msg::Back => self.on_page(Engine::back),
            Msg::Forward => self.on_page(Engine::forward),
            Msg::Reload => {
                self.tabs[self.active].crashed = false;
                self.on_page(Engine::reload);
            }
            Msg::Stop => self.on_page(Engine::stop),
            Msg::DialogAnswer(accept) => self.answer_dialog(accept),
            Msg::DialogTyped(text) => self.dialog_typed(text),
            Msg::Reader(open) => return self.show_reading(open),
            Msg::Read { tab, result } => self.read(&tab, result),
            Msg::Location(true) => return self.open_location(),
            Msg::Location(false) | Msg::Cancel => {
                if self.location.is_some() {
                    return self.close_location();
                }
                // Reading mode holds the keyboard only while it is shown, so Esc from it puts the
                // page back rather than reaching the page underneath.
                if self.reading() {
                    return self.show_reading(false);
                }
            }
            Msg::LocationTyped(text) => {
                if self.location.is_some() {
                    self.suggest(&text);
                    self.location = Some(text);
                }
            }
            Msg::Go(text) => return self.submit(&text),
            Msg::Bookmark => return self.toggle_bookmark(),
            Msg::OpenBookmark(url, new_tab) => return self.open_bookmark(&url, new_tab),
            Msg::RemoveBookmark(url) => return self.remove_bookmark(&url),
            Msg::MoreBookmarks(open) => self.more_bookmarks = open,
            Msg::OpenSuggestion(row) => return self.open_suggestion(row),
            Msg::ZoomIn => self.move_level(zoom::Move::In),
            Msg::ZoomOut => self.move_level(zoom::Move::Out),
            Msg::ZoomReset => self.move_level(zoom::Move::Reset),
            Msg::ZoomMenu(open) => self.zoom_menu = open,
            // The settings screen stands in the place of the page, and the page keeps running
            // under it, so qbrowser's own keys go on working from there.
            Msg::Settings(open) => return self.open_settings(open),
            Msg::Help(open) => self.help_open = open,
            // The framework's rows write their own files, key by key.
            Msg::Appearance(change) => return self.appearance.update(change, &mut self.settings),
            Msg::SearchEngine(engine) => return self.set_search_engine(engine),
            Msg::StartPage(page) => return self.set_start_page(page),
            // Nothing is written and nothing is applied here: the runtime has already switched
            // the screen. What the person is doing on the rows stays as it is.
            Msg::Preferences(preferences) => self.appearance.refresh(preferences),
            Msg::Saved(row, Err(reason)) => self.not_saved = Some((row, reason)),
            Msg::Saved(_, Ok(())) => {}
            Msg::History => {
                self.history_open = !self.history_open;
                self.history_at = None;
                self.steps = history::Steps::default();
                self.middle = false;
                // The two screens stand in the same place; the one asked for is the one shown.
                self.settings_open = false;
                // The screen's list takes the keyboard while it is on, and the page has it back
                // after: while the page had it, no key of the list's would ever arrive.
                return Command::focus(if self.history_open { history::SCREEN } else { view::PAGE });
            }
            Msg::CloseHistory => {
                self.history_open = false;
                return Command::focus(view::PAGE);
            }
            Msg::Steps(which) => {
                // A middle press that landed on no row must not make the next click a new tab.
                self.middle = false;
                self.toggle_steps(which);
            }
            Msg::FocusStep(row) => self.focus_step(row),
            Msg::OpenStep(row) => {
                let (which, new_tab) = (self.steps.which, std::mem::take(&mut self.middle));
                return self.open_step(which, row, new_tab);
            }
            Msg::MiddleStep => self.middle = true,
            Msg::FocusVisit(row) => self.history_at = Some(row),
            Msg::OpenVisit(row) => {
                let new_tab = std::mem::take(&mut self.middle);
                return self.open_visit(row, new_tab);
            }
            Msg::ForgetVisit(row) => return self.forget_visit(row),
            Msg::RemoveVisit(at) => return self.remove_visit(at).unwrap_or_else(Command::none),
            Msg::Copy => return self.copy_selection(),
            Msg::CopyRaw => return self.copy_selection_raw(),
            Msg::CopyAddress(address) => return Command::copy(address),
            Msg::OpenLink(address) => return self.new_tab(&address),
            Msg::PageRight { column, row } => self.ask_about_link(column, row),
            Msg::HighlightOption(row) => self.highlight_option(row),
            Msg::ChooseOption(row) => return self.choose_option(row),
            Msg::JumpToOption(letter) => self.jump_to_option(letter),
            Msg::CloseOptions => return self.close_drop_down(),
            Msg::Install => {
                if let Phase::Missing(Missing { command: Some(command), .. }) = &self.phase {
                    return command.run();
                }
            }
            Msg::ShowCommand => {
                if let Phase::Missing(missing) = &mut self.phase {
                    missing.command_shown = true;
                }
            }
            Msg::Installed(_) => return self.installed(),
            Msg::Restart => return self.restart(),
            Msg::NewVersion(update) => return Command::toast(update.toast()),
            Msg::Quit => {
                self.save_session();
                return Command::batch([self.stop_engine(), Command::quit()]);
            }
        }
        Command::none()
    }

    fn view(&self, ui: &mut View<'_, Msg>) {
        self.screen(ui);
    }

    fn preferences(&self, preferences: &Preferences) -> Option<Msg> {
        Some(Msg::Preferences(preferences.clone()))
    }

    fn action(&self, name: &str) -> Option<Msg> {
        // The settings screen is openable while Chromium is missing, failed or gone as well: it is
        // then the only way to change how qbrowser looks and to stop it asking crates.io about a
        // newer version on a machine where the browser cannot start at all.
        // The key overview too: a person whose Chromium is missing still wants to know the keys.
        match name {
            "help" => return Some(Msg::Help(true)),
            "settings" => return Some(Msg::Settings(!self.settings_open)),
            "cancel" if self.settings_open => return Some(Msg::Settings(false)),
            _ => {}
        }
        if self.phase != Phase::Running && self.phase != Phase::Starting {
            return None;
        }
        let msg = match name {
            "new-tab" => Msg::NewTab,
            "close-tab" => Msg::CloseActive,
            "next-tab" => Msg::StepTab(1),
            "prev-tab" => Msg::StepTab(-1),
            "location" => Msg::Location(true),
            "back" => Msg::Back,
            "forward" => Msg::Forward,
            "reload" => Msg::Reload,
            "cancel" if self.history_open => Msg::CloseHistory,
            "cancel" => Msg::Cancel,
            "bookmark" => Msg::Bookmark,
            "reader" => Msg::Reader(!self.reading()),
            "zoom-in" => Msg::ZoomIn,
            "zoom-out" => Msg::ZoomOut,
            "zoom-reset" => Msg::ZoomReset,
            "history" => Msg::History,
            "copy" => Msg::Copy,
            _ => return None,
        };
        Some(msg)
    }

    fn before_quit(&self) -> Option<Msg> {
        Some(Msg::Quit)
    }

    fn terminating(&self, _cause: Termination) -> Option<Msg> {
        // Whatever ends qbrowser, Chromium ends with it, a hangup included.
        Some(Msg::Quit)
    }
}

#[cfg(test)]
mod tests;

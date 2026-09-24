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
use qframe::storage::{Family, Preferences, Settings};
use qframe::widgets::ImageData;

use crate::cli::Start;
use crate::engine::{Engine, Event, StartError, TabId, find_chromium};
use crate::page_view::PageInput;

mod engine_link;
pub mod install;
mod tabs;
mod view;

pub use engine_link::Handover;
use engine_link::StartResult;
use install::InstallCommand;
use tabs::{BLANK, Tab};

/// qbrowser's name among the Quvyta apps: its settings file is `browser.conf` in the shared
/// Quvyta folder and its profile lives in the Quvyta state folder under `browser`.
pub const APP: &str = "browser";

/// What qbrowser knows about the machine it runs on: where Chromium and the profile are, and the
/// size of a cell in pixels.
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
    /// The shared Quvyta folder, which holds `browser.conf` and the Quvyta-wide switches; `None`
    /// keeps every change in memory.
    pub config: Option<PathBuf>,
    /// Where the update notice is read and the last question remembered; `None` asks nothing.
    pub updates: Option<UpdateFolders>,
    /// The pixel size of a cell; `None` asks the terminal.
    pub cell: Option<(u32, u32)>,
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
            config: family.config_dir(),
            updates: UpdateFolders::here(),
            cell: None,
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
                Settings::open(folder.join(format!("{APP}.conf"))).member_of(&family),
                family.preferences_in(folder, APP, &i18n),
            ),
            None => (Settings::in_memory(), family.preferences(APP, &i18n)),
        };
        Self { browser: Browser::new(machine, start), settings, preferences }
    }
}

impl Browser {
    fn new(machine: Machine, start: &Start) -> Self {
        let first = start.address.as_deref().unwrap_or(BLANK);
        Self {
            machine,
            phase: Phase::Starting,
            engine: None,
            pump: None,
            temporary: false,
            tabs: vec![Tab::opening(first)],
            active: 0,
            location: None,
            size: Size::new(0, 0),
            graphics: Graphics::HalfBlock,
            start_in_location: start.address.is_none(),
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

    /// The page area's size in cells: the screen below the tab strip and the toolbar.
    fn page_cells(&self) -> Size {
        Size::new(self.size.width, self.size.height.saturating_sub(view::CHROME_ROWS))
    }

    /// The page area's size in CSS pixels.
    fn viewport(&self) -> (u32, u32) {
        engine_link::viewport(self.page_cells(), engine_link::cell_pixels(&self.machine))
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
                self.engine = Some(engine);
                self.phase = Phase::Running;
                let (id, pump) = engine_link::pump(events);
                self.pump = Some(id);
                for index in 0..self.tabs.len() {
                    self.ask_chromium(index);
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
        Command::focus(view::LOCATION)
    }

    /// The address bar goes back to showing the address, the keyboard back on the page.
    fn close_location(&mut self) -> Command<Msg> {
        self.location = None;
        Command::focus(view::PAGE)
    }

    /// Goes where the address bar's text leads, in the active tab.
    fn go(&mut self, typed: &str) -> Command<Msg> {
        let destination = crate::address::destination(typed);
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
    fn page_input(&self, input: PageInput) {
        let cell = engine_link::cell_pixels(&self.machine);
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
    }

    /// The page area took a new size: every tab is laid out for it.
    fn resized(&mut self, size: Size) {
        self.size = size;
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
    /// The address bar becomes a field, or goes back to the address.
    Location(bool),
    /// The text of the address field changed.
    LocationTyped(String),
    /// Enter in the address field.
    Go(String),
    /// Esc outside the page.
    Cancel,
    /// Run the command that installs Chromium.
    Install,
    /// Show the command that installs Chromium.
    ShowCommand,
    /// The install command ended.
    Installed(HandoffOutcome),
    /// Start Chromium again.
    Restart,
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
            Msg::Graphics(graphics) => self.graphics = graphics,
            Msg::Started(result) => return self.started(&result),
            Msg::Engine(event) => return self.engine_event(event),
            Msg::Picture(id, picture) => self.picture(&id, picture),
            Msg::Page(input) => self.page_input(input),
            Msg::SelectTab(index) => return self.select_tab(index),
            Msg::CloseTab(index) => return self.close_tab(index),
            Msg::CloseActive => return self.close_tab(self.active),
            Msg::NewTab => return self.new_tab(BLANK),
            Msg::StepTab(step) => return self.step_tab(step),
            Msg::Back => self.on_page(Engine::back),
            Msg::Forward => self.on_page(Engine::forward),
            Msg::Reload => {
                self.tabs[self.active].crashed = false;
                self.on_page(Engine::reload);
            }
            Msg::Stop => self.on_page(Engine::stop),
            Msg::Location(true) => return self.open_location(),
            Msg::Location(false) | Msg::Cancel => {
                if self.location.is_some() {
                    return self.close_location();
                }
            }
            Msg::LocationTyped(text) => {
                if let Some(location) = &mut self.location {
                    *location = text;
                }
            }
            Msg::Go(text) => return self.go(&text),
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
            Msg::Quit => return Command::batch([self.stop_engine(), Command::quit()]),
        }
        Command::none()
    }

    fn view(&self, ui: &mut View<'_, Msg>) {
        self.screen(ui);
    }

    fn action(&self, name: &str) -> Option<Msg> {
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
            "cancel" => Msg::Cancel,
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

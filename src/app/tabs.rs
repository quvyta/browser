//! The tabs as the screen keeps them: what each one shows, opening, closing and moving between
//! them, and what Chromium reports about them.

use qframe::runtime::Command;
use qframe::widgets::ImageData;

use super::{Browser, Msg, view};
use crate::engine::{Event, TabId};

/// The address an empty tab opens.
pub(super) const BLANK: &str = "about:blank";

/// One tab on the strip.
#[derive(Debug, Clone)]
pub(super) struct Tab {
    /// Chromium's tab, once it has opened it.
    pub(super) id: Option<TabId>,
    /// The address asked for before Chromium opened the tab; it is gone to once it has.
    pub(super) wanted: Option<String>,
    /// The address Chromium was asked to open the tab on, while it is being opened.
    pub(super) requested: Option<String>,
    pub(super) url: String,
    pub(super) title: String,
    pub(super) loading: bool,
    pub(super) can_back: bool,
    pub(super) can_forward: bool,
    pub(super) crashed: bool,
    /// The last picture of the page, kept while the tab is not shown so that coming back to it
    /// shows it at once.
    pub(super) picture: Option<ImageData>,
}

impl Tab {
    /// A tab Chromium is still to open, on `url`.
    pub(super) fn opening(url: &str) -> Self {
        Self {
            id: None,
            wanted: None,
            requested: None,
            url: url.to_owned(),
            title: String::new(),
            loading: false,
            can_back: false,
            can_forward: false,
            crashed: false,
            picture: None,
        }
    }

    /// A tab a page opened, which Chromium has already.
    fn opened(id: TabId, url: &str) -> Self {
        Self { id: Some(id), ..Self::opening(url) }
    }

    /// The address as the address bar shows it: nothing for an empty tab.
    pub(super) fn address(&self) -> &str {
        if self.url == BLANK { "" } else { &self.url }
    }

    /// What the strip calls the tab: its title, else its address, else `None` for "New tab".
    pub(super) fn label(&self) -> Option<&str> {
        [self.title.as_str(), self.address()].into_iter().find(|text| !text.trim().is_empty())
    }

    /// Whether the tab is busy: Chromium is opening it or its page is loading.
    pub(super) fn busy(&self) -> bool {
        self.id.is_none() || self.loading
    }
}

impl Browser {
    /// The tab on screen.
    pub(super) fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    fn position(&self, id: &TabId) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id.as_ref() == Some(id))
    }

    /// Opens a tab on `url` right of the active one and shows it; an empty one puts the keyboard
    /// in the address bar.
    pub(super) fn new_tab(&mut self, url: &str) -> Command<Msg> {
        let at = (self.active + 1).min(self.tabs.len());
        self.tabs.insert(at, Tab::opening(url));
        self.active = at;
        self.location = None;
        self.ask_chromium(at);
        if url == BLANK { self.open_location() } else { Command::focus(view::PAGE) }
    }

    /// Asks Chromium to open the tab at `index`, when it runs and has not been asked yet.
    pub(super) fn ask_chromium(&mut self, index: usize) {
        let Some(engine) = &self.engine else { return };
        let tab = &mut self.tabs[index];
        if tab.id.is_some() || tab.requested.is_some() {
            return;
        }
        engine.open_tab(&tab.url);
        tab.requested = Some(tab.url.clone());
    }

    /// Closes the tab at `index`. The last tab is never left closed: an empty one takes its place,
    /// since qbrowser ends only when asked to.
    pub(super) fn close_tab(&mut self, index: usize) -> Command<Msg> {
        if index >= self.tabs.len() {
            return Command::none();
        }
        let tab = self.tabs.remove(index);
        if let (Some(engine), Some(id)) = (&self.engine, &tab.id) {
            engine.close_tab(id);
        }
        self.after_removal(index)
    }

    /// Keeps the active tab sensible after the tab at `index` went: the one right of it, else the
    /// one left of it, else a new empty tab.
    fn after_removal(&mut self, index: usize) -> Command<Msg> {
        if self.tabs.is_empty() {
            self.active = 0;
            return self.new_tab(BLANK);
        }
        let was_active = index == self.active;
        if index < self.active || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        if was_active {
            self.location = None;
            self.show_active();
        }
        Command::none()
    }

    /// Opens the tab at `index`.
    pub(super) fn select_tab(&mut self, index: usize) -> Command<Msg> {
        if index >= self.tabs.len() {
            return Command::none();
        }
        if index != self.active {
            self.active = index;
            self.location = None;
            self.show_active();
        }
        Command::focus(view::PAGE)
    }

    /// Opens the tab `step` places along, going round at the ends.
    pub(super) fn step_tab(&mut self, step: isize) -> Command<Msg> {
        let count = self.tabs.len();
        let index = (self.active.cast_signed() + step).rem_euclid(count.cast_signed()).cast_unsigned();
        self.select_tab(index)
    }

    /// Has Chromium send the frames of the active tab.
    pub(super) fn show_active(&self) {
        if let (Some(engine), Some(id)) = (&self.engine, &self.tab().id) {
            engine.show(id);
        }
    }

    /// Gives the tab `id` the page area's size.
    pub(super) fn size_tab(&self, id: &TabId) {
        if let Some(engine) = &self.engine {
            let (width, height) = self.viewport();
            engine.set_viewport(id, width, height);
        }
    }

    /// Carries out what Chromium reported.
    pub(super) fn engine_event(&mut self, event: Event) -> Command<Msg> {
        // What an engine ended on purpose still said on its way out (its tabs closing) is left
        // unheard: those tabs are to come back on a restart, and on quitting nothing changes.
        if self.engine.is_none() {
            return Command::none();
        }
        match event {
            Event::TabOpened { tab, opener, url } => return self.tab_opened(&tab, opener.as_ref(), &url),
            Event::TabClosed { tab } => {
                if let Some(index) = self.position(&tab) {
                    self.tabs.remove(index);
                    return self.after_removal(index);
                }
            }
            // Frames arrive decoded, as `Msg::Picture`.
            Event::Frame { .. } => {}
            Event::Navigated { tab, url, can_back, can_forward } => {
                if let Some(index) = self.position(&tab) {
                    let state = &mut self.tabs[index];
                    state.url = url;
                    state.can_back = can_back;
                    state.can_forward = can_forward;
                    state.crashed = false;
                }
            }
            Event::Title { tab, title } => {
                if let Some(index) = self.position(&tab) {
                    self.tabs[index].title = title;
                }
            }
            Event::Loading { tab, loading } => {
                if let Some(index) = self.position(&tab) {
                    self.tabs[index].loading = loading;
                }
            }
            Event::Crashed { tab } => {
                if let Some(index) = self.position(&tab) {
                    self.tabs[index].crashed = true;
                    self.tabs[index].loading = false;
                }
            }
            Event::Gone { reason } => return self.chromium_gone(reason),
        }
        Command::none()
    }

    /// A tab opened: one qbrowser asked for takes its place on the strip, and one a page opened
    /// goes right of its opener and is shown.
    fn tab_opened(&mut self, id: &TabId, opener: Option<&TabId>, url: &str) -> Command<Msg> {
        let asked =
            opener.is_none().then(|| self.tabs.iter().position(|tab| tab.id.is_none() && tab.requested.is_some()));
        let index = match asked.flatten() {
            Some(index) => {
                let tab = &mut self.tabs[index];
                tab.id = Some(id.clone());
                let requested = tab.requested.take();
                // The person may have typed another address while the tab was being opened.
                if let (Some(wanted), Some(engine)) = (tab.wanted.take(), &self.engine)
                    && Some(&wanted) != requested.as_ref()
                {
                    engine.navigate(id, &wanted);
                    tab.url = wanted;
                }
                index
            }
            None => {
                let at = opener.and_then(|opener| self.position(opener)).map_or(self.tabs.len(), |index| index + 1);
                self.tabs.insert(at, Tab::opened(id.clone(), url));
                self.active = at;
                self.location = None;
                at
            }
        };
        self.size_tab(id);
        if index == self.active {
            self.show_active();
            if opener.is_some() {
                return Command::focus(view::PAGE);
            }
        }
        Command::none()
    }

    /// The newest picture of tab `id`.
    pub(super) fn picture(&mut self, id: &TabId, picture: ImageData) {
        if let Some(index) = self.position(id) {
            self.tabs[index].picture = Some(picture);
        }
    }
}

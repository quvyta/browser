//! The tabs that were open when qbrow last ended, opened again at the next start, as desktop
//! browsers do: ctrl+q with twenty tabs open no longer loses them.
//!
//! The list lives beside the profile (`<profile folder>/session`), one address per line, the
//! active tab's line marked with a leading `*`. Only the window that runs on the persistent
//! profile reads and writes it: a second window, on a temporary profile, neither takes the first
//! one's tabs nor writes over them when it ends. Written whole through a temporary file and a
//! rename, so an interrupted write leaves the last list whole.

use std::path::{Path, PathBuf};

use qframe::storage::atomic_write;

use super::tabs::{BLANK, Tab};
use super::{Browser, Machine};

/// The name of the list's file in the profile's folder.
const FILE: &str = "session";

/// The tabs of the last run: their addresses, and which one was on screen.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct Session {
    pub(super) addresses: Vec<String>,
    pub(super) active: usize,
}

impl Session {
    /// The list kept in `text`: empty lines and empty tabs left out, the marked line active.
    fn parse(text: &str) -> Self {
        let mut session = Self::default();
        for line in text.lines() {
            let (active, address) = line.strip_prefix('*').map_or((false, line), |rest| (true, rest));
            let address = address.trim();
            if address.is_empty() || address == BLANK {
                continue;
            }
            if active {
                session.active = session.addresses.len();
            }
            session.addresses.push(address.to_owned());
        }
        session
    }

    /// The file's text.
    fn text(&self) -> String {
        self.addresses
            .iter()
            .enumerate()
            .map(|(index, address)| if index == self.active { format!("*{address}\n") } else { format!("{address}\n") })
            .collect()
    }
}

/// Where `machine` keeps the list.
fn file(machine: &Machine) -> PathBuf {
    machine.profile_home.join(FILE)
}

/// The list at `path`; empty when there is none or it cannot be read.
fn read(path: &Path) -> Session {
    std::fs::read_to_string(path).map(|text| Session::parse(&text)).unwrap_or_default()
}

impl Browser {
    /// Chromium runs on the persistent profile: the tabs of the last run come back, before the
    /// ones this start asked for. An empty tab nobody has typed into yet gives its place to them;
    /// true when it did, and the keyboard then belongs on the page rather than in its address bar.
    pub(super) fn restore_session(&mut self) -> bool {
        // A restart after Chromium stopped keeps the tabs it has; they are not added twice.
        if self.keeps_session {
            return false;
        }
        self.keeps_session = true;
        let session = read(&file(&self.machine));
        if session.addresses.is_empty() {
            return false;
        }
        let untouched = self.tabs.len() == 1 && self.tabs[0].url == BLANK && self.tabs[0].id.is_none();
        let restored: Vec<Tab> = session.addresses.iter().map(|address| Tab::opening(address)).collect();
        let count = restored.len();
        if untouched {
            self.tabs = restored;
            self.active = session.active.min(count - 1);
            self.location = None;
        } else {
            // What this start opened stays on screen, right after the tabs that came back.
            self.tabs.splice(0..0, restored);
            self.active += count;
        }
        untouched
    }

    /// qbrow is ending: the open tabs are kept for the next start, when this window keeps them.
    pub(super) fn save_session(&self) {
        if !self.keeps_session {
            return;
        }
        let mut session = Session::default();
        for (index, tab) in self.tabs.iter().enumerate() {
            if tab.url == BLANK {
                continue;
            }
            if index == self.active {
                session.active = session.addresses.len();
            }
            session.addresses.push(tab.url.clone());
        }
        let path = file(&self.machine);
        if let Some(folder) = path.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        // Nothing can be said any more once qbrow is ending; the next start finds the old list.
        let _ = atomic_write(&path, session.text().as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_reads_back_with_its_active_tab_and_without_empty_ones() {
        let session = Session { addresses: vec!["https://a.example/".into(), "https://b.example/".into()], active: 1 };
        assert_eq!(Session::parse(&session.text()), session);
        let odd = Session::parse("\nabout:blank\nhttps://a.example/\n*https://c.example/\n  \n");
        assert_eq!(
            odd,
            Session { addresses: vec!["https://a.example/".into(), "https://c.example/".into()], active: 1 }
        );
    }
}

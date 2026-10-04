//! The person's bookmarks: pages they kept, in the order they kept them, and the file that holds
//! them in qbrowser's data folder.
//!
//! The list is the person's record, not a setting, so it lives in the Quvyta data folder
//! (`~/.local/share/quvyta/browser/bookmarks`) rather than next to `browser.conf`. One bookmark is
//! one line: the address, a tab, and the name. An address never holds a tab or a line break, and
//! a name has them turned into spaces when it is kept, so every line reads back as it was written.
//! The file is read at start. Every change reads it again under a lock, makes the change on what
//! it found and writes it whole through a temporary file and a rename, so a crash never leaves
//! half a list and a second qbrowser window's bookmarks are never written over by the first's
//! older copy. A line that is not a bookmark is skipped rather
//! than refused: one broken line must not cost the rest.

use std::io;
use std::path::Path;

use qframe::storage::atomic_write;

/// The name of the list's file in qbrowser's data folder.
pub const FILE: &str = "bookmarks";

/// How many bookmarks the address bar suggests at most.
pub const SUGGESTIONS: usize = 8;

/// One kept page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    /// The page's address, as Chromium reported it.
    pub url: String,
    /// The page's title when it was kept; may be empty.
    pub name: String,
}

impl Bookmark {
    /// The bookmark of `url` named `name`, with the name's tabs and line breaks turned into
    /// spaces so it fits on its line.
    #[must_use]
    pub fn new(url: impl Into<String>, name: &str) -> Self {
        let name = name.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>();
        Self { url: url.into(), name: name.trim().to_owned() }
    }

    /// What the bookmark is called on screen: its name, or its address when it has none.
    #[must_use]
    pub fn label(&self) -> &str {
        if self.name.is_empty() { &self.url } else { &self.name }
    }

    /// Whether `url` can be kept on a line: a real address has no whitespace or control
    /// characters and starts with a scheme.
    #[must_use]
    pub fn storable(url: &str) -> bool {
        let scheme = url.split_once(':').map(|(scheme, _)| scheme);
        scheme.is_some_and(|scheme| {
            !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
        }) && !url.chars().any(|c| c.is_whitespace() || c.is_control())
    }

    /// The bookmark a line of the file spells: the address before the first tab, the name after
    /// it. Whether it is one [`Bookmarks::add`] decides.
    fn parse(line: &str) -> Self {
        let (url, name) = line.split_once('\t').unwrap_or((line, ""));
        Self::new(url, name)
    }
}

/// The bookmarks in the order they were added, each address once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bookmarks {
    list: Vec<Bookmark>,
}

impl Bookmarks {
    /// The list kept in `file`. A file that is not there is an empty list; so is one that cannot
    /// be read, since the browser must start either way.
    #[must_use]
    pub fn read(file: &Path) -> Self {
        std::fs::read(file).map(|bytes| Self::parse(&bytes)).unwrap_or_default()
    }

    /// The bookmarks on the lines of `bytes`, skipping the lines that hold none and the addresses
    /// already read.
    fn parse(bytes: &[u8]) -> Self {
        let mut bookmarks = Self::default();
        for line in bytes.split(|byte| *byte == b'\n') {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            // Bytes that are not text cost only themselves: the address before them may still be
            // good.
            bookmarks.add(Bookmark::parse(&String::from_utf8_lossy(line)));
        }
        bookmarks
    }

    /// Writes the list to `file`, one bookmark per line, making its folder when it is not there
    /// yet.
    ///
    /// # Errors
    ///
    /// Returns the error of making the folder or of writing the file.
    pub fn write(&self, file: &Path) -> io::Result<()> {
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let mut text = String::new();
        for bookmark in &self.list {
            text.push_str(&bookmark.url);
            text.push('\t');
            text.push_str(&bookmark.name);
            text.push('\n');
        }
        atomic_write(file, text.as_bytes())
    }

    /// Makes `change` on the list kept in `file` as it is on disk now, not as this copy last
    /// saw it, and writes the result back; returns the list as it now stands and whether
    /// `change` changed anything. Another qbrowser window changes the same file: reading it again
    /// under the lock `<file>.lock` means its bookmarks are kept, and the two windows never write
    /// at the same time.
    ///
    /// # Errors
    ///
    /// Returns the error of making the folder, taking the lock or writing the file. The file is
    /// left as it was.
    pub fn update(file: &Path, change: impl FnOnce(&mut Self) -> bool) -> io::Result<(Self, bool)> {
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let mut lock_name = file.as_os_str().to_owned();
        lock_name.push(".lock");
        let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(lock_name)?;
        // Held until `lock` is dropped at the end, released by the system if this process dies.
        lock.lock()?;
        let mut list = Self::read(file);
        let changed = change(&mut list);
        if changed {
            list.write(file)?;
        }
        Ok((list, changed))
    }

    /// The bookmarks, in their order.
    #[must_use]
    pub fn all(&self) -> &[Bookmark] {
        &self.list
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Whether `url` is kept.
    #[must_use]
    pub fn contains(&self, url: &str) -> bool {
        self.list.iter().any(|bookmark| bookmark.url == url)
    }

    /// Adds `bookmark` at the end, unless its address is kept already or cannot be kept; true
    /// when it was added.
    pub fn add(&mut self, bookmark: Bookmark) -> bool {
        if !Bookmark::storable(&bookmark.url) || self.contains(&bookmark.url) {
            return false;
        }
        self.list.push(bookmark);
        true
    }

    /// Takes the bookmark of `url` out; true when there was one.
    pub fn remove(&mut self, url: &str) -> bool {
        let before = self.list.len();
        self.list.retain(|bookmark| bookmark.url != url);
        self.list.len() != before
    }

    /// The bookmarks whose name or address holds `typed`, whatever its case, in their order and
    /// at most [`SUGGESTIONS`]; none for blank text.
    #[must_use]
    pub fn matching(&self, typed: &str) -> Vec<Bookmark> {
        let typed = typed.trim().to_lowercase();
        if typed.is_empty() {
            return Vec::new();
        }
        let holds = |text: &str| text.to_lowercase().contains(&typed);
        self.list
            .iter()
            .filter(|bookmark| holds(&bookmark.name) || holds(&bookmark.url))
            .take(SUGGESTIONS)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(bookmarks: &Bookmarks) -> Vec<&str> {
        bookmarks.all().iter().map(|bookmark| bookmark.url.as_str()).collect()
    }

    #[test]
    fn broken_lines_and_repeats_are_skipped_and_the_rest_kept_in_order() {
        let bytes = b"https://a.example/\tA\n\nnot an address\tB\nhttps://b.example/ x\tC\n\xff\xfe\nhttps://d.example/\tbad \xff\n\
                      https://c.example/\r\nhttps://a.example/\tA again\n:nothing\tD\n";
        let bookmarks = Bookmarks::parse(bytes);
        assert_eq!(urls(&bookmarks), ["https://a.example/", "https://d.example/", "https://c.example/"]);
        assert_eq!(bookmarks.all()[0].name, "A", "the first of a repeated address is kept");
        assert_eq!(bookmarks.all()[1].name, "bad \u{fffd}", "a byte that is not text costs only itself");
        assert_eq!(bookmarks.all()[2].label(), "https://c.example/", "a line without a name is named by its address");
    }

    #[test]
    fn a_written_list_reads_back_the_same_with_names_made_to_fit_their_line() {
        let folder = std::env::temp_dir().join(format!("qbrowser-bookmarks-{}", std::process::id()));
        let file = folder.join("deeper").join(FILE);
        let mut bookmarks = Bookmarks::default();
        assert!(bookmarks.add(Bookmark::new("https://a.example/", "Tabs\there,\nlines too")));
        assert!(bookmarks.add(Bookmark::new("http://127.0.0.1:8080/?q=1#top", "")));
        bookmarks.write(&file).unwrap();
        let read = Bookmarks::read(&file);
        std::fs::remove_dir_all(&folder).unwrap();
        assert_eq!(read, bookmarks);
        assert_eq!(read.all()[0].name, "Tabs here, lines too");
    }

    #[test]
    fn two_windows_that_each_keep_a_page_keep_both() {
        let folder = std::env::temp_dir().join(format!("qbrowser-bookmarks-two-{}", std::process::id()));
        let file = folder.join(FILE);
        let (mut first, _) =
            Bookmarks::update(&file, |list| list.add(Bookmark::new("https://one.example/", "One"))).unwrap();
        // The second window started while the file held one bookmark, and keeps a page.
        let (second, added) =
            Bookmarks::update(&file, |list| list.add(Bookmark::new("https://two.example/", "Two"))).unwrap();
        assert!(added);
        assert_eq!(urls(&second), ["https://one.example/", "https://two.example/"]);
        // The first window still holds its older copy when it keeps a third page.
        assert_eq!(urls(&first), ["https://one.example/"]);
        (first, _) =
            Bookmarks::update(&file, |list| list.add(Bookmark::new("https://three.example/", "Three"))).unwrap();
        let on_disk = Bookmarks::read(&file);
        std::fs::remove_dir_all(&folder).unwrap();
        assert_eq!(urls(&on_disk), ["https://one.example/", "https://two.example/", "https://three.example/"]);
        assert_eq!(first, on_disk, "the window now holds what is on disk");
    }

    #[test]
    fn an_address_is_kept_once_and_only_a_real_address_is_kept() {
        let mut bookmarks = Bookmarks::default();
        assert!(bookmarks.add(Bookmark::new("https://a.example/", "A")));
        assert!(!bookmarks.add(Bookmark::new("https://a.example/", "A again")));
        assert!(!bookmarks.add(Bookmark::new("no scheme", "B")));
        assert!(!bookmarks.add(Bookmark::new("", "C")));
        assert_eq!(urls(&bookmarks), ["https://a.example/"]);
        assert!(bookmarks.remove("https://a.example/"));
        assert!(!bookmarks.remove("https://a.example/"));
        assert!(bookmarks.is_empty());
    }

    #[test]
    fn suggestions_hold_the_typed_text_in_the_name_or_the_address_at_most_eight() {
        let mut bookmarks = Bookmarks::default();
        bookmarks.add(Bookmark::new("https://news.example/", "Morning Paper"));
        bookmarks.add(Bookmark::new("https://paper.example/", "Stationery"));
        bookmarks.add(Bookmark::new("https://other.example/", "Other"));
        let names = |typed: &str| bookmarks.matching(typed).into_iter().map(|b| b.name).collect::<Vec<_>>();
        assert_eq!(names("PAPER"), ["Morning Paper", "Stationery"]);
        assert_eq!(names("  "), Vec::<String>::new());
        let mut many = Bookmarks::default();
        for n in 0..12 {
            many.add(Bookmark::new(format!("https://site{n}.example/"), ""));
        }
        assert_eq!(many.matching("site").len(), SUGGESTIONS);
    }
}

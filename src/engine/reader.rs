//! The page's own text, read out of the tab in a world the page's own scripts cannot see.
//!
//! The script that reads it ([`SCRIPT`]) looks at the page and returns what it found; nothing
//! here asks the page anything else, and nothing changes the page while it is read.

use serde_json::Value;

/// The script that reads a page and turns it into Markdown. It is run with
/// [`Runtime.evaluate`](super::Engine::read_page) in an isolated world of the tab's main frame,
/// so the page's own scripts neither see it nor see what it found.
pub(super) const SCRIPT: &str = include_str!("reader.js");

/// What a page has to say: the title it carries and the main content as Markdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    /// The page's own title.
    pub title: String,
    /// The main content as Markdown: headings, paragraphs, lists, quotes, code and tables.
    pub text: String,
}

/// The reading the script returned, or why there is none.
///
/// # Errors
///
/// The script's answer carried no text, which it does only when the page is not a document at
/// all; a script that threw is answered by the caller with what it threw.
pub(super) fn reading(value: &Value) -> Result<Reading, String> {
    let text = value["text"].as_str().ok_or("the page held no text to read")?;
    Ok(Reading { title: value["title"].as_str().unwrap_or_default().to_owned(), text: text.to_owned() })
}

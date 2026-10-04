//! A tab's own history: the steps of it, and reading them out of one
//! `Page.getNavigationHistory` answer.
//!
//! The answer is asked for after every navigation already, to learn whether the back and forward
//! buttons have anywhere to go. This is the same answer read a second time, for the whole list
//! rather than for the tab's place in it, so nothing new is asked of Chromium.

use serde_json::Value;

/// One step of a tab's own history, as Chromium reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Where the tab was.
    pub url: String,
    /// The page's title then, which Chromium may leave empty.
    pub title: String,
}

/// The steps of one `Page.getNavigationHistory` answer: which one the tab is at, every step, and
/// Chromium's own number for each.
///
/// A step is read once and both things that want it take it from here: the events that report a
/// tab's place in its history, and the call that steps through it.
pub(super) struct Steps {
    current: usize,
    entries: Vec<Entry>,
    /// Chromium's number for each step, in the same order as `entries`. `Page.navigateToHistoryEntry`
    /// is asked with it, not with the step's place in the list.
    ids: Vec<Value>,
}

impl Steps {
    /// The steps `result` holds and which one the tab is at, or `None` when it holds no list to
    /// walk: no `entries`, no `currentIndex` to read as a number, or an index past the end of a
    /// list that was cut short.
    pub(super) fn read(result: &Value) -> Option<Self> {
        let entries = result["entries"].as_array()?;
        let current = usize::try_from(result["currentIndex"].as_u64()?).ok()?;
        (current < entries.len()).then(|| {
            let read = |entry: &Value| Entry {
                url: entry["url"].as_str().unwrap_or_default().to_owned(),
                // Empty stays empty: a title is what Chromium said, and the screen falls back to
                // the address rather than being handed a title qbrowser made up.
                title: entry["title"].as_str().unwrap_or_default().to_owned(),
            };
            let ids = entries.iter().map(|entry| entry["id"].clone()).collect();
            Self { current, entries: entries.iter().map(read).collect(), ids }
        })
    }

    /// Which step the tab is at.
    pub(super) fn current(&self) -> usize {
        self.current
    }

    /// Every step, in the order Chromium lists them: the oldest first, which is its own order and
    /// not qbrowser's.
    pub(super) fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Chromium's number for the step at `index`, which is what stepping through the history is
    /// asked with; `None` when the answer carried none.
    pub(super) fn id(&self, index: usize) -> Option<&Value> {
        match self.ids.get(index) {
            Some(Value::Null) | None => None,
            Some(id) => Some(id),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// An answer as Chromium sends it, with `entries` as given.
    fn answer(entries: &Value, current: u64) -> Value {
        json!({ "currentIndex": current, "entries": entries })
    }

    #[test]
    fn every_step_is_read_with_its_own_number_and_the_tab_is_at_the_one_chromium_says() {
        let result = answer(
            &json!([
                { "id": 11, "url": "http://127.0.0.1/links", "title": "Links" },
                { "id": 12, "url": "http://127.0.0.1/second", "title": "Second" },
                { "id": 13, "url": "http://127.0.0.1/third", "title": "Third" },
            ]),
            2,
        );
        let steps = Steps::read(&result).expect("three steps to walk");
        assert_eq!(steps.current(), 2);
        assert_eq!(
            steps.entries(),
            [
                Entry { url: "http://127.0.0.1/links".into(), title: "Links".into() },
                Entry { url: "http://127.0.0.1/second".into(), title: "Second".into() },
                Entry { url: "http://127.0.0.1/third".into(), title: "Third".into() },
            ],
        );
        assert_eq!(steps.id(0), Some(&json!(11)), "stepping is asked with Chromium's own number");
        assert_eq!(steps.id(3), None, "past the list there is no step");
    }

    #[test]
    fn an_empty_step_becomes_the_list_the_history_reading_gives() {
        // A title Chromium left out, one it left empty, a data: address, and a list cut short.
        let result = answer(
            &json!([
                { "id": 21, "url": "http://127.0.0.1/links", "title": "Links" },
                { "id": 22, "url": "http://127.0.0.1/second" },
                { "id": 23, "url": "http://127.0.0.1/third", "title": "" },
                { "id": 24, "url": "data:text/html,hi", "title": "A page of its own" },
            ]),
            1,
        );
        let steps = Steps::read(&result).expect("the list is there");
        assert_eq!(steps.entries()[1], Entry { url: "http://127.0.0.1/second".into(), title: String::new() });
        assert_eq!(
            steps.entries()[2].title,
            "",
            "an empty title stays empty: the address is the screen's to fall back to, not the engine's to fill in"
        );
        assert_eq!(steps.entries()[3].url, "data:text/html,hi", "a data: address is a step like any other");
        assert_eq!(steps.id(3), Some(&json!(24)));

        // The same answer cut short: the index is past the list, so there is nothing to walk.
        assert!(Steps::read(&answer(&json!([{ "id": 31, "url": "http://127.0.0.1/links" }]), 4)).is_none());
        // An answer with no list, no index, or an index that is not a number is no list either.
        assert!(Steps::read(&json!({})).is_none());
        assert!(Steps::read(&answer(&json!([]), 0)).is_none());
        assert!(Steps::read(&json!({ "currentIndex": "first", "entries": [] })).is_none());
        // A step without a number of its own cannot be stepped to, and the rest of the list is
        // still there to be read.
        let without = Steps::read(&answer(
            &json!([{ "url": "http://127.0.0.1/links", "title": "Links" }, { "id": 42, "url": "x:" }]),
            0,
        ))
        .expect("two steps");
        assert_eq!(steps_len(&without), 2, "an entry without a number is still a step");
        assert_eq!(without.id(0), None);
        assert_eq!(without.id(1), Some(&json!(42)));
    }

    /// How many steps `steps` holds.
    fn steps_len(steps: &Steps) -> usize {
        steps.entries().len()
    }
}

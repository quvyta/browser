//! Where the address bar goes for what was typed into it: an address as it was written, an
//! address with the scheme it lacked, or a search.
//!
//! The search is DuckDuckGo's: it tracks nobody, asks nobody to sign in and does not turn a
//! headless Chromium away as a robot. That is why it is the default; the other engines are the
//! person's own to pick, on the settings screen.

use std::net::Ipv4Addr;

/// The schemes kept as they are typed. Anything else before a colon (`localhost:8080`,
/// `example.com:443`) is a host and a port.
const SCHEMES: [&str; 6] = ["http", "https", "file", "about", "data", "chrome"];

/// DuckDuckGo's address; the typed words follow, percent-encoded.
const DUCKDUCKGO: &str = "https://duckduckgo.com/?q=";

/// Brave Search's address: its own index, and it asks for no account either.
const BRAVE: &str = "https://search.brave.com/search?q=";

/// Mojeek's address: a small independent index that keeps no profile of what was searched for.
const MOJEEK: &str = "https://www.mojeek.com/search?q=";

/// A search engine the address bar can search with, in the order the picker shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchEngine {
    /// DuckDuckGo, the default.
    DuckDuckGo,
    /// Brave Search.
    Brave,
    /// Mojeek.
    Mojeek,
}

impl SearchEngine {
    /// Every engine, in the order the picker shows them. DuckDuckGo comes first, so a key that is
    /// missing from `browser.conf` means the default without a second piece of state.
    pub const ALL: [Self; 3] = [Self::DuckDuckGo, Self::Brave, Self::Mojeek];

    /// The name `browser.conf` keeps this engine under, and the key its name is shown under.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::DuckDuckGo => "duckduckgo",
            Self::Brave => "brave",
            Self::Mojeek => "mojeek",
        }
    }

    /// The engine `browser.conf` keeps under `name`, or DuckDuckGo for a name it does not know:
    /// a key an older qbrowser wrote, or a value of the wrong shape, must not stop the browser.
    #[must_use]
    pub fn named(name: &str) -> Self {
        Self::ALL.into_iter().find(|engine| engine.name() == name).unwrap_or(Self::DuckDuckGo)
    }

    /// The address a search on this engine goes to; the typed words follow it.
    #[must_use]
    pub fn url(self) -> &'static str {
        match self {
            Self::DuckDuckGo => DUCKDUCKGO,
            Self::Brave => BRAVE,
            Self::Mojeek => MOJEEK,
        }
    }
}

/// What the address bar did with what was typed into it. A row that has to say whether it will
/// open an address or search for words needs the difference, not only the address that comes out
/// of it; [`destination`] is that address on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Going {
    /// Nothing was typed, or only spaces: there is no address to open.
    Nothing,
    /// An address, opened as it stands.
    Address(String),
    /// Words, searched for with `engine`.
    Search(SearchEngine, String),
}

impl Going {
    /// The address to open; empty when there is none.
    #[must_use]
    pub fn address(&self) -> &str {
        match self {
            Self::Nothing => "",
            Self::Address(address) | Self::Search(_, address) => address,
        }
    }
}

/// The address to open for `typed`, trimmed, searching with `engine`:
///
/// - an address with a scheme qbrowser knows (`http`, `https`, `file`, `about`, `data`,
///   `chrome`) stays as it is;
/// - `localhost`, an IPv4 address or a bracketed IPv6 address, with or without a port and a path,
///   gets `http://`, since a server on this machine or the local network rarely speaks TLS;
/// - text with no space whose host part has a dot (`quvyta.com/tr/`) gets `https://`;
/// - anything else, words with spaces or a single word, becomes a search on `engine`;
/// - empty text stays empty, so the caller can tell that nothing was asked for.
#[must_use]
pub fn destination(typed: &str, engine: SearchEngine) -> String {
    going(typed, engine).address().to_owned()
}

/// What [`destination`] did with `typed`, and where it leads.
#[must_use]
pub fn going(typed: &str, engine: SearchEngine) -> Going {
    let typed = typed.trim();
    if typed.is_empty() {
        return Going::Nothing;
    }
    if has_known_scheme(typed) {
        return Going::Address(typed.to_owned());
    }
    if typed.chars().any(char::is_whitespace) {
        return Going::Search(engine, search(typed, engine));
    }
    let host = host_of(typed);
    if is_local_or_ip(host) {
        return Going::Address(format!("http://{typed}"));
    }
    if looks_like_a_domain(host) {
        return Going::Address(format!("https://{typed}"));
    }
    Going::Search(engine, search(typed, engine))
}

/// Whether `text` starts with `scheme:` for one of [`SCHEMES`], in any case.
fn has_known_scheme(text: &str) -> bool {
    text.split_once(':').is_some_and(|(scheme, _)| SCHEMES.iter().any(|known| scheme.eq_ignore_ascii_case(known)))
}

/// The host and port of an address without a scheme: everything before its path, query or
/// fragment.
fn host_of(text: &str) -> &str {
    text.split(['/', '?', '#']).next().unwrap_or(text)
}

/// Whether `host` (with its port, if any) is `localhost`, an IPv4 address or a bracketed IPv6
/// address.
fn is_local_or_ip(host: &str) -> bool {
    if host.starts_with('[') {
        return host.contains(']');
    }
    let name = host.split_once(':').map_or(host, |(name, _)| name);
    name.eq_ignore_ascii_case("localhost") || name.parse::<Ipv4Addr>().is_ok()
}

/// Whether `host` reads as a domain name: a dot between two non-empty labels, and a last label
/// that is not a number, so `3.14` is searched for rather than opened.
fn looks_like_a_domain(host: &str) -> bool {
    let name = host.split_once(':').map_or(host, |(name, _)| name);
    let Some((_, last)) = name.rsplit_once('.') else { return false };
    !name.starts_with('.') && !last.is_empty() && !last.chars().all(|c| c.is_ascii_digit())
}

/// The search for `words` on `engine`, each word percent-encoded as it is typed.
fn search(words: &str, engine: SearchEngine) -> String {
    let mut address = String::from(engine.url());
    for byte in words.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            address.push(char::from(byte));
        } else {
            address.push_str(&format!("%{byte:02X}"));
        }
    }
    address
}

#[cfg(test)]
mod tests {
    use super::{SearchEngine, destination, going};

    /// The engine the address bar uses when nothing is chosen.
    const DEFAULT: SearchEngine = SearchEngine::DuckDuckGo;

    #[test]
    fn an_address_with_a_known_scheme_stays_as_it_is() {
        for typed in [
            "https://quvyta.com/tr/",
            "http://example.com",
            "file:///home/me/page.html",
            "about:blank",
            "data:text/html,<p>hi</p>",
            "chrome://version",
            "HTTPS://Quvyta.com",
        ] {
            assert_eq!(destination(typed, DEFAULT), typed);
        }
        assert_eq!(destination("  https://quvyta.com  ", DEFAULT), "https://quvyta.com", "the ends are trimmed");
    }

    #[test]
    fn a_domain_gets_https_and_this_machine_gets_http() {
        assert_eq!(destination("quvyta.com/tr/", DEFAULT), "https://quvyta.com/tr/");
        assert_eq!(destination("docs.rs/serde?search=x", DEFAULT), "https://docs.rs/serde?search=x");
        assert_eq!(destination("example.com:8443", DEFAULT), "https://example.com:8443");
        assert_eq!(destination("localhost", DEFAULT), "http://localhost");
        assert_eq!(destination("localhost:8080/app", DEFAULT), "http://localhost:8080/app");
        assert_eq!(destination("127.0.0.1:3000", DEFAULT), "http://127.0.0.1:3000");
        assert_eq!(destination("192.168.1.1/admin", DEFAULT), "http://192.168.1.1/admin");
        assert_eq!(destination("[::1]:8080/", DEFAULT), "http://[::1]:8080/");
    }

    #[test]
    fn words_a_single_word_and_a_number_are_searched_for() {
        assert_eq!(destination("rust tui", DEFAULT), "https://duckduckgo.com/?q=rust%20tui");
        assert_eq!(destination("quvyta", DEFAULT), "https://duckduckgo.com/?q=quvyta");
        assert_eq!(destination("3.14", DEFAULT), "https://duckduckgo.com/?q=3.14");
        assert_eq!(
            destination("a&b=c", DEFAULT),
            "https://duckduckgo.com/?q=a%26b%3Dc",
            "what a query parses is escaped"
        );
        assert_eq!(destination("çay nasıl", DEFAULT), "https://duckduckgo.com/?q=%C3%A7ay%20nas%C4%B1l");
        assert_eq!(destination("what is quvyta.com", DEFAULT), "https://duckduckgo.com/?q=what%20is%20quvyta.com");
    }

    #[test]
    fn nothing_typed_asks_for_nothing() {
        assert_eq!(destination("", DEFAULT), "");
        assert_eq!(destination("   ", DEFAULT), "");
    }

    #[test]
    fn each_engine_searches_where_its_own_address_says() {
        for engine in SearchEngine::ALL {
            assert_eq!(
                destination("quvyta browser", engine),
                format!("{}quvyta%20browser", engine.url()),
                "{:?} searches where its own address says",
                engine
            );
            assert!(engine.url().starts_with("https://"), "a search address is a web address");
        }
        assert_eq!(destination("rust tui", SearchEngine::Brave), "https://search.brave.com/search?q=rust%20tui");
        assert_eq!(destination("rust tui", SearchEngine::Mojeek), "https://www.mojeek.com/search?q=rust%20tui");
    }

    #[test]
    fn the_engine_changes_only_what_is_searched_for() {
        for typed in ["quvyta.com/tr/", "localhost:8080", "about:blank", "192.168.1.1"] {
            for engine in SearchEngine::ALL {
                assert_eq!(destination(typed, engine), destination(typed, SearchEngine::DuckDuckGo), "{typed}");
            }
        }
    }

    #[test]
    fn an_engine_qbrowser_does_not_know_is_the_default() {
        assert_eq!(SearchEngine::named("brave"), SearchEngine::Brave);
        assert_eq!(SearchEngine::named(""), SearchEngine::DuckDuckGo);
        assert_eq!(SearchEngine::named("yahoo"), SearchEngine::DuckDuckGo);
    }

    #[test]
    fn a_row_is_told_whether_it_opens_an_address_or_searches_for_words() {
        assert_eq!(going("", SearchEngine::Brave).address(), "");
        assert_eq!(going("   ", SearchEngine::Brave).address(), "");
        assert!(matches!(going("quvyta.com", SearchEngine::Brave), super::Going::Address(_)));
        assert_eq!(going("quvyta.com", SearchEngine::Brave).address(), "https://quvyta.com");
        let words = going("rust tui", SearchEngine::Brave);
        assert_eq!(
            words,
            super::Going::Search(SearchEngine::Brave, "https://search.brave.com/search?q=rust%20tui".to_owned())
        );
        assert_eq!(words.address(), "https://search.brave.com/search?q=rust%20tui");
    }
}

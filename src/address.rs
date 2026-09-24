//! Where the address bar goes for what was typed into it: an address as it was written, an
//! address with the scheme it lacked, or a search.
//!
//! The search is DuckDuckGo's: it tracks nobody, asks nobody to sign in and does not turn a
//! headless Chromium away as a robot.

use std::net::Ipv4Addr;

/// The schemes kept as they are typed. Anything else before a colon (`localhost:8080`,
/// `example.com:443`) is a host and a port.
const SCHEMES: [&str; 6] = ["http", "https", "file", "about", "data", "chrome"];

/// Where the search goes; the typed words follow, percent-encoded.
const SEARCH: &str = "https://duckduckgo.com/?q=";

/// The address to open for `typed`, trimmed:
///
/// - an address with a scheme qbrowser knows (`http`, `https`, `file`, `about`, `data`,
///   `chrome`) stays as it is;
/// - `localhost`, an IPv4 address or a bracketed IPv6 address, with or without a port and a path,
///   gets `http://`, since a server on this machine or the local network rarely speaks TLS;
/// - text with no space whose host part has a dot (`quvyta.com/tr/`) gets `https://`;
/// - anything else, words with spaces or a single word, becomes a DuckDuckGo search;
/// - empty text stays empty, so the caller can tell that nothing was asked for.
#[must_use]
pub fn destination(typed: &str) -> String {
    let typed = typed.trim();
    if typed.is_empty() {
        return String::new();
    }
    if has_known_scheme(typed) {
        return typed.to_owned();
    }
    if typed.chars().any(char::is_whitespace) {
        return search(typed);
    }
    let host = host_of(typed);
    if is_local_or_ip(host) {
        return format!("http://{typed}");
    }
    if looks_like_a_domain(host) {
        return format!("https://{typed}");
    }
    search(typed)
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

/// The DuckDuckGo search for `words`.
fn search(words: &str) -> String {
    let mut address = String::from(SEARCH);
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
    use super::destination;

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
            assert_eq!(destination(typed), typed);
        }
        assert_eq!(destination("  https://quvyta.com  "), "https://quvyta.com", "the ends are trimmed");
    }

    #[test]
    fn a_domain_gets_https_and_this_machine_gets_http() {
        assert_eq!(destination("quvyta.com/tr/"), "https://quvyta.com/tr/");
        assert_eq!(destination("docs.rs/serde?search=x"), "https://docs.rs/serde?search=x");
        assert_eq!(destination("example.com:8443"), "https://example.com:8443");
        assert_eq!(destination("localhost"), "http://localhost");
        assert_eq!(destination("localhost:8080/app"), "http://localhost:8080/app");
        assert_eq!(destination("127.0.0.1:3000"), "http://127.0.0.1:3000");
        assert_eq!(destination("192.168.1.1/admin"), "http://192.168.1.1/admin");
        assert_eq!(destination("[::1]:8080/"), "http://[::1]:8080/");
    }

    #[test]
    fn words_a_single_word_and_a_number_are_searched_for() {
        assert_eq!(destination("rust tui"), "https://duckduckgo.com/?q=rust%20tui");
        assert_eq!(destination("quvyta"), "https://duckduckgo.com/?q=quvyta");
        assert_eq!(destination("3.14"), "https://duckduckgo.com/?q=3.14");
        assert_eq!(destination("a&b=c"), "https://duckduckgo.com/?q=a%26b%3Dc", "what a query parses is escaped");
        assert_eq!(destination("çay nasıl"), "https://duckduckgo.com/?q=%C3%A7ay%20nas%C4%B1l");
        assert_eq!(destination("what is quvyta.com"), "https://duckduckgo.com/?q=what%20is%20quvyta.com");
    }

    #[test]
    fn nothing_typed_asks_for_nothing() {
        assert_eq!(destination(""), "");
        assert_eq!(destination("   "), "");
    }
}

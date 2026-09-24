//! The command line: `qbrow [ADDRESS] [--profile FOLDER]`, `--version` and `--help`.
//!
//! The address is what the address bar would take: a web address, words to search for, or the
//! path of a file or folder on this machine, which opens as a `file://` address so that a file
//! manager can open a saved page with qbrowser.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::address;

/// How the screen starts.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Start {
    /// The address the first tab opens, or `None` for an empty tab.
    pub address: Option<String>,
    /// The folder that holds the profile, given with `--profile`, or `None` for the usual one.
    pub profile: Option<PathBuf>,
}

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    /// Open the screen.
    Screen(Start),
    /// Print the version and leave.
    Version,
    /// Print how to use qbrowser and leave.
    Help,
    /// `--profile` without a folder after it.
    NoProfileFolder,
    /// An option qbrowser does not know, or more than one address.
    Unknown(String),
}

/// Reads the arguments after the program name. Relative paths, of a file to open or of the
/// profile folder, are read from `cwd`.
#[must_use]
pub fn parse(args: impl IntoIterator<Item = OsString>, cwd: &Path) -> Invocation {
    let mut start = Start::default();
    let mut only_addresses = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy().into_owned();
        if !only_addresses && text.starts_with('-') && text.len() > 1 {
            match text.as_str() {
                "--version" | "-V" => return Invocation::Version,
                "--help" | "-h" => return Invocation::Help,
                "--" => only_addresses = true,
                "--profile" => match args.next() {
                    Some(folder) => start.profile = Some(cwd.join(folder)),
                    None => return Invocation::NoProfileFolder,
                },
                other => match other.strip_prefix("--profile=") {
                    Some("") => return Invocation::NoProfileFolder,
                    Some(folder) => start.profile = Some(cwd.join(folder)),
                    None => return Invocation::Unknown(other.to_owned()),
                },
            }
            continue;
        }
        if start.address.is_some() {
            return Invocation::Unknown(text);
        }
        start.address = Some(address_for(&text, cwd)).filter(|address| !address.is_empty());
    }
    Invocation::Screen(start)
}

/// The address `argument` opens: a `file://` address when it names something on this machine,
/// otherwise what the address bar makes of it.
fn address_for(argument: &str, cwd: &Path) -> String {
    // An empty argument would name `cwd` itself.
    if argument.trim().is_empty() {
        return String::new();
    }
    let path = cwd.join(argument);
    // A web address is not a path; `quvyta.com` is looked for only in case a file has that name.
    let is_web = argument.contains("://") || (argument.contains(':') && !argument.starts_with('/'));
    match path.canonicalize() {
        Ok(found) if !is_web => file_address(&found),
        _ => address::destination(argument),
    }
}

/// The `file://` address of an absolute path, with every byte a URL cannot carry
/// percent-encoded.
fn file_address(path: &Path) -> String {
    let mut address = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~') {
            address.push(char::from(byte));
        } else {
            address.push_str(&format!("%{byte:02X}"));
        }
    }
    address
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("qbrow-cli-{name}-{}", std::process::id()));
        fs::create_dir_all(root.join("saved pages")).expect("folders");
        fs::write(root.join("saved pages/çay.html"), "<p>tea</p>").expect("file");
        fs::canonicalize(root).expect("root")
    }

    fn screen(address: Option<&str>, profile: Option<PathBuf>) -> Invocation {
        Invocation::Screen(Start { address: address.map(str::to_owned), profile })
    }

    #[test]
    fn an_address_opens_as_the_address_bar_would_and_nothing_opens_an_empty_tab() {
        let cwd = Path::new("/nowhere");
        assert_eq!(parse(args(&[]), cwd), screen(None, None));
        assert_eq!(parse(args(&["quvyta.com"]), cwd), screen(Some("https://quvyta.com"), None));
        assert_eq!(parse(args(&["localhost:8080"]), cwd), screen(Some("http://localhost:8080"), None));
        assert_eq!(parse(args(&["rust tui"]), cwd), screen(Some("https://duckduckgo.com/?q=rust%20tui"), None));
        assert_eq!(parse(args(&["about:blank"]), cwd), screen(Some("about:blank"), None));
        assert_eq!(parse(args(&["  "]), cwd), screen(None, None), "blank text asks for nothing");
        assert_eq!(parse(args(&[""]), Path::new("/")), screen(None, None), "nor does it open the folder qbrow ran in");
    }

    #[test]
    fn a_file_on_this_machine_opens_as_a_file_address() {
        let root = scratch("file");
        let expected = format!("file://{}/saved%20pages/%C3%A7ay.html", root.display());
        assert_eq!(parse(args(&["saved pages/çay.html"]), &root), screen(Some(&expected), None));
        let absolute = root.join("saved pages/çay.html");
        assert_eq!(parse(args(&[absolute.to_str().unwrap()]), Path::new("/")), screen(Some(&expected), None));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_profile_folder_is_read_from_where_qbrow_started() {
        let cwd = Path::new("/work");
        assert_eq!(
            parse(args(&["--profile", "try", "quvyta.com"]), cwd),
            screen(Some("https://quvyta.com"), Some(PathBuf::from("/work/try")))
        );
        assert_eq!(parse(args(&["--profile=/tmp/p"]), cwd), screen(None, Some(PathBuf::from("/tmp/p"))));
        assert_eq!(parse(args(&["--profile"]), cwd), Invocation::NoProfileFolder);
        assert_eq!(parse(args(&["--profile="]), cwd), Invocation::NoProfileFolder);
    }

    #[test]
    fn options_are_read_and_the_unknown_is_named() {
        let cwd = Path::new("/");
        assert_eq!(parse(args(&["--version"]), cwd), Invocation::Version);
        assert_eq!(parse(args(&["-V"]), cwd), Invocation::Version);
        assert_eq!(parse(args(&["--help"]), cwd), Invocation::Help);
        assert_eq!(parse(args(&["-h"]), cwd), Invocation::Help);
        assert_eq!(parse(args(&["--frobnicate"]), cwd), Invocation::Unknown("--frobnicate".into()));
        assert_eq!(parse(args(&["a.com", "b.com"]), cwd), Invocation::Unknown("b.com".into()));
        assert_eq!(
            parse(args(&["--", "-x"]), cwd),
            screen(Some("https://duckduckgo.com/?q=-x"), None),
            "after -- a dash starts an address"
        );
    }
}

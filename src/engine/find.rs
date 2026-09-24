//! Where Chromium is on this machine.

use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// The program names searched for, in order. Only Chromium and Chrome: other browsers built on
/// Chromium may treat the profile and the flags differently.
const NAMES: [&str; 4] = ["chromium", "chromium-browser", "google-chrome-stable", "google-chrome"];

/// `explicit` when it is a file, else the first of `chromium`, `chromium-browser`,
/// `google-chrome-stable`, `google-chrome` found as an executable file in a folder of `path_var`
/// (a `PATH`-style list). `None` when neither gives one.
#[must_use]
pub fn find_chromium(explicit: Option<&Path>, path_var: Option<&OsStr>) -> Option<PathBuf> {
    if let Some(explicit) = explicit.filter(|path| path.is_file()) {
        return Some(explicit.to_path_buf());
    }
    // An empty entry means the working folder to a shell; a browser picked up from wherever the
    // program happens to be started would be a surprise, so it is skipped.
    let folders: Vec<PathBuf> =
        std::env::split_paths(path_var?).filter(|folder| !folder.as_os_str().is_empty()).collect();
    NAMES
        .iter()
        .flat_map(|name| folders.iter().map(move |folder| folder.join(name)))
        .find(|candidate| is_executable(candidate))
}

/// Whether `path` is a file someone may run; a file without any execute bit on the search path is
/// not a program, as the shell sees it too.
fn is_executable(path: &Path) -> bool {
    path.metadata().is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tests::fixture::Scratch;
    use std::ffi::OsString;
    use std::fs;

    fn program(path: &Path) {
        fs::write(path, "#!/bin/sh\n").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn an_explicit_file_wins_over_the_search_path() {
        let scratch = Scratch::new();
        let bin = scratch.dir("bin");
        program(&bin.join("chromium"));
        let explicit = scratch.path().join("my-chrome");
        program(&explicit);
        assert_eq!(find_chromium(Some(&explicit), Some(bin.as_os_str())), Some(explicit));
    }

    #[test]
    fn a_missing_explicit_path_falls_back_to_the_search_path_in_name_order() {
        let scratch = Scratch::new();
        let first = scratch.dir("first");
        let second = scratch.dir("second");
        program(&first.join("google-chrome"));
        program(&second.join("chromium-browser"));
        let path_var = std::env::join_paths([&first, &second]).unwrap();
        let missing = scratch.path().join("nowhere");
        assert_eq!(find_chromium(Some(&missing), Some(&path_var)), Some(second.join("chromium-browser")));
    }

    #[test]
    fn a_file_that_cannot_run_is_not_found_and_neither_is_an_empty_path() {
        let scratch = Scratch::new();
        let bin = scratch.dir("bin");
        fs::write(bin.join("chromium"), "").unwrap();
        assert_eq!(find_chromium(None, Some(bin.as_os_str())), None);
        assert_eq!(find_chromium(None, None), None);
        assert_eq!(find_chromium(None, Some(&OsString::new())), None);
    }
}

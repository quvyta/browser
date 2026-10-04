//! The rule no screen shows, held over the source: process facts come from `/proc` and signals go
//! out through the system, never through a program that may not be installed.

use std::path::Path;

/// The programs that look at processes or signal them, each in two halves so that this file's own
/// text is not among them. A container without procps, a Raspberry Pi and a shell whose `kill` is
/// only a builtin all lack at least one of these, and a signal that cannot be sent is not an
/// error: it fails silently and leaves the process behind.
fn helper_names() -> Vec<String> {
    [("ki", "ll"), ("p", "kill"), ("p", "s"), ("pg", "rep"), ("pi", "dof"), ("pstr", "ee"), ("t", "op"), ("ht", "op")]
        .iter()
        .map(|(first, second)| format!("{first}{second}"))
        .collect()
}

/// The program a line starts with `Command::new(..)`, when it names one in the text; a name that
/// is not written there (`Command::new(CHROMIUM)`, a path the test chose itself) is not this
/// guard's business.
fn named_program(line: &str) -> Option<&str> {
    let after = line.split_once("Command::new(")?.1;
    Some(after.split_once('"')?.1.split_once('"')?.0)
}

/// A test about the source, and not a fake one: what it protects cannot be seen on a screen, and
/// the failure it is there to catch is a green run that quietly checked nothing. That is the same
/// danger
/// [`every_test_that_runs_chromium_is_ignored_without_it`](super::fixture::every_test_that_runs_chromium_is_ignored_without_it)
/// is held against: there, a browser test that runs without the mark fails on a machine without
/// Chromium instead of being listed as ignored; here, a file that runs `kill` would work on the
/// owner's machine and do nothing at all on a machine that has no such program.
#[test]
fn no_process_starts_a_helper_program() {
    let helpers = helper_names();
    fn visit(folder: &Path, helpers: &[String], started: &mut Vec<String>) {
        for entry in std::fs::read_dir(folder).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, helpers, started);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                // This file names what it looks for, so it is not held to its own rule.
                if path.ends_with("helpers.rs") {
                    continue;
                }
                for (n, line) in std::fs::read_to_string(&path).unwrap().lines().enumerate() {
                    let Some(program) = named_program(line) else { continue };
                    if program.split('/').any(|part| helpers.iter().any(|helper| helper == part)) {
                        started.push(format!("{}:{} starts {program}", path.display(), n + 1));
                    }
                }
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut started = Vec::new();
    visit(&root.join("src"), &helpers, &mut started);
    visit(&root.join("tests"), &helpers, &mut started);
    assert!(
        started.is_empty(),
        "a program that looks at processes or signals them: /proc and a signal of our own do the same without one.\n{}",
        started.join("\n")
    );
}

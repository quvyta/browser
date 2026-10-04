//! Finds the Chromium the tests drive, so that a machine without one ignores those tests out loud
//! instead of failing each of them after a long wait, and the program itself is built the same
//! either way.
//!
//! The tests run `QBROW_TEST_CHROMIUM`, or `/usr/bin/chromium` when it is not set. When that file
//! is there, the `chromium` cfg is set and every test runs; when it is not, each test that drives a
//! real Chromium is listed as ignored with the reason "needs Chromium", and the build says which
//! path it looked at. Nothing here reaches the program's own code: only the tests read it.

use std::path::Path;

/// Where the tests look for Chromium when `QBROW_TEST_CHROMIUM` is not set.
const DEFAULT: &str = "/usr/bin/chromium";

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=QBROW_TEST_CHROMIUM");
    let chromium = std::env::var("QBROW_TEST_CHROMIUM").unwrap_or_else(|_| DEFAULT.to_owned());
    let path = Path::new(&chromium);
    // Chromium installed later is noticed through its folder, whose time changes when a file is
    // added; watching the missing file itself would run this again on every build.
    let watched = if path.exists() { Some(path) } else { path.ancestors().skip(1).find(|folder| folder.is_dir()) };
    if let Some(watched) = watched {
        println!("cargo::rerun-if-changed={}", watched.display());
    }
    println!("cargo::rustc-env=QBROW_TEST_CHROMIUM={chromium}");
    if path.is_file() {
        println!("cargo::rustc-cfg=chromium");
    } else {
        println!(
            "cargo::warning=no Chromium at {chromium}: the tests that drive a real Chromium are ignored; \
             install it or set QBROW_TEST_CHROMIUM to run them"
        );
    }
}

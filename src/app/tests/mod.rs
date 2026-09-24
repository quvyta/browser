//! Screen tests, end to end: the whole screen in the framework's harness, a real headless
//! Chromium, and the engine tests' local web server. Every test builds a machine of its own in a
//! scratch folder: the profile, the temporary profiles, the shared Quvyta settings and the search
//! path are all in it. The harness records the programs a test would hand the terminal to and
//! runs none, and nothing reaches a real site or the desktop.
//!
//! The engine runs on real threads while the harness keeps its own clock, so the tests move the
//! clock in small steps, with a short real pause between them, until what they wait for is on
//! screen or in the page; never longer than [`PATIENCE`].

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use qframe::color::ColorDepth;
use qframe::graphics::Graphics;
use qframe::icons::GlyphMode;
use qframe::prelude::*;
use serde_json::Value;

use super::{Browser, Machine, Opening, UpdateFolders};
use crate::cli::Start;
use crate::engine::tests::fixture::{self, CHROMIUM, PATIENCE};

mod bookmarks;
mod browsing;
mod states;
mod tabs;

pub(super) use fixture::{Slot, page};

/// The fixed size of a cell in the tests, in pixels.
pub(super) const CELL: (u32, u32) = (10, 20);

/// The screen's size in the tests.
const WIDTH: u16 = 100;
const HEIGHT: u16 = 32;

/// The first row of the page area.
pub(super) const PAGE_TOP: i32 = 2;

/// A scratch folder with a machine in it.
pub(super) struct Scratch(fixture::Scratch);

impl Scratch {
    pub(super) fn new() -> Self {
        let scratch = fixture::Scratch::new();
        let config = scratch.dir("config");
        std::fs::write(config.join("quvyta.conf"), "language = \"en\"\nicons = \"unicode\"\n").unwrap();
        scratch.dir("bin");
        Self(scratch)
    }

    pub(super) fn path(&self, relative: &str) -> PathBuf {
        self.0.path().join(relative)
    }

    /// Puts a program named `name` that does nothing on the machine's search path.
    pub(super) fn program(&self, name: &str) {
        let path = self.path("bin").join(name);
        std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// The machine: the test Chromium, everything else in the scratch folder, cells 10 × 20.
    pub(super) fn machine(&self) -> Machine {
        Machine {
            chromium: Some(PathBuf::from(CHROMIUM)),
            path_var: Some(self.path("bin").into_os_string()),
            profile_home: self.path("home"),
            temp_root: self.path("temp"),
            config: Some(self.path("config")),
            updates: Some(UpdateFolders { config: self.path("config"), state: self.path("state") }),
            cell: Some(CELL),
            bookmarks: Some(self.path("data/quvyta/browser/bookmarks")),
        }
    }

    /// A machine where Chromium is nowhere.
    pub(super) fn machine_without_chromium(&self) -> Machine {
        Machine { chromium: Some(self.path("no/chromium")), ..self.machine() }
    }
}

/// The screen on `machine`, opening `address` (an empty tab without one), in English with
/// Unicode glyphs and half-block pictures, drawn the same wherever the tests run.
pub(super) fn open_on(machine: Machine, address: Option<&str>) -> Harness<Browser> {
    let start = Start { address: address.map(str::to_owned), profile: None };
    let opening = Opening::new(machine, &start);
    let mut h = Harness::with_env(opening.browser, crate::locales::env(), WIDTH, HEIGHT);
    h.set_locale("en")
        .set_glyph_mode(GlyphMode::Unicode)
        .set_depth(ColorDepth::TrueColor)
        .set_graphics(Graphics::HalfBlock)
        .set_reduced_motion(true);
    h
}

/// The screen of `scratch`'s machine on `address`, once its page is drawn.
pub(super) fn open(scratch: &Scratch, address: &str) -> Harness<Browser> {
    let mut h = open_on(scratch.machine(), Some(address));
    until(&mut h, &format!("{address} drawn"), |h| h.app().address() == address && page_drawn(h));
    h
}

/// Moves the clock in small steps until `done` holds, and fails with the screen when it does not
/// within [`PATIENCE`].
pub(super) fn until(h: &mut Harness<Browser>, what: &str, done: impl Fn(&Harness<Browser>) -> bool) {
    let deadline = Instant::now() + PATIENCE;
    loop {
        h.advance(Duration::from_millis(20));
        if done(h) {
            return;
        }
        assert!(Instant::now() < deadline, "no {what} within {PATIENCE:?}:\n{}", h.screen());
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Whether a picture of the page is drawn: half blocks fill the page area.
pub(super) fn page_drawn(h: &Harness<Browser>) -> bool {
    h.screen().lines().skip(PAGE_TOP.cast_unsigned() as usize).any(|line| line.contains('▀'))
}

/// The value of `expression` in the page on screen.
pub(super) fn eval(h: &Harness<Browser>, expression: &str) -> Value {
    let browser = h.app();
    let (Some(engine), Some(tab)) = (browser.engine(), browser.active_tab()) else {
        panic!("no page to ask `{expression}`:\n{}", h.screen());
    };
    engine.evaluate(tab, expression, PATIENCE).unwrap_or_else(|error| panic!("`{expression}` failed: {error}"))
}

/// Waits until `expression` is true in the page on screen.
pub(super) fn until_page(h: &mut Harness<Browser>, expression: &str) {
    until(h, &format!("`{expression}`"), |h| {
        h.app().active_tab().is_some() && eval(h, expression) == Value::Bool(true)
    });
}

/// The screen cell where the middle of the element `selector` is drawn.
pub(super) fn cell_of(h: &Harness<Browser>, selector: &str) -> (i32, i32) {
    let rect = eval(
        h,
        &format!(
            "(() => {{ const r = document.querySelector('{selector}').getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; }})()"
        ),
    );
    let (x, y) = (rect[0].as_f64().unwrap(), rect[1].as_f64().unwrap());
    let column = (x / f64::from(CELL.0)).floor();
    let row = (y / f64::from(CELL.1)).floor();
    // Whole cells of a page a few hundred pixels across.
    #[expect(clippy::cast_possible_truncation, reason = "a cell of the test screen")]
    (column as i32, row as i32 + PAGE_TOP)
}

/// Where `text` first is on screen row `row`.
pub(super) fn find_in_row(h: &Harness<Browser>, text: &str, row: usize) -> Option<(i32, i32)> {
    let line = h.screen().lines().nth(row)?.to_owned();
    let at = line.find(text)?;
    let x = line[..at].chars().count();
    Some((i32::try_from(x).ok()?, i32::try_from(row).ok()?))
}

/// Clicks the toolbar's button drawn with the glyph of `icon`.
pub(super) fn click_icon(h: &mut Harness<Browser>, icon: &str) {
    let glyph = h.env().icons().glyph(icon).into_owned();
    let (x, y) = find_in_row(h, &glyph, 1).unwrap_or_else(|| panic!("no {icon} button:\n{}", h.screen()));
    h.click(x, y);
}

//! qbrowser: a real web browser inside the terminal. An unmodified Chromium runs out of sight and
//! draws the pages; qbrowser drives it from outside through the DevTools protocol, shows what it
//! draws and hands it the clicks, the wheel and the keys.

pub mod address;
pub mod app;
pub mod bookmarks;
pub mod cli;
pub mod engine;
pub mod keys;
pub mod locales;
pub mod page_view;

use std::io::{self, Write};
use std::process::ExitCode;
use std::sync::Arc;

use qframe::prelude::*;

use app::{Machine, Opening};
use cli::Invocation;

/// Runs one `qbrow` invocation on this machine. Both commands, `qbrow` and `quvyta-browser`,
/// start here.
///
/// # Errors
///
/// Returns the terminal's error when the screen cannot be opened or drawn.
pub fn run() -> io::Result<ExitCode> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| "/".into());
    let start = match cli::parse(std::env::args_os().skip(1), &cwd) {
        Invocation::Screen(start) => start,
        other => return Ok(answer(&other)),
    };
    runtime(Machine::here(start.profile.clone()), &start).run()?;
    Ok(ExitCode::SUCCESS)
}

/// The runtime that shows the screen on `machine`: qbrow's own languages, keys and icons, its
/// saved look, and, where the machine has a Quvyta folder, membership of the ecosystem, so a look
/// another Quvyta application changes while qbrow is open is followed at once. The settings and
/// preferences given here are used as they are and not read twice.
pub fn runtime(machine: Machine, start: &cli::Start) -> Runtime<app::Browser> {
    let config = machine.config.clone();
    let opening = Opening::new(machine, start);
    let runtime = locales::LOCALES
        .iter()
        .fold(Runtime::new(opening.browser), |runtime, (file, text)| runtime.locale_source(*file, *text))
        .keymap_source(locales::KEYMAP.0, locales::KEYMAP.1)
        .icon_source(locales::ICONS.0, locales::ICONS.1)
        .settings(&opening.settings)
        .preferences(&opening.preferences);
    match config {
        Some(folder) => runtime.member_in(qframe::storage::Ecosystem::QUVYTA, folder, app::APP),
        None => runtime,
    }
}

/// Says what a command line that opens no screen asks for, in the person's language, and gives the
/// exit code: 0 for the version and the help, 2 for an unknown option or a missing profile folder.
#[must_use]
pub fn answer(invocation: &Invocation) -> ExitCode {
    let env = locales::env();
    let detected = env.i18n().detect(|name| std::env::var(name).ok());
    let mut i18n = env.i18n().clone();
    if let Some(code) = detected {
        i18n.set_active(&code);
    }
    qframe::i18n::scope(Arc::new(i18n), || {
        let mut out = io::stdout().lock();
        match invocation {
            Invocation::Screen(_) => ExitCode::SUCCESS,
            Invocation::Version => {
                let _ = writeln!(out, "qbrow {}", env!("CARGO_PKG_VERSION"));
                ExitCode::SUCCESS
            }
            Invocation::Help => {
                let _ = writeln!(out, "{}", t!("browser.cli.help", version = env!("CARGO_PKG_VERSION")));
                ExitCode::SUCCESS
            }
            Invocation::NoProfileFolder => {
                eprintln!("{}", t!("browser.cli.no-profile-folder"));
                ExitCode::from(2)
            }
            Invocation::Unknown(argument) => {
                eprintln!("{}", t!("browser.cli.unknown", argument = argument.as_str()));
                ExitCode::from(2)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_option_or_a_missing_profile_folder_exits_with_2_and_the_rest_with_0() {
        assert_eq!(answer(&Invocation::Unknown("--frobnicate".into())), ExitCode::from(2));
        assert_eq!(answer(&Invocation::NoProfileFolder), ExitCode::from(2));
        assert_eq!(answer(&Invocation::Version), ExitCode::SUCCESS);
        assert_eq!(answer(&Invocation::Help), ExitCode::SUCCESS);
    }
}

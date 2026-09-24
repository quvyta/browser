//! Installing Chromium where it is missing: which package manager this system has, the command
//! that installs Chromium with it, and running that command in the terminal before the person's
//! eyes, only when they press "Install here".

use std::ffi::OsStr;
use std::path::Path;

use qframe::runtime::{Command, Handoff};

use super::Msg;

/// The package managers qbrowser knows, in the order they are looked for, with the words that
/// install Chromium through them.
const MANAGERS: [(&str, &[&str]); 4] = [
    ("pacman", &["-S", "chromium"]),
    ("apt", &["install", "chromium"]),
    ("dnf", &["install", "chromium"]),
    ("zypper", &["install", "chromium"]),
];

/// The programs that let a person act as the system's administrator, in the order they are
/// looked for. The one found asks for the password itself.
const ELEVATORS: [&str; 3] = ["sudo", "doas", "run0"];

/// The command that installs Chromium on this system, as the person would type it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallCommand {
    /// The program run first: the administrator's gate, or the package manager itself when
    /// there is no gate.
    pub program: String,
    /// What follows it.
    pub args: Vec<String>,
}

impl InstallCommand {
    /// The command for the package manager found on `path_var`, or `None` when none is there.
    #[must_use]
    pub fn find(path_var: Option<&OsStr>) -> Option<Self> {
        let on_path = |name: &str| path_var.is_some_and(|path| on_path(path, name));
        let (manager, words) = MANAGERS.iter().find(|(manager, _)| on_path(manager))?;
        let mut command: Vec<String> = Vec::new();
        if let Some(elevator) = ELEVATORS.iter().find(|elevator| on_path(elevator)) {
            command.push((*elevator).to_owned());
        }
        command.push((*manager).to_owned());
        command.extend(words.iter().map(|word| (*word).to_owned()));
        let program = command.remove(0);
        Some(Self { program, args: command })
    }

    /// The command as one line of text.
    #[must_use]
    pub fn line(&self) -> String {
        std::iter::once(self.program.as_str()).chain(self.args.iter().map(String::as_str)).collect::<Vec<_>>().join(" ")
    }

    /// Runs the command in the terminal and waits for it; the person sees its output and answers
    /// its questions, and reads how it ended before the screen comes back.
    pub(super) fn run(&self) -> Command<Msg> {
        Command::handoff(Handoff::new(self.program.clone(), Msg::Installed).args(self.args.clone()).pause(true))
    }
}

/// Whether `name` is an executable file in one of the folders of `path_var`.
fn on_path(path_var: &OsStr, name: &str) -> bool {
    std::env::split_paths(path_var).any(|folder| is_executable(&folder.join(name)))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata().is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    use super::*;

    fn folder_with(name: &str, programs: &[&str]) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("qbrowser-install-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        for program in programs {
            let path = folder.join(program);
            fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        folder
    }

    #[test]
    fn the_package_manager_found_installs_chromium_through_the_administrators_gate() {
        let folder = folder_with("gate", &["sudo", "apt"]);
        let command = InstallCommand::find(Some(folder.as_os_str())).unwrap();
        assert_eq!(command.line(), "sudo apt install chromium");
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn without_a_gate_the_package_manager_runs_itself_and_without_one_nothing_is_offered() {
        let folder = folder_with("bare", &["zypper"]);
        assert_eq!(InstallCommand::find(Some(folder.as_os_str())).unwrap().line(), "zypper install chromium");
        let empty = folder_with("empty", &[]);
        assert_eq!(InstallCommand::find(Some(empty.as_os_str())), None);
        assert_eq!(InstallCommand::find(None), None);
        let _ = fs::remove_dir_all(&folder);
        let _ = fs::remove_dir_all(&empty);
    }

    #[test]
    fn a_file_that_cannot_run_is_no_package_manager() {
        let folder = folder_with("plain", &[]);
        fs::write(folder.join("pacman"), "not a program").unwrap();
        assert_eq!(InstallCommand::find(Some(folder.as_os_str())), None);
        let _ = fs::remove_dir_all(&folder);
    }
}

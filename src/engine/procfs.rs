//! What `/proc` says about a process: whether it still runs, its group and its command line.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;

/// The fields of `/proc/<pid>/stat` after the command name: the state and the process group.
fn stat(pid: u32) -> Option<(char, u32)> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name sits in parentheses and may itself hold spaces or parentheses, so the
    // fields are read from after the last closing one.
    let rest = &text[text.rfind(')')? + 1..];
    let mut fields = rest.split_whitespace();
    let state = fields.next()?.chars().next()?;
    let _parent = fields.next()?;
    let group = fields.next()?.parse().ok()?;
    Some((state, group))
}

/// Whether `pid` no longer runs: gone, or a zombie waiting to be reaped. A zombie still holds
/// its number, so its process group cannot be handed to anyone else yet.
pub(super) fn has_exited(pid: u32) -> bool {
    stat(pid).is_none_or(|(state, _)| matches!(state, 'Z' | 'X'))
}

/// The process group `pid` belongs to, while it runs.
pub(super) fn process_group(pid: u32) -> Option<u32> {
    stat(pid).filter(|(state, _)| !matches!(state, 'Z' | 'X')).map(|(_, group)| group)
}

/// The arguments `pid` was started with, while it runs.
pub(super) fn arguments(pid: u32) -> Option<Vec<OsString>> {
    let bytes = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    Some(
        bytes
            .split(|byte| *byte == 0)
            .filter(|arg| !arg.is_empty())
            .map(|arg| OsString::from_vec(arg.to_vec()))
            .collect(),
    )
}

/// Sends SIGKILL to every process of group `group`. Through the `kill` program, so no `unsafe`
/// call is needed; a group with nobody left in it is not an error worth reporting.
pub(super) fn kill_group(group: u32) {
    kill(&format!("-{group}"));
}

/// Sends SIGKILL to the single process `pid`.
pub(super) fn kill_process(pid: u32) {
    kill(&pid.to_string());
}

fn kill(target: &str) {
    let _ = std::process::Command::new("kill")
        .args(["-KILL", "--", target])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_runs_in_its_own_group_with_its_own_arguments() {
        let me = std::process::id();
        assert!(!has_exited(me));
        assert!(process_group(me).is_some());
        let arguments = arguments(me).unwrap();
        assert_eq!(arguments.first().map(|arg| arg.to_string_lossy().into_owned()), std::env::args().next());
    }

    #[test]
    fn a_reaped_process_has_exited_and_a_zombie_counts_as_exited() {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !has_exited(pid) {
            assert!(std::time::Instant::now() < deadline, "`true` did not finish");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(process_group(pid).is_none(), "a zombie has no group worth killing");
        child.wait().unwrap();
        assert!(has_exited(pid));
    }
}

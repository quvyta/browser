//! What `/proc` says about a process — whether it still runs, its group and its command line — and
//! how a process is ended: a signal of our own, so no program has to be installed for it to be
//! sent.

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

/// Sends SIGKILL to every process of group `group`. The signal is sent by this process, through
/// the system, rather than by running a program: `kill` is very often a shell builtin and is
/// absent on a machine without procps, and a signal that cannot be sent fails silently, which
/// would leave a Chromium behind. `rustix` sends it without an `unsafe` call, which the crate
/// forbids. A group with nobody left in it is not an error worth reporting.
pub(crate) fn kill_group(group: u32) {
    // A number no process group has is not an error worth reporting: the group is empty because
    // everything in it is already gone, which is what the caller wanted.
    if let Some(pid) = i32::try_from(group).ok().and_then(rustix::process::Pid::from_raw) {
        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
    }
}

/// Sends SIGKILL to the single process `pid`, for a Chromium that leads a group of its own.
pub(crate) fn kill_process(pid: u32) {
    // A number no process has is not an error worth reporting: the process is gone already, or
    // the number has not been given out yet.
    if let Some(pid) = i32::try_from(pid).ok().and_then(rustix::process::Pid::from_raw) {
        let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
    }
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

    /// How long a test waits for a process to be gone, and the sleep it looks at `/proc` between
    /// times: generous, so a loaded machine does not fail the test, and finite.
    const PATIENCE: std::time::Duration = std::time::Duration::from_secs(20);

    /// Waits until `/proc` says `pid` is gone, and says so if it is not.
    fn wait_until_gone(pid: u32) {
        let deadline = std::time::Instant::now() + PATIENCE;
        while !has_exited(pid) {
            assert!(std::time::Instant::now() < deadline, "process {pid} still runs");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// A `sleep 30` of ours in the process group `group`; the command only waits, so what the
    /// test watches is the signal and nothing else.
    fn waiting_child(group: u32) -> std::process::Child {
        use std::os::unix::process::CommandExt;
        let group = i32::try_from(group).unwrap();
        std::process::Command::new("sleep").arg("30").process_group(group).spawn().unwrap()
    }

    /// The group of `pid`, and `/proc` says it is running; both are asked for before a signal
    /// goes out, so a test cannot pass because a process was never there to be killed.
    fn running_in(pid: u32, group: u32) {
        assert!(!has_exited(pid), "process {pid} was gone before the signal");
        assert_eq!(process_group(pid), Some(group), "process {pid} is not in group {group}");
    }

    #[test]
    fn a_signal_reaches_a_process_group_and_later_a_helper() {
        // The leader leads a group of its own, and the helper starts after it into that same
        // group: the shape Chromium has, where renderers and the GPU and network processes are
        // started behind the browser process.
        let mut leader = waiting_child(0);
        let leader_pid = leader.id();
        let mut helper = waiting_child(leader_pid);
        let helper_pid = helper.id();
        running_in(leader_pid, leader_pid);
        running_in(helper_pid, leader_pid);
        kill_group(leader_pid);
        wait_until_gone(leader_pid);
        // A function that killed only the leader would leave this one, and the group is what
        // reaches it.
        wait_until_gone(helper_pid);
        let _ = leader.wait();
        let _ = helper.wait();
    }

    #[test]
    fn a_signal_reaches_a_single_process() {
        let mut alone = waiting_child(0);
        let pid = alone.id();
        running_in(pid, pid);
        kill_process(pid);
        wait_until_gone(pid);
        let _ = alone.wait();
        // The number is free again once the process is reaped, and a signal to a number no
        // process has is not an error worth reporting: nothing is raised and nothing is claimed.
        assert!(has_exited(pid));
        kill_process(pid);
        assert!(has_exited(pid));
    }
}

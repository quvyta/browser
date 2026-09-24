//! The profile Chromium runs on: the persistent one while this program holds its lock, a
//! temporary one while another holds it.

use std::ffi::OsString;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use super::procfs;

/// How long a killed orphan Chromium gets to disappear before starting goes on regardless.
const ORPHAN_GONE_WITHIN: Duration = Duration::from_secs(10);

/// Numbers temporary profiles made by this process apart.
static TEMPORARY: AtomicU32 = AtomicU32::new(0);

/// The profile folder Chromium runs on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// The folder handed to Chromium as its user data folder.
    pub path: PathBuf,
    /// Whether this is a temporary profile, made because another qbrowser holds the persistent
    /// one; it is removed when the engine shuts down.
    pub temporary: bool,
}

/// A profile this process may run Chromium on, and the lock that makes it ours.
pub(super) struct Claim {
    profile: Profile,
    /// The locked `profile.lock`; `None` for a temporary profile, which nobody else knows of.
    lock: Option<File>,
}

impl Claim {
    /// The profile claimed.
    pub(super) fn profile(&self) -> &Profile {
        &self.profile
    }

    /// Writes Chromium's process id into the lock file, so that the next qbrowser to take the
    /// lock can end this Chromium if this program dies without ending it.
    pub(super) fn record(&mut self, pid: u32) {
        if let Some(lock) = &mut self.lock {
            let _ = rewrite(lock, &pid.to_string());
        }
    }

    /// Forgets the recorded Chromium, lets the lock go and removes a temporary profile. Chromium
    /// must have ended before this is called.
    pub(super) fn release(&mut self) {
        if let Some(mut lock) = self.lock.take() {
            let _ = rewrite(&mut lock, "");
            // Dropping the file lets the lock go.
        }
        if self.profile.temporary {
            let _ = std::fs::remove_dir_all(&self.profile.path);
        }
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        self.release();
    }
}

/// Takes `<home>/profile` by locking `<home>/profile.lock`, or makes a temporary profile under
/// `temp_root` when another process holds that lock. Holding the lock, it first ends a Chromium
/// that a qbrowser killed without warning left running on the profile.
pub(super) fn claim(home: &Path, temp_root: &Path) -> Result<Claim, String> {
    let at = |path: &Path, error: std::io::Error| format!("{}: {error}", path.display());
    std::fs::create_dir_all(home).map_err(|error| at(home, error))?;
    let lock_path = home.join("profile.lock");
    // Never truncated on open: the id of the Chromium a dead holder left behind is in there.
    let mut lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| at(&lock_path, error))?;
    match lock.try_lock() {
        Ok(()) => {
            let path = home.join("profile");
            std::fs::create_dir_all(&path).map_err(|error| at(&path, error))?;
            end_orphan(&mut lock, &path);
            Ok(Claim { profile: Profile { path, temporary: false }, lock: Some(lock) })
        }
        // Held by another qbrowser, or a file system without locks: in both cases the persistent
        // profile may be in use, and a second Chromium on it would refuse to start.
        Err(TryLockError::WouldBlock | TryLockError::Error(_)) => {
            let path = make_temporary(temp_root)?;
            Ok(Claim { profile: Profile { path, temporary: true }, lock: None })
        }
    }
}

/// Makes a fresh `<temp_root>/qbrowser-<pid>-<n>` folder.
fn make_temporary(temp_root: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(temp_root).map_err(|error| format!("{}: {error}", temp_root.display()))?;
    loop {
        let n = TEMPORARY.fetch_add(1, Ordering::Relaxed);
        let path = temp_root.join(format!("qbrowser-{}-{n}", std::process::id()));
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            // Left behind by an earlier process that had the same id; not ours to reuse.
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("{}: {error}", path.display())),
        }
    }
}

/// Ends the Chromium whose id the lock file records, if it still runs on `profile`. Its command
/// line has to carry `--user-data-dir=<profile>`: an id reused by any other process is left
/// alone. Nothing else is ever killed.
fn end_orphan(lock: &mut File, profile: &Path) {
    let mut recorded = String::new();
    if lock.seek(SeekFrom::Start(0)).is_err() || lock.read_to_string(&mut recorded).is_err() {
        return;
    }
    let Ok(pid) = recorded.trim().parse::<u32>() else { return };
    let mut expected = OsString::from("--user-data-dir=");
    expected.push(profile);
    let runs_on_profile = procfs::arguments(pid).is_some_and(|arguments| arguments.contains(&expected));
    if procfs::has_exited(pid) || !runs_on_profile {
        return;
    }
    // qbrowser starts Chromium as the leader of its own group; its helpers go with it.
    if procfs::process_group(pid) == Some(pid) {
        procfs::kill_group(pid);
    } else {
        procfs::kill_process(pid);
    }
    let deadline = Instant::now() + ORPHAN_GONE_WITHIN;
    while !procfs::has_exited(pid) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
}

/// Replaces the lock file's contents with `text`.
fn rewrite(lock: &mut File, text: &str) -> std::io::Result<()> {
    lock.set_len(0)?;
    lock.seek(SeekFrom::Start(0))?;
    lock.write_all(text.as_bytes())?;
    lock.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tests::fixture::Scratch;

    #[test]
    fn the_first_claim_is_persistent_and_a_second_one_is_temporary_until_released() {
        let scratch = Scratch::new();
        let home = scratch.dir("home");
        let temp = scratch.path().join("temp");
        let mut first = claim(&home, &temp).unwrap();
        assert_eq!(first.profile(), &Profile { path: home.join("profile"), temporary: false });
        let mut second = claim(&home, &temp).unwrap();
        assert!(second.profile().temporary);
        assert!(second.profile().path.starts_with(&temp));
        assert!(second.profile().path.is_dir());
        let temporary = second.profile().path.clone();
        second.release();
        assert!(!temporary.exists(), "a temporary profile is removed");
        first.release();
        let third = claim(&home, &temp).unwrap();
        assert!(!third.profile().temporary, "a released lock can be taken again");
    }

    #[test]
    fn the_recorded_id_is_written_and_forgotten_on_release() {
        let scratch = Scratch::new();
        let home = scratch.dir("home");
        let mut claimed = claim(&home, &scratch.path().join("temp")).unwrap();
        claimed.record(4242);
        assert_eq!(std::fs::read_to_string(home.join("profile.lock")).unwrap(), "4242");
        claimed.release();
        assert_eq!(std::fs::read_to_string(home.join("profile.lock")).unwrap(), "");
    }

    #[test]
    fn a_recorded_process_that_does_not_run_on_the_profile_is_left_alone() {
        let scratch = Scratch::new();
        let home = scratch.dir("home");
        let mut bystander = std::process::Command::new("sleep").arg("60").spawn().unwrap();
        std::fs::write(home.join("profile.lock"), bystander.id().to_string()).unwrap();
        let claimed = claim(&home, &scratch.path().join("temp")).unwrap();
        assert!(!claimed.profile().temporary);
        let survived = bystander.try_wait().unwrap().is_none();
        let _ = bystander.kill();
        let _ = bystander.wait();
        assert!(survived, "only a process running on this profile is ever killed");
    }
}

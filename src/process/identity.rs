// @group Security : PID identity capture and verified platform-specific termination

use crate::process::instance::ProcessIdentity;
use anyhow::{anyhow, Result};
use sysinfo::{Pid, ProcessesToUpdate, System};

/// Check whether a PID is still present in the OS process table.
pub fn is_pid_alive(pid: u32) -> bool {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), false);
    system.process(Pid::from_u32(pid)).is_some()
}

/// Capture the attributes needed to distinguish a live process from a reused PID.
pub fn capture_process_identity(pid: u32) -> Option<ProcessIdentity> {
    let sysinfo_pid = Pid::from_u32(pid);
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[sysinfo_pid]), true);
    let process = system.process(sysinfo_pid)?;
    Some(ProcessIdentity {
        executable: process
            .exe()
            .map(|path| path.to_string_lossy().into_owned()),
        command_line: process
            .cmd()
            .iter()
            .map(|part| part.to_string_lossy().into_owned())
            .collect(),
        cwd: process
            .cwd()
            .map(|path| path.to_string_lossy().into_owned()),
        start_time_secs: process.start_time(),
    })
}

pub(crate) async fn capture_process_identity_with_retry(pid: u32) -> Option<ProcessIdentity> {
    for _ in 0..10 {
        if let Some(identity) = capture_process_identity(pid) {
            return Some(identity);
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
    }
    None
}

pub fn process_identity_matches(pid: u32, expected: &ProcessIdentity) -> bool {
    capture_process_identity(pid)
        .as_ref()
        .is_some_and(|current| stable_identity_matches(current, expected))
}

/// Only compare attributes that cannot legitimately change during a process
/// lifetime. cwd, argv and executable metadata may change after chdir/exec and
/// are retained for diagnostics, not ownership decisions.
pub(crate) fn stable_identity_matches(
    current: &ProcessIdentity,
    expected: &ProcessIdentity,
) -> bool {
    current.start_time_secs != 0 && current.start_time_secs == expected.start_time_secs
}

/// Kill an orphaned process only after proving the saved identity still matches.
pub async fn kill_orphan_pid(pid: u32, expected: &ProcessIdentity) -> Result<()> {
    kill_process_verified(pid, Some(expected)).await
}

pub(crate) async fn kill_process_verified(
    pid: u32,
    expected: Option<&ProcessIdentity>,
) -> Result<()> {
    let expected = expected.ok_or_else(|| {
        anyhow!("refusing to stop PID {pid}: no saved process identity is available")
    })?;
    #[cfg(target_os = "windows")]
    let process_handle = open_process_handle(pid)?;
    #[cfg(target_os = "linux")]
    let process_handle = open_pidfd(pid)?;

    if !process_identity_matches(pid, expected) {
        return Err(anyhow!(
            "refusing to stop PID {pid}: immutable process start time no longer matches"
        ));
    }

    #[cfg(target_os = "windows")]
    {
        kill_process_windows(pid, &process_handle).await
    }
    #[cfg(target_os = "linux")]
    {
        kill_process_group_linux(pid, &process_handle).await
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
    {
        kill_process_group_unix(pid, expected).await
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
async fn kill_process_group_unix(pid: u32, expected: &ProcessIdentity) -> Result<()> {
    let group =
        libc::pid_t::try_from(pid).map_err(|_| anyhow!("PID does not fit process group"))?;
    if unsafe { libc::getpgid(group) } != group {
        return Err(anyhow!(
            "refusing to stop PID {pid}: it is not the leader of its owned process group"
        ));
    }
    if !process_identity_matches(pid, expected) {
        return Err(anyhow!(
            "refusing to stop PID {pid}: immutable process start time no longer matches"
        ));
    }
    if unsafe { libc::kill(-group, libc::SIGTERM) } != 0 {
        return Err(anyhow!(
            "failed to signal process group {group}: {}",
            std::io::Error::last_os_error()
        ));
    }
    for _ in 0..30 {
        if !unix_process_group_exists(group) {
            return Ok(());
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    if is_pid_alive(pid) && !process_identity_matches(pid, expected) {
        return Err(anyhow!(
            "refusing to force-stop PID {pid}: process identity changed"
        ));
    }
    if unsafe { libc::kill(-group, libc::SIGKILL) } != 0 {
        return Err(anyhow!(
            "failed to force-stop process group {group}: {}",
            std::io::Error::last_os_error()
        ));
    }
    for _ in 0..10 {
        if !unix_process_group_exists(group) {
            return Ok(());
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    Err(anyhow!("failed to stop verified process group {group}"))
}

#[cfg(all(unix, not(target_os = "linux")))]
fn unix_process_group_exists(group: libc::pid_t) -> bool {
    (unsafe { libc::kill(-group, 0) }) == 0
        || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(target_os = "linux")]
fn open_pidfd(pid: u32) -> Result<std::os::fd::OwnedFd> {
    use std::os::fd::FromRawFd;

    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) } as i32;
    if fd < 0 {
        return Err(anyhow!(
            "cannot open a stable pidfd for PID {pid}: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) })
}

#[cfg(target_os = "linux")]
async fn kill_process_group_linux(pid: u32, pidfd: &std::os::fd::OwnedFd) -> Result<()> {
    use std::os::fd::AsRawFd;

    let group =
        libc::pid_t::try_from(pid).map_err(|_| anyhow!("PID does not fit process group"))?;
    if unsafe { libc::getpgid(group) } != group {
        return Err(anyhow!(
            "refusing to stop PID {pid}: it is not the leader of its owned process group"
        ));
    }
    let signal_group = |signal: i32| -> Result<bool> {
        if unsafe { libc::kill(-group, signal) } == 0 {
            return Ok(true);
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(false);
        }
        Err(anyhow!("failed to signal process group {group}: {error}"))
    };
    let root_exited = || {
        let mut pollfd = libc::pollfd {
            fd: pidfd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        (unsafe { libc::poll(&mut pollfd, 1, 0) }) > 0
    };
    let group_exists = || {
        if unsafe { libc::kill(-group, 0) } == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    };

    if !signal_group(libc::SIGTERM)? {
        return Ok(());
    }
    for _ in 0..30 {
        if root_exited() && !group_exists() {
            return Ok(());
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    if !signal_group(libc::SIGKILL)? {
        return Ok(());
    }
    for _ in 0..10 {
        if root_exited() && !group_exists() {
            return Ok(());
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    Err(anyhow!(
        "failed to stop verified process group {group}: descendants are still present"
    ))
}

/// Terminate a child while the caller still owns its OS child handle. Keeping
/// that handle alive prevents Windows from recycling the PID between lookup
/// and taskkill; other platforms use the child handle's direct kill method.
pub(crate) async fn kill_spawned_process(
    child: &mut tokio::process::Child,
    pid: u32,
) -> Result<()> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }

    child
        .kill()
        .await
        .map_err(|error| anyhow!("failed to stop spawned PID {pid}: {error}"))
}

#[cfg(target_os = "windows")]
struct OwnedProcessHandle(isize);

#[cfg(target_os = "windows")]
impl OwnedProcessHandle {
    fn raw(&self) -> windows::Win32::Foundation::HANDLE {
        windows::Win32::Foundation::HANDLE(self.0 as *mut std::ffi::c_void)
    }
}

#[cfg(target_os = "windows")]
impl Drop for OwnedProcessHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.raw());
        }
    }
}

#[cfg(target_os = "windows")]
fn open_process_handle(pid: u32) -> Result<OwnedProcessHandle> {
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    };

    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | PROCESS_TERMINATE,
            false,
            pid,
        )
    }
    .map_err(|error| anyhow!("cannot open a stable handle for PID {pid}: {error}"))?;
    Ok(OwnedProcessHandle(handle.0 as isize))
}

#[cfg(target_os = "windows")]
async fn kill_process_windows(pid: u32, handle: &OwnedProcessHandle) -> Result<()> {
    use windows::Win32::Foundation::WAIT_OBJECT_0;
    use windows::Win32::System::Threading::{TerminateProcess, WaitForSingleObject};

    unsafe { TerminateProcess(handle.raw(), 1) }
        .map_err(|error| anyhow!("failed to terminate verified PID {pid}: {error}"))?;

    for _ in 0..30 {
        if unsafe { WaitForSingleObject(handle.raw(), 0) } == WAIT_OBJECT_0 {
            return Ok(());
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    Err(anyhow!(
        "failed to stop verified PID {pid}: process is still alive"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_and_matches_current_process_identity() {
        let pid = std::process::id();
        let identity = capture_process_identity(pid).expect("current process must be visible");
        assert!(process_identity_matches(pid, &identity));

        let mut stale = identity;
        stale.start_time_secs = stale.start_time_secs.saturating_add(1);
        assert!(!process_identity_matches(pid, &stale));
    }

    #[test]
    fn mutable_process_metadata_is_not_part_of_stable_identity() {
        let expected = ProcessIdentity {
            executable: Some("old.exe".to_string()),
            command_line: vec!["old".to_string()],
            cwd: Some("old".to_string()),
            start_time_secs: 42,
        };
        let current = ProcessIdentity {
            executable: Some("new.exe".to_string()),
            command_line: vec!["new".to_string()],
            cwd: Some("new".to_string()),
            start_time_secs: 42,
        };
        assert!(stable_identity_matches(&current, &expected));
    }
}

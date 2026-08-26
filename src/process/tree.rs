// @group BusinessLogic > ProcessTree : Cross-platform ownership boundary for spawned process trees

/// Owns a platform process-tree boundary. Short-lived Unix operations terminate
/// the group on drop; daemon-owned groups explicitly opt into crash survival and
/// remain terminable through `terminate_and_wait`.
pub struct ProcessTreeGuard {
    #[cfg(windows)]
    job: isize,
    #[cfg(unix)]
    process_group: libc::pid_t,
    #[cfg(unix)]
    leader_start_time_secs: u64,
    #[cfg(unix)]
    terminate_on_drop: bool,
}

impl ProcessTreeGuard {
    #[cfg(windows)]
    pub fn new(pid: u32, owner: &str) -> anyhow::Result<Self> {
        use anyhow::Context;
        use sha2::{Digest, Sha256};
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{CloseHandle, BOOL};
        use windows::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
            JobObjectExtendedLimitInformation, SetInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
        };

        let owner_hash = Sha256::digest(owner.as_bytes());
        let job_name = format!("Local\\RunDock-ProcessTree-{owner_hash:x}");
        let job_name_wide = job_name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let job = unsafe { CreateJobObjectW(None, PCWSTR(job_name_wide.as_ptr())) }
            .context("failed to create or open named process-tree job")?;
        let configure = (|| -> anyhow::Result<()> {
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            unsafe {
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    std::ptr::from_ref(&limits).cast(),
                    std::mem::size_of_val(&limits) as u32,
                )
                .context("failed to configure named process-tree job")?;
            }
            let process = unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA | PROCESS_TERMINATE,
                    false,
                    pid,
                )
            }
            .context("failed to open process for tree assignment")?;
            let mut already_assigned = BOOL::default();
            let membership = unsafe { IsProcessInJob(process, job, &mut already_assigned) };
            let assignment = membership
                .context("failed to query named process-tree membership")
                .and_then(|_| {
                    if already_assigned.as_bool() {
                        Ok(())
                    } else {
                        unsafe { AssignProcessToJobObject(job, process) }
                            .context("failed to assign process to named process-tree job")
                    }
                });
            unsafe {
                let _ = CloseHandle(process);
            }
            assignment?;
            Ok(())
        })();
        if let Err(error) = configure {
            unsafe {
                let _ = CloseHandle(job);
            }
            return Err(error);
        }
        Ok(Self {
            job: job.0 as isize,
        })
    }

    #[cfg(unix)]
    pub fn new(pid: u32, _owner: &str) -> anyhow::Result<Self> {
        let process_group = libc::pid_t::try_from(pid)
            .map_err(|_| anyhow::anyhow!("PID does not fit a process-group id"))?;
        let actual_group = unsafe { libc::getpgid(process_group) };
        anyhow::ensure!(
            actual_group == process_group,
            "process {pid} is not the leader of its owned process group"
        );
        let identity = crate::process::identity::capture_process_identity(pid)
            .ok_or_else(|| anyhow::anyhow!("process-group leader identity is unavailable"))?;
        anyhow::ensure!(
            identity.start_time_secs != 0,
            "process-group leader start time is unavailable"
        );
        Ok(Self {
            process_group,
            leader_start_time_secs: identity.start_time_secs,
            terminate_on_drop: true,
        })
    }

    /// Keep a daemon-owned Unix process group alive if the daemon unwinds or
    /// crashes. Explicit stop/delete paths still use `terminate_and_wait`.
    pub fn preserve_on_drop(&mut self) {
        #[cfg(unix)]
        {
            self.terminate_on_drop = false;
        }
    }

    #[cfg(unix)]
    fn owned_unix_group_exists(&self) -> anyhow::Result<bool> {
        if self.process_group <= 0 {
            return Ok(false);
        }
        let group_exists = unsafe { libc::kill(-self.process_group, 0) } == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
        if !group_exists {
            return Ok(false);
        }
        let leader_pid = u32::try_from(self.process_group)
            .map_err(|_| anyhow::anyhow!("process-group id does not fit a PID"))?;
        if crate::process::identity::is_pid_alive(leader_pid) {
            let identity = crate::process::identity::capture_process_identity(leader_pid)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "refusing to signal Unix process group: live leader identity is unavailable"
                    )
                })?;
            anyhow::ensure!(
                identity.start_time_secs != 0
                    && identity.start_time_secs == self.leader_start_time_secs
                    && unsafe { libc::getpgid(self.process_group) } == self.process_group,
                "refusing to signal an unverified or recycled Unix process group"
            );
        }
        // When the leader is gone but the group still exists, its remaining
        // members keep the group id allocated; this is the owned descendant set.
        Ok(true)
    }

    /// Transfer Unix process-group ownership to a verified replacement daemon.
    /// The replacement must already hold its own guard before this is called.
    #[cfg(unix)]
    pub fn relinquish_without_termination(&mut self) {
        self.process_group = 0;
        self.leader_start_time_secs = 0;
    }

    /// Terminate the complete owned tree and wait until the OS confirms that
    /// no member remains. This is the recovery path when root-PID based
    /// termination loses the leader before all descendants have exited.
    #[cfg(windows)]
    pub async fn terminate_and_wait(&self) -> anyhow::Result<()> {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::JobObjects::{
            JobObjectBasicAccountingInformation, QueryInformationJobObject, TerminateJobObject,
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
        };

        {
            let job = HANDLE(self.job as *mut std::ffi::c_void);
            unsafe { TerminateJobObject(job, 1) }.map_err(|error| {
                anyhow::anyhow!("failed to terminate owned process job: {error}")
            })?;
        }
        for _ in 0..50 {
            let active_processes = {
                let job = HANDLE(self.job as *mut std::ffi::c_void);
                let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
                unsafe {
                    QueryInformationJobObject(
                        job,
                        JobObjectBasicAccountingInformation,
                        std::ptr::from_mut(&mut accounting).cast(),
                        std::mem::size_of_val(&accounting) as u32,
                        None,
                    )
                }
                .map_err(|error| anyhow::anyhow!("failed to inspect owned process job: {error}"))?;
                accounting.ActiveProcesses
            };
            if active_processes == 0 {
                return Ok(());
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
        anyhow::bail!("owned process job still contains active descendants after termination")
    }

    #[cfg(unix)]
    pub async fn terminate_and_wait(&self) -> anyhow::Result<()> {
        if !self.owned_unix_group_exists()? {
            return Ok(());
        }
        let signal_result = unsafe { libc::kill(-self.process_group, libc::SIGKILL) };
        if signal_result != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                return Ok(());
            }
            return Err(anyhow::anyhow!(
                "failed to terminate owned process group {}: {error}",
                self.process_group
            ));
        }
        for _ in 0..50 {
            let exists = unsafe { libc::kill(-self.process_group, 0) } == 0
                || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
            if !exists {
                return Ok(());
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
        anyhow::bail!(
            "owned process group {} still contains descendants after termination",
            self.process_group
        )
    }
}

impl Drop for ProcessTreeGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            use windows::Win32::Foundation::CloseHandle;
            let job = windows::Win32::Foundation::HANDLE(self.job as *mut std::ffi::c_void);
            // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE terminates the tree only when
            // the final handle closes. A replacement daemon can therefore
            // open the same named job before the old daemon exits and preserve
            // managed children across a verified handoff.
            let _ = CloseHandle(job);
        }
        #[cfg(unix)]
        {
            if self.process_group <= 0 || !self.terminate_on_drop {
                return;
            }
            match self.owned_unix_group_exists() {
                Ok(true) => unsafe {
                    let _ = libc::kill(-self.process_group, libc::SIGKILL);
                },
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(
                        process_group = self.process_group,
                        %error,
                        "refusing to signal an unverified or recycled Unix process group"
                    );
                }
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::ProcessTreeGuard;
    use std::os::unix::process::CommandExt;

    #[test]
    fn relinquished_unix_guard_does_not_kill_the_transferred_group() {
        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 30")
            .process_group(0)
            .spawn()
            .unwrap();
        let mut guard = ProcessTreeGuard::new(child.id(), "test-handoff").unwrap();

        guard.relinquish_without_termination();
        drop(guard);

        assert!(child.try_wait().unwrap().is_none());
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn preserved_unix_guard_does_not_kill_the_owned_group() {
        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 30")
            .process_group(0)
            .spawn()
            .unwrap();
        let mut guard = ProcessTreeGuard::new(child.id(), "test-crash-survival").unwrap();

        guard.preserve_on_drop();
        drop(guard);

        assert!(child.try_wait().unwrap().is_none());
        child.kill().unwrap();
        child.wait().unwrap();
    }
}

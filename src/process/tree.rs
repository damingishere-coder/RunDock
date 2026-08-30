// @group BusinessLogic > ProcessTree : Cross-platform ownership boundary for spawned process trees

/// Owns a platform process-tree boundary. Short-lived Unix operations terminate
/// the group on drop; daemon-owned groups explicitly opt into crash survival and
/// remain terminable through `terminate_and_wait`.
#[derive(Debug)]
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
    /// Attach the newly spawned child to an owned process-tree boundary. If the
    /// boundary cannot be established, synchronously clean up the complete
    /// tree before returning the attachment error.
    pub async fn attach_or_terminate(
        child: &mut tokio::process::Child,
        pid: u32,
        owner: &str,
    ) -> anyhow::Result<Self> {
        Self::attach_or_terminate_with(child, pid, owner, Self::new).await
    }

    /// Terminate a live tree that could not be attached during daemon
    /// recovery. The immutable process identity is checked before any signal
    /// or Windows descendant snapshot is used.
    pub async fn terminate_unowned_existing(
        pid: u32,
        expected: &crate::process::instance::ProcessIdentity,
    ) -> anyhow::Result<()> {
        #[cfg(windows)]
        {
            let expected = expected.clone();
            tokio::task::spawn_blocking(move || {
                cleanup_unowned_existing_windows_tree(pid, &expected)
            })
            .await
            .map_err(|error| anyhow::anyhow!("Windows fallback cleanup task failed: {error}"))??;
            Ok(())
        }
        #[cfg(not(windows))]
        {
            crate::process::identity::kill_process_verified(pid, Some(expected)).await
        }
    }

    async fn attach_or_terminate_with<F>(
        child: &mut tokio::process::Child,
        pid: u32,
        owner: &str,
        attach: F,
    ) -> anyhow::Result<Self>
    where
        F: FnOnce(u32, &str) -> anyhow::Result<Self>,
    {
        match attach(pid, owner) {
            Ok(guard) => Ok(guard),
            Err(attach_error) => {
                if let Err(cleanup_error) = cleanup_unowned_spawned_tree(child, pid).await {
                    return Err(anyhow::anyhow!(
                        "failed to establish process-tree ownership: {attach_error}; fallback tree cleanup could not be confirmed: {cleanup_error}"
                    ));
                }
                Err(attach_error).map_err(|error| {
                    error.context(
                        "failed to establish process-tree ownership; spawned tree was terminated",
                    )
                })
            }
        }
    }
}

#[cfg(unix)]
async fn cleanup_unowned_spawned_tree(
    child: &mut tokio::process::Child,
    pid: u32,
) -> anyhow::Result<()> {
    let process_group = libc::pid_t::try_from(pid)
        .map_err(|_| anyhow::anyhow!("PID does not fit a process-group id"))?;
    let actual_group = unsafe { libc::getpgid(process_group) };
    anyhow::ensure!(
        actual_group == process_group,
        "refusing fallback cleanup because process {pid} is not its process-group leader"
    );

    let signal_result = unsafe { libc::kill(-process_group, libc::SIGKILL) };
    if signal_result != 0 {
        let error = std::io::Error::last_os_error();
        anyhow::ensure!(
            error.raw_os_error() == Some(libc::ESRCH),
            "failed to terminate fallback process group {process_group}: {error}"
        );
    }
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
        .await
        .map_err(|_| {
            anyhow::anyhow!("fallback process-group leader did not exit within 5 seconds")
        })??;
    for _ in 0..50 {
        let exists = unsafe { libc::kill(-process_group, 0) } == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
        if !exists {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    anyhow::bail!(
        "fallback process group {process_group} still contains descendants after termination"
    )
}

#[cfg(windows)]
fn windows_process_snapshot() -> anyhow::Result<Vec<(u32, u32)>> {
    use anyhow::Context;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .context("failed to snapshot Windows processes")?;
    let result = (|| -> anyhow::Result<Vec<(u32, u32)>> {
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut processes = Vec::new();
        unsafe { Process32FirstW(snapshot, &mut entry) }
            .context("failed to enumerate the first Windows process")?;
        loop {
            processes.push((entry.th32ProcessID, entry.th32ParentProcessID));
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            if let Err(error) = unsafe { Process32NextW(snapshot, &mut entry) } {
                if error.code().0 == 0x8007_0012_u32 as i32 {
                    break;
                }
                return Err(error).context("Windows process enumeration failed before completion");
            }
        }
        Ok(processes)
    })();
    unsafe {
        let _ = CloseHandle(snapshot);
    }
    result
}

#[cfg(windows)]
fn windows_descendants(root_pid: u32, processes: &[(u32, u32)]) -> std::collections::HashSet<u32> {
    let mut owned = std::collections::HashSet::from([root_pid]);
    loop {
        let before = owned.len();
        for &(pid, parent) in processes {
            if owned.contains(&parent) {
                owned.insert(pid);
            }
        }
        if owned.len() == before {
            return owned;
        }
    }
}

#[cfg(windows)]
fn suspend_windows_tree_threads(
    owned: &std::collections::HashSet<u32>,
) -> anyhow::Result<Vec<windows::Win32::Foundation::HANDLE>> {
    use anyhow::Context;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows::Win32::System::Threading::{OpenThread, SuspendThread, THREAD_SUSPEND_RESUME};

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }
        .context("failed to snapshot Windows threads")?;
    let mut suspended = Vec::new();
    let result = (|| -> anyhow::Result<()> {
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        unsafe { Thread32First(snapshot, &mut entry) }
            .context("failed to enumerate the first Windows thread")?;
        loop {
            if owned.contains(&entry.th32OwnerProcessID) {
                let thread =
                    unsafe { OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID) }
                        .with_context(|| {
                            format!(
                                "failed to open thread {} for suspension",
                                entry.th32ThreadID
                            )
                        })?;
                if unsafe { SuspendThread(thread) } == u32::MAX {
                    unsafe {
                        let _ = CloseHandle(thread);
                    }
                    anyhow::bail!("failed to suspend thread {}", entry.th32ThreadID);
                }
                suspended.push(thread);
            }
            entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            if let Err(error) = unsafe { Thread32Next(snapshot, &mut entry) } {
                if error.code().0 == 0x8007_0012_u32 as i32 {
                    break;
                }
                return Err(error).context("Windows thread enumeration failed before completion");
            }
        }
        Ok(())
    })();
    unsafe {
        let _ = CloseHandle(snapshot);
    }
    if result.is_err() {
        use windows::Win32::System::Threading::ResumeThread;
        for thread in suspended.drain(..) {
            unsafe {
                let _ = ResumeThread(thread);
                let _ = CloseHandle(thread);
            }
        }
        result?;
    }
    Ok(suspended)
}

#[cfg(windows)]
fn cleanup_unowned_spawned_windows_tree(pid: u32) -> anyhow::Result<()> {
    use anyhow::Context;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{
        OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    };

    let mut suspended_threads = Vec::new();
    let mut owned = std::collections::HashSet::from([pid]);
    let containment = (|| -> anyhow::Result<()> {
        for _ in 0..4 {
            let snapshot = windows_process_snapshot()?;
            let discovered = windows_descendants(pid, &snapshot);
            let new_owned = discovered.difference(&owned).copied().collect::<Vec<_>>();
            owned.extend(discovered);
            let newly_suspended = suspend_windows_tree_threads(&owned)?;
            suspended_threads.extend(newly_suspended);
            if new_owned.is_empty() {
                break;
            }
        }
        let final_snapshot = windows_process_snapshot()?;
        let final_owned = windows_descendants(pid, &final_snapshot);
        anyhow::ensure!(
            final_owned.is_subset(&owned),
            "Windows process tree kept spawning descendants while containment was being established"
        );
        Ok(())
    })();
    if let Err(error) = containment {
        use windows::Win32::System::Threading::ResumeThread;
        for thread in suspended_threads {
            unsafe {
                let _ = ResumeThread(thread);
                let _ = CloseHandle(thread);
            }
        }
        return Err(error);
    }

    let mut process_handles: Vec<(u32, HANDLE)> = Vec::new();
    let mut open_failures = Vec::new();
    for owned_pid in &owned {
        match unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, *owned_pid) } {
            Ok(handle) => process_handles.push((*owned_pid, handle)),
            Err(error) => open_failures.push(format!("PID {owned_pid}: {error}")),
        }
    }

    let cleanup = (|| -> anyhow::Result<()> {
        process_handles.sort_by_key(|(owned_pid, _)| *owned_pid == pid);
        for &(owned_pid, handle) in &process_handles {
            if unsafe { WaitForSingleObject(handle, 0) } != WAIT_OBJECT_0 {
                unsafe { TerminateProcess(handle, 1) }
                    .with_context(|| format!("failed to terminate fallback process {owned_pid}"))?;
            }
        }
        for &(owned_pid, handle) in &process_handles {
            anyhow::ensure!(
                unsafe { WaitForSingleObject(handle, 5_000) } == WAIT_OBJECT_0,
                "fallback process {owned_pid} did not exit within 5 seconds"
            );
        }
        Ok(())
    })();

    for (_, handle) in process_handles {
        unsafe {
            let _ = CloseHandle(handle);
        }
    }
    for thread in suspended_threads {
        unsafe {
            let _ = CloseHandle(thread);
        }
    }
    cleanup?;
    anyhow::ensure!(
        open_failures.is_empty(),
        "could not open every fallback process with stable termination handles: {}",
        open_failures.join(", ")
    );
    for _ in 0..50 {
        let live_pids = windows_process_snapshot()?
            .into_iter()
            .map(|(live_pid, _)| live_pid)
            .filter(|live_pid| owned.contains(live_pid))
            .collect::<Vec<_>>();
        if live_pids.is_empty() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    anyhow::bail!("fallback Windows process tree still contains a known root or descendant")
}

#[cfg(windows)]
fn cleanup_unowned_existing_windows_tree(
    pid: u32,
    expected: &crate::process::instance::ProcessIdentity,
) -> anyhow::Result<()> {
    use anyhow::Context;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    };

    // Hold a stable handle before checking the PID identity so a root exit and
    // PID reuse cannot redirect the subsequent descendant cleanup.
    let root = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | PROCESS_TERMINATE,
            false,
            pid,
        )
    }
    .context("failed to open adopted root with a stable cleanup handle")?;
    let cleanup = (|| -> anyhow::Result<()> {
        anyhow::ensure!(
            crate::process::identity::process_identity_matches(pid, expected),
            "refusing fallback tree cleanup for PID {pid}: immutable process identity no longer matches"
        );
        cleanup_unowned_spawned_windows_tree(pid)
    })();
    unsafe {
        let _ = CloseHandle(root);
    }
    cleanup
}

#[cfg(windows)]
async fn cleanup_unowned_spawned_tree(
    child: &mut tokio::process::Child,
    pid: u32,
) -> anyhow::Result<()> {
    let cleanup = cleanup_unowned_spawned_windows_tree(pid);
    let reap = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
        .await
        .map_err(|_| anyhow::anyhow!("fallback root process did not reap within 5 seconds"))?;
    match (cleanup, reap) {
        (Ok(()), Ok(_)) => Ok(()),
        (Err(cleanup_error), Ok(_)) => Err(cleanup_error),
        (Ok(()), Err(reap_error)) => Err(reap_error.into()),
        (Err(cleanup_error), Err(reap_error)) => Err(anyhow::anyhow!(
            "{cleanup_error}; root process reap also failed: {reap_error}"
        )),
    }
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
            JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
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
            unsafe {
                QueryInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    std::ptr::from_mut(&mut limits).cast(),
                    std::mem::size_of_val(&limits) as u32,
                    None,
                )
                .context("failed to inspect named process-tree job")?;
            }
            limits.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
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

    /// Keep a daemon-owned process tree alive if the daemon unwinds, exits, or
    /// crashes. Explicit stop/delete paths still use `terminate_and_wait`.
    pub fn preserve_on_drop(&mut self) -> anyhow::Result<()> {
        #[cfg(windows)]
        {
            use anyhow::Context;
            use windows::Win32::Foundation::HANDLE;
            use windows::Win32::System::JobObjects::{
                JobObjectExtendedLimitInformation, QueryInformationJobObject,
                SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            };

            let job = HANDLE(self.job as *mut std::ffi::c_void);
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            unsafe {
                QueryInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    std::ptr::from_mut(&mut limits).cast(),
                    std::mem::size_of_val(&limits) as u32,
                    None,
                )
            }
            .context("failed to inspect managed process tree preservation flags")?;
            limits.BasicLimitInformation.LimitFlags &= !JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            unsafe {
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    std::ptr::from_ref(&limits).cast(),
                    std::mem::size_of_val(&limits) as u32,
                )
            }
            .context("failed to preserve managed process tree after daemon exit")?;
        }
        #[cfg(unix)]
        {
            self.terminate_on_drop = false;
        }
        Ok(())
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

        guard.preserve_on_drop().unwrap();
        drop(guard);

        assert!(child.try_wait().unwrap().is_none());
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[tokio::test]
    async fn failed_attachment_terminates_the_spawned_unix_group() {
        let mut command = tokio::process::Command::new("sh");
        command.arg("-c").arg("sleep 30 & wait");
        command.as_std_mut().process_group(0);
        let mut child = command.spawn().unwrap();
        let pid = child.id().unwrap();

        let error = ProcessTreeGuard::attach_or_terminate_with(
            &mut child,
            pid,
            "forced-attach-failure",
            |_, _| anyhow::bail!("forced attachment failure"),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("spawned tree was terminated"));
        assert!(child.try_wait().unwrap().is_some());
        let group = libc::pid_t::try_from(pid).unwrap();
        assert_ne!(unsafe { libc::kill(-group, 0) }, 0);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::ProcessTreeGuard;
    use std::os::windows::process::CommandExt;

    #[test]
    fn preserved_windows_guard_does_not_kill_the_owned_job() {
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let owner = format!("test-crash-survival-{}", uuid::Uuid::new_v4());
        let spawn = |flags| {
            std::process::Command::new("powershell.exe")
                .args([
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "Start-Sleep -Seconds 30",
                ])
                .creation_flags(flags)
                .spawn()
        };
        let mut child =
            match spawn(CREATE_BREAKAWAY_FROM_JOB | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW) {
                Ok(child) => child,
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                    spawn(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW).unwrap()
                }
                Err(error) => panic!("failed to spawn Windows preservation test child: {error}"),
            };
        let mut guard = ProcessTreeGuard::new(child.id(), &owner).unwrap();

        guard.preserve_on_drop().unwrap();
        drop(guard);

        assert!(child.try_wait().unwrap().is_none());
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[tokio::test]
    async fn failed_attachment_terminates_the_spawned_windows_tree() {
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let spawn = |flags| {
            let mut command = tokio::process::Command::new("powershell.exe");
            command.args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$child = Start-Process powershell.exe -ArgumentList '-NoLogo -NoProfile -NonInteractive -Command Start-Sleep -Seconds 30' -PassThru; Start-Sleep -Seconds 30",
            ]);
            command.creation_flags(flags);
            command.spawn()
        };
        let mut child =
            match spawn(CREATE_BREAKAWAY_FROM_JOB | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW) {
                Ok(child) => child,
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                    spawn(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW).unwrap()
                }
                Err(error) => panic!("failed to spawn Windows fallback-cleanup test: {error}"),
            };
        let pid = child.id().unwrap();
        let mut observed = std::collections::HashSet::new();
        for _ in 0..30 {
            observed = super::windows_descendants(pid, &super::windows_process_snapshot().unwrap());
            if observed.len() > 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(
            observed.len() > 1,
            "test process did not create a descendant"
        );

        let error = ProcessTreeGuard::attach_or_terminate_with(
            &mut child,
            pid,
            "forced-attach-failure",
            |_, _| anyhow::bail!("forced attachment failure"),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("spawned tree was terminated"));
        assert!(child.try_wait().unwrap().is_some());
        for owned_pid in observed {
            assert!(
                !crate::process::identity::is_pid_alive(owned_pid),
                "fallback cleanup left PID {owned_pid} alive"
            );
        }
    }

    #[tokio::test]
    async fn adopted_fallback_verifies_identity_and_terminates_descendants() {
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let spawn = |flags| {
            let mut command = tokio::process::Command::new("powershell.exe");
            command.args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$child = Start-Process powershell.exe -ArgumentList '-NoLogo -NoProfile -NonInteractive -Command Start-Sleep -Seconds 30' -PassThru; Start-Sleep -Seconds 30",
            ]);
            command.creation_flags(flags);
            command.spawn()
        };
        let mut child =
            match spawn(CREATE_BREAKAWAY_FROM_JOB | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW) {
                Ok(child) => child,
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                    spawn(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW).unwrap()
                }
                Err(error) => panic!("failed to spawn adopted fallback test tree: {error}"),
            };
        let pid = child.id().unwrap();
        let identity = crate::process::identity::capture_process_identity_with_retry(pid)
            .await
            .expect("test root identity was unavailable");
        let mut observed = std::collections::HashSet::new();
        for _ in 0..30 {
            observed = super::windows_descendants(pid, &super::windows_process_snapshot().unwrap());
            if observed.len() > 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(
            observed.len() > 1,
            "test process did not create a descendant"
        );

        let mut mismatched = identity.clone();
        mismatched.start_time_secs = mismatched.start_time_secs.saturating_add(1);
        let refused = ProcessTreeGuard::terminate_unowned_existing(pid, &mismatched)
            .await
            .unwrap_err();
        assert!(refused.to_string().contains("identity no longer matches"));
        assert!(crate::process::identity::is_pid_alive(pid));

        ProcessTreeGuard::terminate_unowned_existing(pid, &identity)
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
            .await
            .expect("fallback root did not reap")
            .unwrap();
        for owned_pid in observed {
            assert!(
                !crate::process::identity::is_pid_alive(owned_pid),
                "adopted fallback cleanup left PID {owned_pid} alive"
            );
        }
    }
}

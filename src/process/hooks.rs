// @group BusinessLogic > Hooks : Pre/post lifecycle hook executor

use crate::process::tree::ProcessTreeGuard;
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::process::Stdio;

// @group BusinessLogic > Hooks : Execute a shell hook command
/// Runs via `cmd /C` on Windows, `sh -c` on Unix.
/// Returns Ok(()) if exit code is 0, Err otherwise.
pub async fn run_hook(
    hook_cmd: &str,
    cwd: Option<&str>,
    env: &HashMap<String, String>,
) -> Result<()> {
    // Hook commands and their output may contain credentials. Keep lifecycle
    // logs useful without copying command text or child output into daemon logs.
    tracing::info!("running lifecycle hook");

    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(hook_cmd);
        c
    };

    #[cfg(not(target_os = "windows"))]
    let mut cmd = {
        let mut c = tokio::process::Command::new("sh");
        c.arg("-c").arg(hook_cmd);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            c.as_std_mut().process_group(0);
        }
        c
    };

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    // Hooks are control-plane commands, not log producers. Discard their output
    // so an untrusted hook cannot fill daemon memory before the timeout.
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    cmd.kill_on_drop(false);

    // Suppress console window on Windows
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000 | 0x01000000); // CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB
    }

    let mut child = cmd.spawn()?;
    let pid = child
        .id()
        .ok_or_else(|| anyhow::anyhow!("lifecycle hook did not expose a PID"))?;
    let process_tree =
        ProcessTreeGuard::attach_or_terminate(&mut child, pid, &format!("hook-{pid}"))
            .await
            .context("failed to contain lifecycle hook process tree")?;
    let status = match tokio::time::timeout(std::time::Duration::from_secs(60), child.wait()).await
    {
        Ok(result) => result?,
        Err(_) => {
            drop(process_tree);
            let cleanup = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                if child.try_wait()?.is_none() {
                    child.kill().await?;
                }
                child.wait().await
            })
            .await;
            match cleanup {
                Ok(Ok(_)) => bail!("lifecycle hook timed out after 60 seconds"),
                Ok(Err(error)) => bail!(
                    "lifecycle hook timed out after 60 seconds and cleanup failed: {error}"
                ),
                Err(_) => bail!(
                    "lifecycle hook timed out after 60 seconds and cleanup was not confirmed within 5 seconds"
                ),
            }
        }
    };
    drop(process_tree);
    let exit_code = status.code();

    if !status.success() {
        bail!("lifecycle hook failed with exit code: {exit_code:?}");
    }

    Ok(())
}

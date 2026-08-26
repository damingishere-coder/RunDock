// @group BusinessLogic : `alter startup` / `alter unstartup` command handlers
// Generates OS startup scripts to auto-start the daemon on boot

use anyhow::Result;

pub async fn run_startup() -> Result<()> {
    let exe = std::env::current_exe()?.to_string_lossy().to_string();

    #[cfg(target_os = "windows")]
    {
        let escaped_exe = exe.replace('\'', "''");
        println!("[alter] To auto-start the daemon on Windows login, run this in PowerShell (as Administrator):");
        println!();
        println!(
            "  $action = New-ScheduledTaskAction -Execute '{escaped_exe}' -Argument 'daemon start'"
        );
        println!(r#"  $trigger = New-ScheduledTaskTrigger -AtLogon"#);
        println!(
            r#"  Register-ScheduledTask -TaskName "alter-daemon" -Action $action -Trigger $trigger -RunLevel Highest"#
        );
    }

    #[cfg(target_os = "linux")]
    {
        if exe.chars().any(char::is_control) {
            anyhow::bail!("executable path contains unsupported control characters");
        }
        let escaped_exe = exe
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%");
        let user = std::env::var("USER").unwrap_or_else(|_| "root".to_string());
        anyhow::ensure!(
            !user.is_empty()
                && user.len() <= 256
                && user
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric()
                        || matches!(character, '_' | '-' | '.')),
            "USER is not a valid systemd account name"
        );
        let unit = format!(
            r#"[Unit]
Description=RunDock process manager daemon (alter CLI compatibility service)
After=network.target

[Service]
Type=simple
ExecStart="{exe}" --internal-daemon
Restart=on-failure
User={user}

[Install]
WantedBy=multi-user.target
"#,
            exe = escaped_exe,
            user = user,
        );

        let path = "/etc/systemd/system/alter-daemon.service";
        println!("[alter] Suggested systemd unit for {path} (not written automatically):");
        println!(
            "[alter] Run: sudo systemctl enable alter-daemon && sudo systemctl start alter-daemon"
        );
        println!();
        println!("{unit}");
    }

    #[cfg(target_os = "macos")]
    {
        anyhow::bail!(
            "macOS startup registration is not implemented; no launchd configuration was changed (run `{exe} daemon start` manually)"
        );
    }

    #[cfg(not(target_os = "macos"))]
    Ok(())
}

pub async fn run_unstartup() -> Result<()> {
    #[cfg(target_os = "macos")]
    anyhow::bail!("macOS startup removal is not implemented; no launchd configuration was changed");

    #[cfg(target_os = "windows")]
    {
        println!("[alter] To remove startup task, run in PowerShell (as Administrator):");
        println!(r#"  Unregister-ScheduledTask -TaskName "alter-daemon" -Confirm:$false"#);
    }

    #[cfg(target_os = "linux")]
    {
        println!("[alter] To remove systemd unit:");
        println!("  sudo systemctl disable alter-daemon");
        println!("  sudo rm /etc/systemd/system/alter-daemon.service");
    }

    #[cfg(not(target_os = "macos"))]
    Ok(())
}

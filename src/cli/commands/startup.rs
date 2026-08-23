// @group BusinessLogic : `alter startup` / `alter unstartup` command handlers
// Generates OS startup scripts to auto-start the daemon on boot

use anyhow::Result;

pub async fn run_startup() -> Result<()> {
    let exe = std::env::current_exe()?
        .to_string_lossy()
        .to_string();

    #[cfg(target_os = "windows")]
    {
        println!("[alter] To auto-start the daemon on Windows login, run this in PowerShell (as Administrator):");
        println!();
        println!(r#"  $action = New-ScheduledTaskAction -Execute "{exe}" -Argument "daemon start""#);
        println!(r#"  $trigger = New-ScheduledTaskTrigger -AtLogon"#);
        println!(r#"  Register-ScheduledTask -TaskName "alter-daemon" -Action $action -Trigger $trigger -RunLevel Highest"#);
    }

    #[cfg(target_os = "linux")]
    {
        let unit = format!(
r#"[Unit]
Description=RunDock process manager daemon (alter CLI compatibility service)
After=network.target

[Service]
Type=forking
ExecStart={exe} --internal-daemon
Restart=on-failure
User={user}

[Install]
WantedBy=multi-user.target
"#,
            exe = exe,
            user = std::env::var("USER").unwrap_or_else(|_| "root".to_string()),
        );

        let path = "/etc/systemd/system/alter-daemon.service";
        println!("[alter] Writing systemd unit to {path}");
        println!("[alter] Run: sudo systemctl enable alter-daemon && sudo systemctl start alter-daemon");
        println!();
        println!("{unit}");
    }

    #[cfg(target_os = "macos")]
    {
        println!("[alter] macOS launchd support coming soon. For now, add this to your shell profile:");
        println!("  {exe} daemon start");
    }

    Ok(())
}

pub async fn run_unstartup() -> Result<()> {
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

    Ok(())
}

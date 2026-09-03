// @group Utilities : Terminal table renderer for `alter list`

use crate::models::process_info::ProcessInfo;
use crate::utils::format::{format_uptime, status_color, BOLD, DIM, RESET};
use chrono::Local;

pub fn render_process_table(processes: &[ProcessInfo]) {
    if processes.is_empty() {
        println!("{DIM}No processes running.{RESET}");
        return;
    }

    // Determine if any process has a cron schedule (affects column layout)
    let has_cron = processes.iter().any(|p| p.cron.is_some());

    // Column widths
    let name_w = processes
        .iter()
        .map(|p| p.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let id_w = 8; // first 8 chars of UUID
    let mode_w = 5; // "cron" / "watch" / "-"

    if has_cron {
        println!(
            "{BOLD}{:<id_w$}  {:<name_w$}  {:<10}  {:<6}  {:<8}  {:<8}  {:<mode_w$}  {:<16}{RESET}",
            "ID",
            "NAME",
            "STATUS",
            "PID",
            "UPTIME",
            "RESTARTS",
            "MODE",
            "NEXT RUN",
            id_w = id_w,
            name_w = name_w,
            mode_w = mode_w,
        );
        println!("{}", "-".repeat(id_w + name_w + 78));
    } else {
        println!(
            "{BOLD}{:<id_w$}  {:<name_w$}  {:<10}  {:<6}  {:<8}  {:<8}  {:<mode_w$}{RESET}",
            "ID",
            "NAME",
            "STATUS",
            "PID",
            "UPTIME",
            "RESTARTS",
            "MODE",
            id_w = id_w,
            name_w = name_w,
            mode_w = mode_w,
        );
        println!("{}", "-".repeat(id_w + name_w + 60));
    }

    for p in processes {
        let id_short = p.id.to_string()[..8].to_string();
        let status_str = p.status.to_string();
        let color = status_color(&status_str);
        let pid_str = p
            .pid
            .map(|n| n.to_string())
            .unwrap_or_else(|| "-".to_string());
        let uptime_str = p
            .uptime_secs
            .map(format_uptime)
            .unwrap_or_else(|| "-".to_string());

        let mode_str = if p.cron.is_some() {
            "cron"
        } else if p.watch {
            "watch"
        } else {
            "-"
        };

        if has_cron {
            let next_run_str = p
                .cron_next_run
                .map(|t| {
                    let local = t.with_timezone(&Local);
                    local.format("%H:%M %d/%m/%y").to_string()
                })
                .unwrap_or_else(|| "-".to_string());

            println!(
                "{:<id_w$}  {:<name_w$}  {color}{:<10}{RESET}  {:<6}  {:<8}  {:<8}  {:<mode_w$}  {:<16}",
                id_short,
                p.name,
                status_str,
                pid_str,
                uptime_str,
                p.restart_count,
                mode_str,
                next_run_str,
                id_w = id_w,
                name_w = name_w,
                mode_w = mode_w,
            );
        } else {
            println!(
                "{:<id_w$}  {:<name_w$}  {color}{:<10}{RESET}  {:<6}  {:<8}  {:<8}  {:<mode_w$}",
                id_short,
                p.name,
                status_str,
                pid_str,
                uptime_str,
                p.restart_count,
                mode_str,
                id_w = id_w,
                name_w = name_w,
                mode_w = mode_w,
            );
        }
    }
}

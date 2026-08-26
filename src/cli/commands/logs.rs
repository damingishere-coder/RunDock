// @group BusinessLogic : `alter logs` command handler

use crate::cli::args::LogsArgs;
use crate::cli::commands::stop::{require_alive, resolve_id};
use crate::client::daemon_client::DaemonClient;
use anyhow::Result;

// @group Utilities : Format ISO8601 timestamp to dimmed HH:MM:SS for terminal output
fn format_ts(ts: &str) -> String {
    // ISO8601: 2026-02-28T14:23:45.123Z — extract HH:MM:SS at positions 11..19
    if ts.len() >= 19 {
        format!("\x1b[2m{}\x1b[0m ", &ts[11..19])
    } else {
        String::new()
    }
}

pub async fn run(client: &DaemonClient, args: LogsArgs, json_mode: bool) -> Result<()> {
    require_alive(client).await;

    let id = resolve_id(client, &args.target).await?;
    // "all" by default; --err → stderr only; --out → stdout only
    let stream_filter = if args.err {
        "stderr"
    } else if args.out {
        "stdout"
    } else {
        "all"
    };
    // Optional case-insensitive text filter
    let grep = args.grep.as_deref().map(|s| s.to_lowercase());

    if args.follow {
        println!(
            "[alter] streaming logs for '{}' (Ctrl+C to stop)...",
            args.target
        );
        client
            .stream_logs(&id, |line| {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                    let ts = val["timestamp"].as_str().unwrap_or("");
                    let content = val["content"].as_str().unwrap_or(&line);
                    let stream = val["stream"].as_str().unwrap_or("stdout");
                    let matches_stream = stream_filter == "all" || stream == stream_filter;
                    let matches_text = grep
                        .as_deref()
                        .is_none_or(|g| content.to_lowercase().contains(g));
                    if matches_stream && matches_text {
                        let prefix = if stream == "stderr" {
                            "\x1b[31m[err]\x1b[0m"
                        } else {
                            "\x1b[32m[out]\x1b[0m"
                        };
                        let ts_display = format_ts(ts);
                        println!("{ts_display}{prefix} {content}");
                    }
                } else {
                    println!("{line}");
                }
            })
            .await?;
    } else {
        let url = format!(
            "/api/v1/processes/{id}/logs?lines={}&type={}",
            args.lines, stream_filter
        );
        let result = client.get(&url).await?;

        if json_mode {
            println!("{}", serde_json::to_string_pretty(&result)?);
            return Ok(());
        }

        if let Some(lines) = result["lines"].as_array() {
            for entry in lines {
                let content = entry["content"].as_str().unwrap_or("");
                let stream = entry["stream"].as_str().unwrap_or("stdout");
                let ts = entry["timestamp"].as_str().unwrap_or("");
                // Apply text filter (case-insensitive)
                if grep
                    .as_deref()
                    .is_some_and(|g| !content.to_lowercase().contains(g))
                {
                    continue;
                }
                let prefix = if stream == "stderr" {
                    "\x1b[31m[err]\x1b[0m"
                } else {
                    "\x1b[32m[out]\x1b[0m"
                };
                let ts_display = format_ts(ts);
                println!("{ts_display}{prefix} {content}");
            }
        }
    }
    Ok(())
}

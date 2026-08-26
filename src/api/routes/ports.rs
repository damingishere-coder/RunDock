// @group APIEndpoints : Port scan endpoint — lists all open TCP/UDP ports with owning process names

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

use crate::api::error::ApiError;
use crate::daemon::state::DaemonState;

pub fn router(state: Arc<DaemonState>) -> Router {
    Router::new()
        .route("/", get(list_ports))
        .route("/kill/{pid}", post(kill_port_process))
        .with_state(state)
}

// @group Types > Ports : A single network port entry
#[derive(Serialize)]
struct PortEntry {
    port: u16,
    protocol: String,
    local_address: String,
    remote_address: String,
    state: String,
    pid: Option<u32>,
    process_name: Option<String>,
    /// Ancestor PIDs walking upward from the socket-owning process (immediate parent first).
    /// Lets the frontend match a port to its managed root process even when the socket is
    /// owned by a grandchild (e.g. alter → cmd.exe → node npm → cmd.exe → node vite).
    ancestor_pids: Vec<u32>,
}

#[derive(Deserialize)]
struct KillPortRequest {
    port: u16,
    process_name: Option<String>,
}

// @group APIEndpoints > Ports : GET /ports — list all open ports with owning process names
async fn list_ports(State(state): State<Arc<DaemonState>>) -> Result<Json<Value>, ApiError> {
    let _capacity = state
        .blocking_io_limit
        .try_acquire()
        .map_err(|_| ApiError::unavailable("port scan capacity is exhausted; retry later"))?;
    let entries = tokio::task::spawn_blocking(collect_ports)
        .await
        .map_err(|error| ApiError::internal(format!("port scan task failed: {error}")))?
        .map_err(|error| ApiError::internal(format!("port scan failed: {error}")))?;
    Ok(Json(json!({ "ports": entries })))
}

// @group BusinessLogic > Ports : Collect port entries, resolve names, and annotate ancestor chains
fn collect_ports() -> anyhow::Result<Vec<PortEntry>> {
    const MAX_PORT_ENTRIES: usize = 5_000;
    let raw = run_netstat()?;
    let mut entries = parse_netstat(&raw);
    if entries.len() > MAX_PORT_ENTRIES {
        tracing::warn!(
            total = entries.len(),
            limit = MAX_PORT_ENTRIES,
            "port scan result was truncated to protect the API and dashboard"
        );
        entries.truncate(MAX_PORT_ENTRIES);
    }

    // Refresh ALL processes so we can build a complete pid→parent_pid map.
    // ProcessRefreshKind::new() gives us the minimal info (name + parent) without
    // expensive fields like memory, CPU, or environment.
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, false, ProcessRefreshKind::new());

    // Build name and parent maps for every process visible to sysinfo.
    let mut name_map: HashMap<u32, String> = HashMap::new();
    let mut parent_map: HashMap<u32, u32> = HashMap::new();

    for (pid, proc) in sys.processes() {
        let pid_u32 = pid.as_u32();
        name_map.insert(pid_u32, proc.name().to_string_lossy().to_string());
        if let Some(ppid) = proc.parent() {
            let ppid_u32 = ppid.as_u32();
            // Ignore self-parented (PID 0 is the idle process and wraps around on some OSes)
            if ppid_u32 != 0 && ppid_u32 != pid_u32 {
                parent_map.insert(pid_u32, ppid_u32);
            }
        }
    }

    for entry in &mut entries {
        if let Some(pid) = entry.pid {
            entry.process_name = name_map.get(&pid).cloned();
            // Walk up to 12 levels — deep enough for npm → vite → actual server chains.
            entry.ancestor_pids = ancestor_chain(pid, &parent_map, 12);
        }
    }

    // Sort by port ascending, then by protocol
    entries.sort_by(|a, b| a.port.cmp(&b.port).then(a.protocol.cmp(&b.protocol)));
    Ok(entries)
}

// @group Utilities > Ports : Walk the parent chain from `start_pid` upward (max `depth` hops),
// returning ancestor PIDs in order from immediate parent toward the system root.
fn ancestor_chain(start_pid: u32, parent_map: &HashMap<u32, u32>, max_depth: usize) -> Vec<u32> {
    let mut chain = Vec::new();
    let mut current = start_pid;
    for _ in 0..max_depth {
        match parent_map.get(&current) {
            Some(&parent) => {
                chain.push(parent);
                current = parent;
            }
            None => break,
        }
    }
    chain
}

// @group Utilities > Ports : Run platform-appropriate netstat command and return raw stdout
fn bounded_port_command(program: &str, args: &[&str]) -> anyhow::Result<String> {
    const MAX_OUTPUT_BYTES: u64 = 2 * 1024 * 1024;
    const COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

    let output_path =
        std::env::temp_dir().join(format!("alter-ports-{}.log", uuid::Uuid::new_v4()));
    let output_file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&output_path)?;
    let mut command = std::process::Command::new(program);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(output_file))
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let result = (|| -> anyhow::Result<String> {
        let mut child = command.spawn()?;
        let started = std::time::Instant::now();
        let status = loop {
            let size = std::fs::metadata(&output_path)
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            if size > MAX_OUTPUT_BYTES || started.elapsed() >= COMMAND_TIMEOUT {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("{program} exceeded its 8-second or 2 MiB output limit");
            }
            match child.try_wait()? {
                Some(status) => break status,
                None => std::thread::sleep(std::time::Duration::from_millis(25)),
            }
        };
        if !status.success() {
            anyhow::bail!("{program} exited with status {status}");
        }
        let bytes = std::fs::read(&output_path)?;
        if bytes.len() > MAX_OUTPUT_BYTES as usize {
            anyhow::bail!("{program} output exceeded 2 MiB");
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    })();
    if let Err(error) = std::fs::remove_file(&output_path) {
        tracing::warn!(path = %output_path.display(), %error, "port scan output cleanup failed");
    }
    result
}

#[cfg(windows)]
fn run_netstat() -> anyhow::Result<String> {
    bounded_port_command("netstat", &["-ano"])
}

#[cfg(not(windows))]
fn run_netstat() -> anyhow::Result<String> {
    // Try ss first (modern Linux), fall back to netstat
    if let Ok(output) = bounded_port_command("ss", &["-Hntlpu"]) {
        return Ok(output);
    }
    bounded_port_command("netstat", &["-tlnpu"])
}

// @group Utilities > Ports : Parse netstat/ss stdout into PortEntry list (no process names yet)
fn parse_netstat(raw: &str) -> Vec<PortEntry> {
    raw.lines().filter_map(parse_line).collect()
}

// @group Utilities > Ports : Parse one line of netstat output into a PortEntry
#[cfg(windows)]
fn parse_line(line: &str) -> Option<PortEntry> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    match fields.as_slice() {
        // TCP  local  remote  STATE  pid
        [proto, local, remote, state, pid_str] if proto.eq_ignore_ascii_case("TCP") => {
            let port = extract_port(local)?;
            Some(PortEntry {
                port,
                protocol: "TCP".into(),
                local_address: local.to_string(),
                remote_address: remote.to_string(),
                state: state.to_string(),
                pid: pid_str.parse().ok(),
                process_name: None,
                ancestor_pids: Vec::new(),
            })
        }
        // UDP  local  remote  pid  (no state column on Windows)
        [proto, local, remote, pid_str] if proto.eq_ignore_ascii_case("UDP") => {
            let port = extract_port(local)?;
            Some(PortEntry {
                port,
                protocol: "UDP".into(),
                local_address: local.to_string(),
                remote_address: remote.to_string(),
                state: String::new(),
                pid: pid_str.parse().ok(),
                process_name: None,
                ancestor_pids: Vec::new(),
            })
        }
        _ => None,
    }
}

#[cfg(not(windows))]
fn parse_line(line: &str) -> Option<PortEntry> {
    let fields: Vec<&str> = line.split_whitespace().collect();

    // ss -Hntlpu output: Netid  State  RecvQ  SendQ  LocalAddr:Port  PeerAddr:Port  [users:...]
    // netstat -tlnpu:    Proto  RecvQ  SendQ  Local            Foreign          State  PID/Name
    if fields.len() < 5 {
        return None;
    }

    let first = fields[0].to_ascii_lowercase();
    if first.contains("tcp") || first.contains("udp") {
        // Could be netstat or ss netid column
        if fields.len() >= 7 && (first.starts_with("tcp") || first.starts_with("udp")) {
            // netstat format: proto recvq sendq local remote state pid/name
            if fields[0].starts_with("tcp") || fields[0].starts_with("udp") {
                let proto = if first.contains("tcp") { "TCP" } else { "UDP" };
                let local = fields[3];
                let remote = fields[4];
                let state = fields.get(5).copied().unwrap_or("").to_string();
                let pid = fields
                    .get(6)
                    .and_then(|s| s.split('/').next())
                    .and_then(|s| s.parse::<u32>().ok());
                let port = extract_port(local)?;
                return Some(PortEntry {
                    port,
                    protocol: proto.into(),
                    local_address: local.into(),
                    remote_address: remote.into(),
                    state,
                    pid,
                    process_name: None,
                    ancestor_pids: Vec::new(),
                });
            }
        }
        // ss format: netid state recvq sendq local peer [users]
        let proto = if first.contains("tcp") { "TCP" } else { "UDP" };
        let local = fields.get(4).copied().unwrap_or("");
        let remote = fields.get(5).copied().unwrap_or("");
        let state = fields.get(1).copied().unwrap_or("").to_string();
        let pid = fields
            .iter()
            .find(|f| f.starts_with("users:"))
            .and_then(|s| {
                let start = s.find("pid=")?;
                let rest = &s[start + 4..];
                let end = rest
                    .find(|c: char| !c.is_ascii_digit())
                    .unwrap_or(rest.len());
                rest[..end].parse::<u32>().ok()
            });
        let port = extract_port(local)?;
        Some(PortEntry {
            port,
            protocol: proto.into(),
            local_address: local.into(),
            remote_address: remote.into(),
            state,
            pid,
            process_name: None,
            ancestor_pids: Vec::new(),
        })
    } else {
        None
    }
}

// @group Utilities > Ports : Extract port number from "addr:port" or "[::1]:port"
fn extract_port(addr: &str) -> Option<u16> {
    addr.rsplit(':').next()?.parse().ok()
}

// @group APIEndpoints > Ports : POST /ports/kill/:pid — stop the verified managed root that owns a port
async fn kill_port_process(
    State(state): State<Arc<DaemonState>>,
    Path(pid): Path<u32>,
    Json(expected): Json<KillPortRequest>,
) -> Result<Json<Value>, ApiError> {
    let _mutation_guard = state.state_mutation_lock.lock().await;
    let _capacity = state
        .blocking_io_limit
        .try_acquire()
        .map_err(|_| ApiError::unavailable("port scan capacity is exhausted; retry later"))?;
    if pid == 0 {
        return Err(ApiError::bad_request("Cannot stop PID 0 (idle/system)"));
    }

    let entries = tokio::task::spawn_blocking(collect_ports)
        .await
        .map_err(|error| ApiError::internal(format!("port ownership task failed: {error}")))?
        .map_err(|error| ApiError::internal(format!("port ownership scan failed: {error}")))?;
    if expected
        .process_name
        .as_deref()
        .is_some_and(|name| name.len() > 260 || name.chars().any(char::is_control))
    {
        return Err(ApiError::bad_request("invalid expected process name"));
    }
    let entry = entries
        .into_iter()
        .find(|entry| {
            entry.pid == Some(pid)
                && entry.port == expected.port
                && entry.process_name == expected.process_name
        })
        .ok_or_else(|| {
            ApiError::conflict(
                "port or process ownership changed after confirmation; refresh before retrying",
            )
        })?;
    let mut ownership_chain = entry.ancestor_pids;
    ownership_chain.insert(0, pid);
    let managed = state
        .manager
        .list()
        .await
        .into_iter()
        .find(|process| {
            process
                .pid
                .is_some_and(|root| ownership_chain.contains(&root))
        })
        .ok_or_else(|| ApiError {
            status: axum::http::StatusCode::FORBIDDEN,
            message: "refusing to stop an unmanaged OS process".to_string(),
        })?;
    let before = state
        .manager
        .snapshot_one(managed.id)
        .await
        .map_err(ApiError::from)?;
    let stopped = state
        .manager
        .stop(managed.id)
        .await
        .map_err(ApiError::from)?;
    if let Err(error) = state.save_to_disk().await {
        state
            .manager
            .restore_snapshot(before)
            .await
            .map_err(|rollback_error| {
                ApiError::internal(format!(
                    "managed process stop was not persisted ({error}); runtime rollback failed ({rollback_error})"
                ))
            })?;
        return Err(ApiError::internal(format!(
            "managed process stop was not persisted; the exact runtime snapshot was restored: {error}"
        )));
    }
    Ok(Json(json!({
        "success": true,
        "managed_process_id": stopped.id,
        "managed_process_name": stopped.name,
    })))
}

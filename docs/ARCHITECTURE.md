# Architecture

> How RunDock works under the hood — daemon design, process lifecycle, logging pipeline, and data flow.

---

## Overview

The RunDock backend is a **single binary** that plays two roles depending on how it is invoked:

1. **CLI** — RunDock's command-line interface (`alter start`, `alter list`, etc.)
2. **Daemon** — a long-running background HTTP server that manages processes

```
┌─────────────────────────────────────────────┐
│  Terminal / Script / Web Browser             │
└──────┬──────────────────────────┬────────────┘
       │ alter <command>           │ HTTP (browser)
       ▼                          ▼
┌──────────────┐        ┌──────────────────────┐
│  CLI Layer   │        │  Web Dashboard        │
│  (clap)      │        │  (embedded HTML/JS)   │
└──────┬───────┘        └──────────┬────────────┘
       │ HTTP (reqwest)            │ HTTP
       ▼                          ▼
┌─────────────────────────────────────────────┐
│          Daemon (Axum HTTP on :2999)         │
│  ┌──────────────────────────────────────┐   │
│  │          DaemonState                 │   │
│  │  ┌───────────────────────────────┐   │   │
│  │  │       ProcessManager          │   │   │
│  │  │  (DashMap<Uuid, ManagedProc>) │   │   │
│  │  └───────────────────────────────┘   │   │
│  └──────────────────────────────────────┘   │
└─────────────────────────────────────────────┘
       │
       ├─ Spawns child processes (hidden, no console window)
       ├─ Captures stdout/stderr → log files + broadcast channel
       ├─ Watches files (notify crate) for watch mode
       └─ Persists state to disk (state.json)
```

---

## Source Layout

```
src/
├── main.rs               # Entry point — routes to CLI or internal daemon
├── cli/
│   ├── args.rs           # Clap CLI definitions (commands + flags)
│   ├── commands/         # One file per subcommand (start, stop, list, …)
│   └── mod.rs
├── api/
│   ├── routes/
│   │   ├── processes.rs, projects.rs, git.rs
│   │   ├── auth.rs, system.rs, update.rs, ports.rs
│   │   ├── ai.rs, telegram.rs, notifications.rs, log_alerts.rs
│   │   └── tunnels.rs, terminal.rs, scripts.rs, metrics.rs
│   ├── error.rs          # ApiError → HTTP response conversion
│   ├── middleware.rs
│   └── mod.rs
├── daemon/
│   ├── server.rs         # Axum server setup (SO_REUSEADDR, CORS, routes)
│   ├── state.rs          # DaemonState, save/load/restore
│   ├── signals.rs        # OS signal handling
│   └── mod.rs
├── process/
│   ├── manager.rs        # ProcessManager — high-level lifecycle API
│   ├── instance.rs       # ManagedProcess — per-process state
│   ├── runner.rs         # Spawn child + pipe stdout/stderr
│   ├── identity.rs       # Immutable PID identity + verified termination
│   ├── tree.rs           # Windows Job / Unix process-group ownership
│   ├── health.rs         # Bounded loopback health probes
│   ├── restarter.rs      # Auto-restart loop with exponential backoff
│   ├── watcher.rs        # File system watcher (watch mode)
│   └── mod.rs
├── logging/
│   ├── writer.rs         # RollingFileWriter
│   ├── rotation.rs       # Size-based + date-based log rotation
│   ├── reader.rs         # Read/merge historical logs
│   └── mod.rs
├── config/
│   ├── ecosystem.rs      # AppConfig + EcosystemConfig structs
│   ├── paths.rs          # Platform-aware data/log paths
│   ├── atomic_file.rs     # Bounded atomic JSON + validated LKG recovery
│   ├── state_transaction.rs # Crash-recoverable state/projects pair
│   ├── daemon_config.rs  # DaemonConfig (port, host)
│   └── mod.rs
├── models/
│   ├── process_info.rs   # ProcessInfo — serializable snapshot for API
│   ├── process_status.rs # ProcessStatus enum (7 states)
│   ├── api_types.rs      # StartRequest + response structs
│   └── mod.rs
├── client/
│   ├── daemon_client.rs  # reqwest HTTP client (CLI → daemon)
│   └── mod.rs
├── web/                   # rust-embed serving for web-ui/dist
├── telegram/, tunnel/, notifications/
├── web-ui/                # React/Vite dashboard source
├── desktop-shell/          # Windows-only Tauri 2/WebView2 desktop shell
│   ├── src/                # Tray, single-instance, startup and navigation policy
│   └── ui/                 # Local startup/error page (no daemon IPC capability)
└── utils/
    ├── pid.rs, format.rs, table.rs
    └── mod.rs
```

---

## Daemon Startup Sequence

```
alter daemon start
        │
        ▼
DaemonClient::probe_readiness() → TCP connect + strict health contract on :2999
        │
        ├── verified RunDock? → reuse it
        ├── port unused? → start the sibling backend executable
        └── occupied/incompatible? → fail with diagnostics; never end the listener
                │
                ▼
        Spawn hidden child: alter --internal-daemon --port 2999
        (Windows: CREATE_NO_WINDOW | DETACHED_PROCESS)
        (Unix: stdio → /dev/null)
                │
                ▼
        Poll :2999 every 100ms (up to 10s)
                │
                ▼
        GET /api/v1/system/health → status/version/PID/persistence contract
                │
                ▼
        Print "daemon started at http://127.0.0.1:2999/"
```

**Internal daemon start (`--internal-daemon`):**

```
DaemonState::new()
        │
        ├── ProcessManager::new()  (empty DashMap)
        ├── recover and validate state.json + projects.json
        │       └── restore() → re-adopt only a live process whose PID identity still matches;
        │                       otherwise retain it as stopped/error state
        │
        ▼
Server::start()
        ├── socket2: bind TCP with SO_REUSEADDR
        ├── Build Axum router (REST API + static assets)
        ├── Add CORS allowlist (same origin + explicit loopback dashboard/dev origins)
        ├── Add tracing layer
        └── tokio::serve() → async loop
```

---

## Process Lifecycle

```
ProcessStatus state machine:

     start()          spawn OK
  ┌──────────┐      ┌──────────┐      ┌─────────┐
  │  Stopped │─────▶│ Starting │─────▶│ Running │
  └──────────┘      └──────────┘      └────┬────┘
       ▲                                    │
       │         stop()                     │ watch mode
       │       ┌──────────┐                 ▼
       └───────│ Stopping │          ┌──────────┐
               └──────────┘          │ Watching │
                                     └──────────┘
                                          │
                                          │ crash (exit ≠ 0)
                                          ▼
                                    ┌─────────┐
                        restart     │ Crashed │
                        attempts ───│         │
                        remaining   └────┬────┘
                                         │ max_restarts reached
                                         ▼
                                    ┌─────────┐
                                    │ Errored │
                                    └─────────┘
```

**Transitions:**
- `start()` → `Stopped → Starting → Running`
- `stop()` → `Running → Stopping → Stopped`
- Clean exit (code 0) → `Stopped` (no restart, even with autorestart)
- Crash (non-zero exit) → `Crashed` → auto-restart loop begins
- Max restarts exceeded → `Errored` (no more attempts)
- Watch mode active → `Watching` (same as Running, plus file watcher)
- File change detected (watch) → `Watching → Stopping → Starting → Watching`

---

## Process Spawning

**File:** `src/process/runner.rs`

```
spawn_process(script, args, cwd, env_vars, log_tx, exit_tx)
        │
        ├── Windows path:
        │   ├── script ends in .exe or has path separator?
        │   │   └── Command::new(script)
        │   └── otherwise (npm, node, python as .cmd):
        │       └── Command::new("cmd").arg("/C").arg(script)
        │   └── .creation_flags(CREATE_NO_WINDOW)  ← no popup window
        │
        ├── set cwd, env vars
        ├── stdout/stderr → Stdio::piped()
        │
        └── child.spawn()
                ├── tokio::spawn → read stdout → broadcast LogLine (Stdout)
                ├── tokio::spawn → read stderr → broadcast LogLine (Stderr)
                └── tokio::spawn → wait_for_exit() → send RunResult
```

**`CREATE_NO_WINDOW` (Windows):**
Every spawned process uses the `0x08000000` creation flag. This prevents Windows from showing a black console window in the taskbar when any process starts. Output is still captured normally — it flows through the piped stdio into the log system.

---

## Logging Pipeline

```
Child process stdout/stderr
        │
        ▼
AsyncBufReadExt::lines()  (tokio)
        │
        ├──▶  broadcast::Sender<LogLine>
        │         │
        │         └──▶  SSE clients (web dashboard, alter logs --follow)
        │
        └──▶  RollingFileWriter (src/logging/writer.rs)
                    │
                    ├── writes to: logs/<name>/out.log  (stdout)
                    │             logs/<name>/err.log   (stderr)
                    │
                    └── triggers rotation when size > max_log_size_mb
                                │
                                ├── size rotation: out.log → out.log.1 → out.log.2 (max 5)
                                └── date rotation: out.log → out.log.YYYY-MM-DD (max 30 days)
```

**SSE streaming:**
- Each process has a `broadcast::Sender<LogLine>` with capacity 1024
- Web dashboard subscribes with `broadcast::Receiver`
- 15-second timeout sends `: keepalive` SSE comment to detect dead connections
- `RecvError::Lagged` (client too slow) is handled — missed messages are skipped, stream continues

---

## State Persistence

**File:** `%APPDATA%\alter-pm2\state.json` (Windows) or `~/.alter-pm2/state.json`

**Format (simplified):**
```json
{
  "schema_version": 1,
  "saved_at": "2026-02-22T10:00:00Z",
  "apps": [
    {
      "id": "uuid",
      "config": { /* full AppConfig */ },
      "restart_count": 2,
      "last_pid": 1234,
      "process_identity": { "start_time_secs": 1771735200 },
      "cron_was_active": false
    }
  ]
}
```

**Auto-save:** Lifecycle mutations are serialized with persistence and rollback. Individual JSON files use bounded temporary files, durable atomic replacement, semantic validation, and a validated last-known-good copy. Runtime state and logical projects additionally use a transaction marker so startup can deterministically roll forward or roll back an interrupted pair update.

**Restore on startup:**
```rust
for app in saved.apps {
    // Re-adopt only when the saved PID is alive and its immutable start
    // identity still matches. Never guess-restart an ordinary process.
    if live_pid_identity_matches(&app) { adopt(app) }
    else if app.cron_was_active { restore_sleeping_cron(app) }
    else { register_stopped(app) }
}
```

---

## Auto-restart with Exponential Backoff

**File:** `src/process/restarter.rs`

```
Process crashes (exit code ≠ 0)
        │
        ▼
wait backoff_delay(base_ms, attempt)
        │
        │  Formula: base_ms × 2^min(attempt, 8)
        │  Capped at 60,000 ms (60 seconds)
        │
        │  attempt=0: 1000ms
        │  attempt=1: 2000ms
        │  attempt=2: 4000ms
        │  attempt=3: 8000ms
        │  attempt=8: 256,000ms → capped to 60,000ms
        │
        ▼
attempt < max_restarts?
        ├── yes → spawn_process() again, increment attempt
        └── no  → status = Errored, stop retrying
```

---

## Watch Mode

**File:** `src/process/watcher.rs`

```
File system watcher (notify crate)
        │
        ├── watches paths in config.watch_paths
        ├── ignores patterns in config.watch_ignore
        │
        ▼
File change event detected
        │
        ▼
Debounce: wait 500ms for burst of events to settle
        │
        ▼
manager.restart(id)  → stop current child → spawn new child
        │
        ▼
status → Watching (same as Running but watcher is active)
```

---

## Web Dashboard Architecture

The React dashboard is built by Vite into `web-ui/dist` and then **compiled into
`alter.exe`** using [rust-embed](https://github.com/pyros2097/rust-embed).
At runtime the daemon serves those assets from memory on the same loopback
origin as `/api/v1`.

**Technology:**
- **Frontend:** React 19 + TypeScript + Vite
- **Styling:** CSS with shared theme variables
- **Real-time updates:** Auto-refresh every 3 seconds + SSE for log streaming
- **Transport:** Fetch API + EventSource

### Windows desktop shell

`rundock.exe` is a separate Tauri 2 package. Its WebView2 window initially loads
only a bundled startup/error page, calls the shared daemon lifecycle state
machine with the sibling `alter.exe`, and navigates to
`http://127.0.0.1:2999/` only after health ownership is verified. The remote
dashboard receives no Tauri IPC, filesystem, or shell capability. Navigation
away from the canonical 2999 loopback origin is denied in the WebView and safe
external HTTP(S) or registered custom-protocol links are handed to Windows.

Only one desktop shell instance is allowed. A second launch restores the
existing window. The close button hides it to the tray; tray exit stops only
`rundock.exe`. Current-user login startup passes `--background`, so the daemon
can be recovered without showing a window.

**Dashboard views:**

| View | Description |
|------|-------------|
| Processes | Process table grouped by namespace, collapse/expand |
| Start Process | Form to start a new process |
| Process Detail | Full-height log viewer with SSE streaming, action buttons |
| Edit Process | Form to update config, restarts the process on save |

---

## IPC Method

The CLI communicates with the daemon over **plain HTTP** using `reqwest`. There is no shared memory, no named pipes, and no Unix domain sockets — just HTTP/JSON on localhost.

```
alter list
  └──▶  GET http://127.0.0.1:2999/api/v1/processes
             └──▶  JSON array of process objects
                       └──▶  formatted as table in terminal
```

This design means:
- Any HTTP client can talk to the daemon (curl, PowerShell, browser, custom scripts)
- The CLI and web dashboard use the exact same API
- Easy to inspect with browser DevTools or curl

---

## Platform-Specific Details

### Windows

| Concern | Solution |
|---------|----------|
| Console window popup | `CREATE_NO_WINDOW` flag on every spawn |
| Daemon detachment | `DETACHED_PROCESS` + `CREATE_NO_WINDOW` |
| Data directory | `%APPDATA%\alter-pm2\` |
| `.cmd` scripts (npm, yarn) | Wrapped in `cmd /C` automatically |
| Terminal button | Tries `wt.exe` (Windows Terminal), falls back to `cmd.exe` |
| Startup integration | Desktop installer: tray autostart; CLI-only: optional Scheduled Task |
| Port reuse | `SO_REUSEADDR` via socket2 crate |

### Linux / macOS

| Concern | Solution |
|---------|----------|
| Daemon detachment | stdio → `/dev/null`, parent returns |
| Data directory | `~/.alter-pm2/` |
| Terminal button | Opens `xterm` |
| Startup (Linux) | systemd unit file template |
| Startup (macOS) | Shell profile instructions |

---

## Data Flow: Starting a Process

```
User runs: alter start python -- -m http.server 8080
                │
                ▼
CLI parses args → builds StartRequest JSON
                │
                ▼
POST /api/v1/processes  {script:"python", args:["-m","http.server","8080"]}
                │
                ▼
start_process handler (processes.rs)
  └── builds AppConfig from request
  └── manager.start(config)
        │
        ├── creates ManagedProcess (Stopped)
        ├── inserts into DashMap registry
        ├── status → Starting
        ├── spawn_process() → Child + log pipes
        ├── status → Running, pid = child.id()
        └── returns ProcessInfo
                │
                ▼
tokio::spawn → save_to_disk() (auto-save, background)
                │
                ▼
Response: 201 Created { id, name, status: "running", pid, ... }
                │
                ▼
CLI prints result table
```

---

## Key Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `tokio` | 1.x | Async runtime (full features) |
| `axum` | 0.8 | HTTP web framework |
| `clap` | 4.x | CLI argument parsing |
| `serde` + `serde_json` + `toml` | — | Serialization |
| `uuid` | 1.x | Process ID generation |
| `chrono` | 0.4 | Timestamps (UTC) |
| `dashmap` | 6.x | Concurrent HashMap (process registry) |
| `tokio::sync::broadcast` | — | Log line fan-out to SSE clients |
| `tracing` + `tracing-subscriber` | — | Structured logging |
| `rust-embed` | 8.x | Compile-time asset embedding |
| `notify` | 6.x | File system events (watch mode) |
| `reqwest` | 0.12 | HTTP client (CLI → daemon) |
| `tower-http` | 0.6 | CORS + request tracing middleware |
| `socket2` | 0.5 | Low-level socket control (SO_REUSEADDR) |
| `anyhow` + `thiserror` | — | Error handling |
| `windows` | 0.58 | Win32 API (Windows only, process flags) |

The independent `desktop-shell/Cargo.toml` adds Tauri 2, its single-instance and
autostart plugins, and Windows WebView2 integration without adding those
dependencies to Linux CLI/package builds.

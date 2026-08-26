# RunDock — Developer Project Console

> A fast, lightweight process manager for Windows (and cross-platform). Manage any runtime — Python, Node.js, Go, Rust, .NET, PHP, Ruby — from a single tool with a built-in web dashboard.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](../LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-orange.svg)](https://www.rust-lang.org/)
[![Platform: Windows](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey.svg)]()

---

## Table of Contents

- [Why RunDock?](#why-rundock)
- [Features](#features)
- [Windows Notes](#windows-notes)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Web Dashboard](#web-dashboard)
- [Ecosystem Config](#ecosystem-config)
- [Persistence](#persistence)
- [Documentation](#documentation)
- [Contributing](#contributing)
- [License](#license)

---

## Why RunDock?

Managing background processes on Windows has always been awkward. RunDock gives you a polished dashboard while retaining the compatible `alter` command:

- **No console window popups** — processes run silently in the background
- **Auto-restart** on crash with exponential backoff
- **Watch mode** — automatically restart when source files change
- **Structured logging** with rotation and historical browsing
- **Web dashboard** — manage everything from your browser, with keyboard shortcuts
- **Notifications** — get alerted on crashes, restarts, or stops via Slack, Teams, Discord, or webhook
- **Single binary** — no runtime dependencies, no Node.js, no Python required

---

## Features

| Feature | Description |
|---------|-------------|
| **Background Daemon** | Runs as a hidden background service, survives terminal close |
| **Any Runtime** | Start Python, Node.js, Go, Rust, .NET, PHP, or any executable |
| **Auto-restart** | Restarts crashed processes with exponential backoff (configurable) |
| **Watch Mode** | Restart on file changes — great for development workflows |
| **Namespaces** | Group processes logically (e.g. `web`, `workers`, `default`) |
| **Log Rotation** | Size-based + date-based rotation, historical browsing |
| **Web Dashboard** | Real-time process monitor with live log streaming and keyboard shortcuts |
| **Notifications** | Webhook, Slack, Teams, and Discord alerts on process events |
| **Resource Monitoring** | Live CPU % and memory usage per process |
| **State Persistence** | Save and restore your process list across reboots |
| **Ecosystem Config** | Define multiple apps in a single TOML or JSON file |
| **REST API** | Full HTTP API — automate anything |
| **OS Startup** | Register the daemon as a system startup task |

---

## Windows Notes

RunDock is **designed with Windows in mind** while retaining the compatible `alter` CLI:

- Processes spawn with `CREATE_NO_WINDOW` — **no black console windows** appearing on your taskbar
- Daemon runs as a detached hidden background process
- Terminal button opens **Windows Terminal** (`wt.exe`) or falls back to `cmd.exe`
- Data stored in `%APPDATA%\alter-pm2\` (no cluttering your home directory)
- Startup integration via PowerShell Scheduled Tasks

**Windows-specific paths:**
```
%APPDATA%\alter-pm2\
├── state.json          ← saved process list
├── daemon.log          ← daemon output
└── logs\
    └── <process-name>\
        ├── out.log     ← process stdout
        └── err.log     ← process stderr
```

---

## Installation

### Debian / Ubuntu package

The signed APT repository and matching public key are published by the release workflow. Inspect the downloaded key fingerprint against `rundock-release-key.asc` on the corresponding GitHub Release before trusting it:

```bash
curl -fsSL https://damingishere-coder.github.io/RunDock/gpg-key.asc -o rundock-release-key.asc
gpg --show-keys --fingerprint rundock-release-key.asc
sudo gpg --dearmor --yes -o /usr/share/keyrings/rundock-archive-keyring.gpg rundock-release-key.asc
echo "deb [signed-by=/usr/share/keyrings/rundock-archive-keyring.gpg] https://damingishere-coder.github.io/RunDock/apt stable main" | sudo tee /etc/apt/sources.list.d/rundock.list
sudo apt update
sudo apt install alter
sudo systemctl enable --now alter-daemon.service
```

The system service stores state in `/var/lib/alter-pm2` and logs in `/var/log/alter-pm2`. It uses a dynamic, sandboxed account and cannot access home directories. Release downloads also contain `SHA256SUMS` and `SHA256SUMS.asc` for offline verification.

Release maintainers must configure the armored private key as the `APT_GPG_KEY` repository secret and its exact uppercase primary-key fingerprint as the `APT_GPG_FINGERPRINT` repository variable. The workflow fails closed when either value is missing or the imported key does not match the pinned fingerprint.

### Build from Source

**Prerequisites:** [Rust 1.98](https://rustup.rs/) (repository-pinned toolchain),
Node.js 24, and npm.

```powershell
# Clone the repo
git clone https://github.com/damingishere-coder/RunDock.git
cd RunDock

# Build the dashboard that Rust embeds, then the locked release binary
cd web-ui
npm ci
npm run build
cd ..
cargo build --release --locked

# The binary is at:
.\target\release\alter.exe

# Optional: add to PATH
$env:PATH += ";$(Get-Location)\target\release"
```

> **Dev build** (faster compile, includes debug info):
> ```powershell
> cargo build --locked
> # Binary: .\target\debug\alter.exe
> ```

---

## Quick Start

```powershell
# 1. Start the background daemon
alter daemon start

# 2. Start a process
alter start python -- -m http.server 8080
alter start node --name api -- server.js
alter start go --name backend --cwd C:\projects\api -- run main.go

# 3. List running processes
alter list

# 4. View logs
alter logs api
alter logs api --follow          # stream in real time

# 5. Control processes
alter stop api
alter restart api
alter delete api

# 6. Open the web dashboard
alter web                        # opens http://127.0.0.1:2999/
```

---

## Web Dashboard

Navigate to **http://127.0.0.1:2999/** after starting the daemon.

**Dashboard features:**
- Real-time process table with status, PID, uptime, restarts
- Namespace grouping with collapse/expand
- Start, Stop, Restart, Delete — directly from the browser
- Live log streaming (no page refresh needed)
- Historical log browsing by date
- Edit process config and apply immediately
- Open terminal in working directory
- Open working directory in VS Code
- Save state / Shutdown daemon

---

## Ecosystem Config

Define multiple processes in one file:

```toml
# alter.config.toml

[[apps]]
name             = "api"
script           = "python"
args             = ["-m", "uvicorn", "main:app", "--port", "8000"]
cwd              = "C:\\projects\\api"
autorestart      = true
max_restarts     = 10
namespace        = "web"

[apps.env]
PORT         = "8000"
DATABASE_URL = "postgres://localhost/mydb"

[[apps]]
name         = "worker"
script       = "node"
args         = ["dist/worker.js"]
watch        = true
watch_paths  = ["dist/"]
namespace    = "workers"

[apps.env]
NODE_ENV = "production"
```

```powershell
alter start alter.config.toml
```

See [ECOSYSTEM_CONFIG.md](./ECOSYSTEM_CONFIG.md) for the full field reference.

---

## Persistence

```powershell
# Save current process list to disk
alter save

# On next boot — restore everything
alter resurrect

# Register daemon to start automatically at login (Windows)
alter startup
```

Processes saved with `alter save` remain visible after reboot. RunDock persists the last PID and an immutable process-start identity: a still-live, matching process is safely re-adopted; a missing or mismatched PID is restored as stopped instead of being started automatically. Active cron schedules are restored as sleeping schedulers. `alter resurrect` performs the same conservative restore and never guesses that an ordinary process should be relaunched.

---

## Notifications

Get alerted when processes crash, restart, stop, or start. Notifications are configured via the REST API and stored in `%APPDATA%\alter-pm2\notifications.json`.

**Supported channels:**
- **Webhook** — generic HTTP POST with a JSON payload
- **Slack** — incoming webhook with color-coded attachments
- **Microsoft Teams** — MessageCard via incoming webhook
- **Discord** — rich embed via Discord webhook URL

**Event triggers:** `on_crash`, `on_restart`, `on_start`, `on_stop`

**Config scope cascade:** process-level → namespace-level → global (most specific wins per channel)

```bash
# Configure global Slack notifications for crashes and restarts
curl -X PUT http://localhost:2999/api/v1/notifications/global \
  -H "Content-Type: application/json" \
  -d '{
    "slack": { "webhook_url": "https://hooks.slack.com/...", "enabled": true },
    "events": { "on_crash": true, "on_restart": true }
  }'

# Test your notification config
curl -X POST http://localhost:2999/api/v1/notifications/test \
  -H "Content-Type: application/json" \
  -d '{ "slack": { "webhook_url": "https://hooks.slack.com/...", "enabled": true }, "events": { "on_start": true } }'
```

See [API Reference](./API.md#notification-endpoints) for the full endpoint reference.

---

## Keyboard Shortcuts

The web dashboard supports global keyboard shortcuts (active when not typing in a form):

| Key | Action |
|-----|--------|
| `r` | Reload / refresh process list |
| `n` | Go to Start New Process |
| `?` | Show keyboard shortcut help |
| `g` → `p` | Navigate to Processes |
| `g` → `h` | Navigate to Home / Analytics |
| `g` → `s` | Navigate to Settings |
| `g` → `n` | Navigate to Start New Process |
| `g` → `c` | Navigate to Cron Jobs |

`g` chords: press `g`, then the second key within 1 second.

---

## Documentation

| Document | Description |
|----------|-------------|
| [CLI Reference](./CLI.md) | All commands, flags, and examples |
| [API Reference](./API.md) | Full REST API documentation |
| [Ecosystem Config](./ECOSYSTEM_CONFIG.md) | Config file format reference |
| [Architecture](./ARCHITECTURE.md) | How RunDock works under the hood |
| [Changelog](./CHANGELOG.md) | Version history |

---

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/my-feature`)
3. Follow the existing code style (`@group` comments, module structure)
4. Add tests in `tests/unit/` or `tests/integration/`
5. Open a pull request

### Running Tests

```powershell
cargo test
```

### Project Layout

```
src/
├── cli/          # Command-line interface (clap)
├── api/          # HTTP REST API (axum)
├── daemon/       # Daemon lifecycle and state
├── process/      # Process spawning, restarting, watching
├── logging/      # Log writing, rotation, reading
├── config/       # Config parsing and paths
├── models/       # Shared data types
├── client/       # HTTP client for CLI → daemon
├── web/          # Embedded web dashboard
└── utils/        # Shared utilities
tests/
├── unit/         # Unit tests
└── integration/  # Integration tests
excluded/
└── docs/         # This documentation
```

---

## License

MIT License — see [LICENSE](../LICENSE) for details.

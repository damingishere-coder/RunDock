# RunDock — Developer Project Console

<p align="center">
  <img src="./assets/rundock-icon.svg" width="112" alt="RunDock icon" />
</p>

> A polished local project and process manager for Windows (and cross-platform). RunDock keeps related services together, exposes logs and ports when needed, and retains the compatible `alter` CLI for existing scripts.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-orange.svg)](https://www.rust-lang.org/)
[![Platform: Windows](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey.svg)]()
[![GitHub](https://img.shields.io/badge/GitHub-RunDock-147BFF?logo=github)](https://github.com/damingishere-coder/RunDock)

---

## Installation

### Manual installer

Download the latest `RunDock-x.x.x-windows-x64-setup.exe` from [Releases](https://github.com/damingishere-coder/RunDock/releases) and run it.
RunDock is the product name; the compatible `alter.exe` command is added to your `PATH` automatically.

### Debian / Ubuntu (signed APT repository)

Download the public key, inspect its fingerprint against the key attached to the matching [GitHub Release](https://github.com/damingishere-coder/RunDock/releases), then install it:

```bash
curl -fsSL https://damingishere-coder.github.io/RunDock/gpg-key.asc -o rundock-release-key.asc
gpg --show-keys --fingerprint rundock-release-key.asc
sudo gpg --dearmor --yes -o /usr/share/keyrings/rundock-archive-keyring.gpg rundock-release-key.asc
echo "deb [signed-by=/usr/share/keyrings/rundock-archive-keyring.gpg] https://damingishere-coder.github.io/RunDock/apt stable main" | sudo tee /etc/apt/sources.list.d/rundock.list
sudo apt update
sudo apt install alter
```

The packaged service is opt-in: `sudo systemctl enable --now alter-daemon.service`. System mode uses `/var/lib/alter-pm2` and `/var/log/alter-pm2`; it is isolated from per-user data.

---

## Features

- **No console window popups** — processes run silently in the background (Windows)
- **Auto-restart** with exponential backoff on crash
- **Watch mode** — restart automatically on file changes
- **Namespaces** — group and bulk-control related processes
- **Web dashboard** — real-time process monitor at `http://localhost:2999/`
- **Live log streaming** — tail logs in terminal or browser
- **State persistence** — save and restore your process list across reboots
- **Ecosystem config** — define all apps in one TOML or JSON file
- **Full REST API** — automate everything
- **Single binary** — no runtime dependencies
- **Dashboard authentication** — password-protect the web UI with Argon2id hashing, session tokens, and a PIN quick-unlock
- **Telegram bot** — control your processes from Telegram: list, start, stop, restart, tail logs, and receive crash/restart alerts
- **AI assistant** — multi-provider chat panel (Ollama, GitHub Models, Claude, OpenAI-compatible) with streaming responses and process-aware context
- **Port Finder** — scan all open TCP/UDP ports, see owning processes, and kill by PID from the dashboard
- **Notifications** — Slack, Discord, Microsoft Teams, and webhook alerts on crash, restart, cron events, and more
- **Process enable/disable** — exclude individual processes from Start All without removing them
- **Terminal history** — per-process command history persisted across sessions
- **Sidebar namespace groups** — active processes grouped by namespace with collapsible sections and bulk stop/restart

### Build from source

Requires [Rust 1.98](https://rustup.rs/), Node.js 24, and npm. The dashboard
must be built before Rust embeds `web-ui/dist` into the binary.

```powershell
git clone https://github.com/damingishere-coder/RunDock
cd RunDock
cd web-ui
npm ci
npm run build
cd ..
cargo build --release --locked
# Binary: target\release\alter.exe
```

---

## Quick Start

```powershell
# Start the daemon
alter daemon start

# Start processes
alter start python -- -m http.server 8080
alter start node --name api -- server.js
alter start go --name backend --cwd C:\projects\api -- run main.go

# List processes
alter list

# Stream logs
alter logs api --follow

# Open web dashboard
alter web    # → http://127.0.0.1:2999/
```

---

## Windows

RunDock is built with Windows as a first-class platform:

- Spawned processes use `CREATE_NO_WINDOW` — **no black console popups**
- Daemon runs completely hidden in the background
- `npm`, `yarn`, `npx` and other `.cmd` scripts work directly
- Terminal button opens Windows Terminal or `cmd.exe` in the process directory
- Data stored in `%APPDATA%\alter-pm2\`

---

## Ecosystem Config

```toml
# alter.config.toml
[[apps]]
name      = "api"
script    = "python"
args      = ["-m", "uvicorn", "main:app", "--port", "8000"]
cwd       = "C:\\projects\\api"
namespace = "web"
[apps.env]
PORT = "8000"

[[apps]]
name      = "worker"
script    = "node"
args      = ["dist/worker.js"]
watch     = true
namespace = "workers"
[apps.env]
NODE_ENV = "production"
```

```powershell
alter start alter.config.toml
```

---

## Documentation

Full documentation is in [`docs/`](./docs/):

| Document | Description |
|----------|-------------|
| [README](./docs/README.md) | Full project overview and setup guide |
| [CLI Reference](./docs/CLI.md) | All commands, flags, and examples |
| [API Reference](./docs/API.md) | Full REST API documentation |
| [Ecosystem Config](./docs/ECOSYSTEM_CONFIG.md) | Config file format reference |
| [Architecture](./docs/ARCHITECTURE.md) | How RunDock works under the hood |
| [Changelog](./docs/CHANGELOG.md) | Version history |

---

## License

MIT — see [LICENSE](./LICENSE) for details.

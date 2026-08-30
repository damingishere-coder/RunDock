#!/usr/bin/env bash
# @group BuildSystem : Real install, upgrade, health, and removal smoke for the Debian package

set -euo pipefail

if [[ "${CI:-}" != "true" || "$#" -ne 1 ]]; then
    echo "usage (CI only): CI=true $0 <release-binary>" >&2
    exit 64
fi

BINARY="$(realpath "$1")"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SMOKE_DIR="$(mktemp -d)"
MARKER="/var/lib/alter-pm2/package-smoke-marker"

cleanup() {
    sudo systemctl stop alter-daemon.service >/dev/null 2>&1 || true
    if dpkg-query -W -f='${Status}' alter 2>/dev/null | grep -q 'install ok installed'; then
        sudo dpkg -r alter >/dev/null 2>&1 || true
    fi
    sudo rm -f "$MARKER"
    sudo rmdir /var/lib/alter-pm2 >/dev/null 2>&1 || true
    rm -rf "$SMOKE_DIR"
}
trap cleanup EXIT

wait_for_health() {
    for _ in $(seq 1 80); do
        if curl --fail --silent --max-time 1 http://127.0.0.1:2999/api/v1/system/health \
            | grep -Eq '"status":"(ok|degraded)"'; then
            return 0
        fi
        sleep 0.25
    done
    sudo systemctl status --no-pager alter-daemon.service || true
    sudo journalctl --no-pager -u alter-daemon.service -n 100 || true
    return 1
}

[[ -x "$BINARY" ]] || { echo "release binary is not executable: $BINARY" >&2; exit 66; }
[[ -d /run/systemd/system ]] || { echo "systemd is required for the Debian lifecycle smoke" >&2; exit 69; }

cp "$BINARY" "$SMOKE_DIR/alter"
chmod 755 "$SMOKE_DIR/alter"
(
    cd "$SMOKE_DIR"
    bash "$REPO_ROOT/scripts/build-deb.sh" ./alter 0.0.0 amd64
    bash "$REPO_ROOT/scripts/build-deb.sh" ./alter 0.0.1 amd64
)

sudo dpkg -i "$SMOKE_DIR/alter_0.0.0_amd64.deb"
dpkg-query -W -f='${Status}' alter | grep -F 'install ok installed'
systemctl cat alter-daemon.service | grep -F 'ExecStart=/usr/local/bin/alter --internal-daemon --port 2999'
sudo systemctl enable --now alter-daemon.service
wait_for_health
sudo install -m 600 /dev/null "$MARKER"
printf 'preserve\n' | sudo tee "$MARKER" >/dev/null

sudo dpkg -i "$SMOKE_DIR/alter_0.0.1_amd64.deb"
systemctl is-enabled --quiet alter-daemon.service
systemctl is-active --quiet alter-daemon.service
wait_for_health
sudo grep -Fx 'preserve' "$MARKER"
dpkg-query -W -f='${Version}' alter | grep -Fx '0.0.1'

sudo dpkg -r alter
for _ in $(seq 1 40); do
    if ! systemctl is-active --quiet alter-daemon.service; then break; fi
    sleep 0.25
done
if systemctl is-active --quiet alter-daemon.service; then
    echo "alter-daemon remained active after package removal" >&2
    sudo systemctl status --no-pager alter-daemon.service || true
    exit 1
fi
if dpkg-query -W -f='${Status}' alter 2>/dev/null | grep -q 'install ok installed'; then
    echo "alter package is still installed after dpkg -r" >&2
    exit 1
fi
if [[ ! -f "$MARKER" ]]; then
    echo "package removal deleted persisted state: $MARKER" >&2
    sudo ls -la /var/lib/alter-pm2 || true
    exit 1
fi
if [[ -e /lib/systemd/system/alter-daemon.service || -e /usr/lib/systemd/system/alter-daemon.service ]]; then
    echo "systemd unit still exists after package removal" >&2
    sudo ls -l /lib/systemd/system/alter-daemon.service /usr/lib/systemd/system/alter-daemon.service 2>/dev/null || true
    exit 1
fi

#!/usr/bin/env bash
# @group BuildSystem : Build a .deb package from a pre-compiled alter binary
#
# Usage: ./scripts/build-deb.sh <binary-path> <version> <arch>
#   binary-path  Path to the compiled alter binary
#   version      Package version (e.g. 0.3.0)
#   arch         Debian arch string: amd64 | arm64
#
# Output: alter_<version>_<arch>.deb in the current directory

set -euo pipefail

if [[ "$#" -ne 3 ]]; then
    echo "usage: $0 <binary-path> <version> <amd64|arm64>" >&2
    exit 64
fi

BINARY="$1"
VERSION="$2"
ARCH="$3"

if [[ ! -f "$BINARY" ]]; then
    echo "[build-deb] binary does not exist: $BINARY" >&2
    exit 66
fi
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]]; then
    echo "[build-deb] version must be a semantic version without path characters" >&2
    exit 64
fi
if [[ "$ARCH" != "amd64" && "$ARCH" != "arm64" ]]; then
    echo "[build-deb] architecture must be amd64 or arm64" >&2
    exit 64
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PKG_NAME="alter_${VERSION}_${ARCH}"
STAGE_DIR="$(mktemp -d)"

cleanup() { rm -rf "$STAGE_DIR"; }
trap cleanup EXIT

echo "[build-deb] Staging package: $PKG_NAME"

# @group BuildSystem > Layout : Create directory structure
mkdir -p "$STAGE_DIR/DEBIAN"
mkdir -p "$STAGE_DIR/usr/local/bin"
mkdir -p "$STAGE_DIR/lib/systemd/system"

# @group BuildSystem > Binary : Install binary
cp "$BINARY" "$STAGE_DIR/usr/local/bin/alter"
chmod 755 "$STAGE_DIR/usr/local/bin/alter"

# @group BuildSystem > Systemd : Install service unit
cp "$REPO_ROOT/packaging/systemd/alter-daemon.service" \
   "$STAGE_DIR/lib/systemd/system/alter-daemon.service"

# @group BuildSystem > Control : Generate control file with version/arch/size
INSTALLED_SIZE=$(du -sk "$STAGE_DIR/usr" | cut -f1)

sed \
    -e "s/VERSION_PLACEHOLDER/$VERSION/g" \
    -e "s/ARCH_PLACEHOLDER/$ARCH/g" \
    -e "s/SIZE_PLACEHOLDER/$INSTALLED_SIZE/g" \
    "$REPO_ROOT/packaging/debian/control" > "$STAGE_DIR/DEBIAN/control"

# @group BuildSystem > Scripts : Install maintainer scripts
cp "$REPO_ROOT/packaging/debian/postinst" "$STAGE_DIR/DEBIAN/postinst"
cp "$REPO_ROOT/packaging/debian/prerm"    "$STAGE_DIR/DEBIAN/prerm"
cp "$REPO_ROOT/packaging/debian/postrm"   "$STAGE_DIR/DEBIAN/postrm"
chmod 755 "$STAGE_DIR/DEBIAN/postinst" "$STAGE_DIR/DEBIAN/prerm" "$STAGE_DIR/DEBIAN/postrm"

# @group BuildSystem > Package : Build .deb
dpkg-deb --build --root-owner-group "$STAGE_DIR" "${PKG_NAME}.deb"
echo "[build-deb] Created: ${PKG_NAME}.deb"

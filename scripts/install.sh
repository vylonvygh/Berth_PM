#!/bin/sh
set -eu

REPO="${BERTH_REPO:-vylonvygh/Berth_PM}"
VERSION="${BERTH_VERSION:-latest}"
INSTALL_DIR="${BERTH_INSTALL_DIR:-/usr/local/bin}"
BINARY_NAME="${BERTH_BIN_NAME:-bh}"
API_BASE="${BERTH_GITHUB_API_BASE:-https://api.github.com}"
DOWNLOAD_BASE="${BERTH_GITHUB_DOWNLOAD_BASE:-https://github.com}"

if [ "$(uname -s)" != "Linux" ]; then
    printf 'berth installer supports Linux only\n' >&2
    exit 1
fi

if [ -n "${BERTH_ASSET:-}" ]; then
    asset="$BERTH_ASSET"
else
    case "$(uname -m)" in
        x86_64 | amd64) asset="berth-x86_64-unknown-linux-gnu" ;;
        *)
            printf 'unsupported architecture: %s\n' "$(uname -m)" >&2
            exit 1
            ;;
    esac
fi

if [ "$VERSION" = "latest" ]; then
    release_json="$(curl -fsSL "$API_BASE/repos/$REPO/releases/latest")"
    tag="$(printf '%s\n' "$release_json" | tr ',' '\n' | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | sed -n '1p')"
    if [ -z "$tag" ]; then
        printf 'could not determine latest release tag\n' >&2
        exit 1
    fi
else
    tag="$VERSION"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
asset_path="$tmp/$asset"
checksum_path="$tmp/checksums.txt"
release_url="$DOWNLOAD_BASE/$REPO/releases/download/$tag"

printf 'Downloading berth %s for %s\n' "$tag" "$asset"
curl -fsSL "$release_url/$asset" -o "$asset_path"
curl -fsSL "$release_url/checksums.txt" -o "$checksum_path"

expected="$(awk -v name="$asset" '$2 == name || $2 == "*" name { print $1; exit }' "$checksum_path")"
if [ -z "$expected" ]; then
    printf 'checksums.txt has no entry for %s\n' "$asset" >&2
    exit 1
fi
actual="$(sha256sum "$asset_path" | awk '{ print $1 }')"
if [ "$expected" != "$actual" ]; then
    printf 'sha256 mismatch: expected %s, got %s\n' "$expected" "$actual" >&2
    exit 1
fi

mkdir -p "$INSTALL_DIR"
if [ -w "$INSTALL_DIR" ]; then
    install -m 0755 "$asset_path" "$INSTALL_DIR/$BINARY_NAME"
elif command -v sudo >/dev/null 2>&1; then
    sudo install -m 0755 "$asset_path" "$INSTALL_DIR/$BINARY_NAME"
else
    printf '%s is not writable and sudo is unavailable\n' "$INSTALL_DIR" >&2
    exit 1
fi

printf 'Installed %s to %s/%s\n' "$tag" "$INSTALL_DIR" "$BINARY_NAME"

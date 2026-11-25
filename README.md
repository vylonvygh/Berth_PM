# Berth (`bh`)

Berth is a personal, cross-distro CLI package manager for Linux that manages
standalone executables. It installs, updates, lists, rolls back, and removes
binaries the way `brew`, `apt`, or `npm` do — scoped to one user managing
one-off tools.

> **Status:** alpha (`v0.1.0-alpha.1`). Berth is experimental: expect breaking
> changes between releases. The full specification is in [SPEC.md](SPEC.md).

## Prerequisites

Berth targets Linux. For the prebuilt installer you need `curl`, `sha256sum`,
`install`, and usually `sudo`. On Fedora:

```sh
sudo dnf install -y curl coreutils sudo
```

Debian/Ubuntu:

```sh
sudo apt update && sudo apt install -y curl coreutils sudo
```

Arch Linux:

```sh
sudo pacman -S --needed curl coreutils sudo
```

For the Cargo method you also need Rust, Cargo, and a C compiler. On Fedora:

```sh
sudo dnf install -y rust cargo gcc
```

Debian/Ubuntu:

```sh
sudo apt update && sudo apt install -y build-essential cargo rustc
```

Arch Linux:

```sh
sudo pacman -S --needed base-devel rust cargo
```

You can also install Rust with [rustup](https://rustup.rs), then reopen your
shell so `$HOME/.cargo/bin` is on `PATH`.

## Installation

### Prebuilt binary (Linux x86_64)

> **Not usable yet:** this `curl` installer needs a published GitHub release,
> which does not exist yet. Use the Cargo method below until it does.

When the alpha release is published, this will install to `/usr/local/bin/bh`:

```sh
curl -fsSL https://github.com/vylonvygh/Berth_PM/releases/latest/download/install.sh | sudo sh
```

The installer downloads the release binary and `checksums.txt`, verifies the
SHA-256 checksum, then installs it as `/usr/local/bin/bh`. Set `BERTH_VERSION`
to pin a specific release tag.

### From Cargo (works now, needs Rust)

```sh
cargo install --git https://github.com/vylonvygh/Berth_PM.git --branch main
sudo install -m 0755 "$HOME/.cargo/bin/berth" /usr/local/bin/bh
```

Cargo installs the executable as `berth`; the second command copies it to
`/usr/local/bin/bh`. Check the result with `bh --version`. If `bh` is not found,
reopen your shell or confirm `/usr/local/bin` is on `PATH`.

## How it works

Berth never copies binaries into `~/.local/bin`. Real binaries live in a
versioned store, and Berth manages symlinks from `~/.local/bin`:

```text
~/.local/bin/gh -> ~/.local/share/berth/bin/ghcli/v2.43.0/gh
```

Old versions stay in the store after an update, which makes rollback possible.
Removing a tool deletes only the symlink and its registry entry; the cached
binary remains.

## Commands (v0.1)

| Command | Description |
|---|---|
| `bh add <name>` | Install from a GitHub release or custom URL/tag |
| `bh remove <name>` | Remove the symlink and registry entry |
| `bh list` | List installed tools (`--json` for scripts) |
| `bh update <name>` | Update to a newer release, keeping the old version |
| `bh info <name>` | Show version, path, checksum, and source |
| `bh rollback <name>` | Restore the last-known-good version |
| `bh sync` | Rebuild broken symlinks, warn on missing binaries |
| `bh search <query>` | Filter the local registry |
| `bh status` | Check for dangling symlinks |

Global `--quiet` and `--no-color` flags are specified so output stays pipeable.

## Locations

| Path | Purpose |
|---|---|
| `~/.config/berth/config.toml` | Config, including optional `gh_token` |
| `~/.local/share/berth/registry.toml` | Install manifest (source of truth) |
| `~/.local/share/berth/bin/` | Versioned real binaries |
| `~/.local/share/berth/cache/` | Downloads and previous versions |
| `~/.local/bin/` | Managed symlinks |

Set `sources.gh_token` in the config only if you hit GitHub's anonymous rate
limit or need access to private repositories.

## Requirements

See Prerequisites for install-time tools. Berth itself has no distro-specific
runtime packages and makes no shell profile changes.

## License

MIT — see [LICENSE](LICENSE).

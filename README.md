# Berth (`bh`)

Berth is a personal, cross-distro CLI package manager for Linux that manages
standalone executables. It installs, updates, lists, rolls back, and removes
binaries the way `brew`, `apt`, or `npm` do — scoped to one user managing
one-off tools.

> **Status:** the v0.1 specification is complete in [SPEC.md](SPEC.md).
> The Rust implementation has not been written yet.

## How it works

Berth never copies binaries into `~/.local/bin`. Real binaries live in a
versioned store, and Berth manages symlinks from `~/.local/bin`:

```text
~/.local/bin/gh -> ~/.local/share/berth/bin/ghcli/v2.43.0/gh
```

Old versions stay in the store after an update, which makes rollback possible.
Removing a tool deletes only the symlink and its registry entry; the cached
binary remains.

## Planned commands (v0.1)

| Command | Description |
|---|---|
| `berth add <name>` | Install from a GitHub release or custom URL/tag |
| `berth remove <name>` | Remove the symlink and registry entry |
| `berth list` | List installed tools (`--json` for scripts) |
| `berth update <name>` | Update to a newer release, keeping the old version |
| `berth info <name>` | Show version, path, checksum, and source |
| `berth rollback <name>` | Restore the last-known-good version |
| `berth sync` | Rebuild broken symlinks, warn on missing binaries |
| `berth search <query>` | Filter the local registry |
| `berth status` | Check for dangling symlinks |

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

- Linux, with `~/.local/bin` available for symlinks
- Rust toolchain (edition 2021 or newer) to build from source

No distro-specific packages, hooks, or shell scripts are required.

## License

MIT — see [LICENSE](LICENSE).

# Installation & Quick Start

## Install

```bash
cargo install --path .
```

## Quick start

Add this to your `~/.bashrc` or `~/.zshrc`:

```bash
eval "$(lhi activate)"
```

That's it. Now whenever you `cd` into a project that has a `.lhi/` directory, a watcher starts automatically in the background. Multiple projects can be watched concurrently — each gets its own watcher process. All watchers are cleaned up when the shell exits.

To initialize a new project, just run `lhi watch` once (it creates `.lhi/` on first run), then let the shell hook handle it from there.

```bash
# Check what changed
lhi log
lhi log src/main.rs
lhi log --since 30m
lhi log --branch main

# View an old version
lhi cat <hash>

# Compare two versions
lhi diff <hash1> <hash2>

# Search through stored file versions
lhi search "fn main"
lhi search "TODO" --file src/lib.rs

# Check storage usage
lhi info

# Restore files to 5 minutes ago
lhi restore --before 5m --dry-run
lhi restore --before 5m

# Take a manual snapshot
lhi snapshot --label "before refactor"

# Shrink the index
lhi compact
```

## Logging

`lhi` uses `tracing` for structured logging. Control verbosity with `RUST_LOG`:

```bash
RUST_LOG=lhi=debug lhi watch    # verbose
RUST_LOG=lhi=trace lhi watch    # very verbose
```

Default level is `info` (warnings and errors only).

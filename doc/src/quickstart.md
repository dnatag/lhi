# Installation & Quick Start

## Install

```bash
cargo install --path .
```

## Quick start

```bash
# Initialize a project
cd ~/my-project
lhi init

# Add this to your ~/.bashrc or ~/.zshrc for automatic watching
eval "$(lhi activate)"
```

That's it. `lhi init` creates the `.lhi/` directory and adds it to `.gitignore`. The shell hook automatically starts a watcher whenever you `cd` into a project with `.lhi/`. Multiple projects can be watched concurrently — each gets its own watcher process. All watchers are cleaned up when the shell exits.

```bash
# Check what changed
lhi log src/main.rs          # shows ~1, ~2, ~3... revision numbers

# View an old version
lhi cat src/main.rs           # latest stored version
lhi cat src/main.rs ~3        # 3rd most recent
lhi cat a1b2c3d4              # by short hash prefix

# Compare versions
lhi diff src/main.rs          # latest stored vs current disk
lhi diff src/main.rs ~5       # revision ~5 vs current disk
lhi diff src/main.rs ~3 ~1    # compare two revisions

# Search through stored file versions
lhi search "fn main"
lhi search "TODO" --file src/lib.rs

# Restore files
lhi restore src/main.rs ~5           # restore single file to revision
lhi restore --at a1b2c3d4            # restore project to that moment
lhi restore --at a1b2c3d4 --dry-run  # preview first
lhi restore --snapshot "before refactor"  # restore to a named snapshot

# Other commands
lhi info                              # storage statistics
lhi snapshot --label "before refactor"  # manual snapshot
lhi compact                           # shrink the index
```

## Logging

`lhi` uses `tracing` for structured logging. Control verbosity with `RUST_LOG`:

```bash
RUST_LOG=lhi=debug lhi watch    # verbose
RUST_LOG=lhi=trace lhi watch    # very verbose
```

Default level is `info` (warnings and errors only).

The shell hook logs watcher stderr to `~/.lhi-watch.log` for troubleshooting.

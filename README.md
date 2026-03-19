# lhi

Local history for your code — like IntelliJ's Local History, but for any editor.

`lhi` watches a directory for file changes and maintains a local version history. Every save is captured automatically with content-addressed storage and a JSONL index. No server, no network, no config — just a `.lhi/` directory at your project root.

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

## Commands

### `lhi activate`

Prints a shell hook script to stdout. Designed to be `eval`'d in your shell rc file:

```bash
eval "$(lhi activate)"
```

The hook:
- Overrides `cd`, `pushd`, and `popd` to detect `.lhi/` projects
- Walks up parent directories (so `cd ~/project/src/deep` activates `~/project`)
- Starts `lhi watch` in the background for each new project entered
- Tracks multiple concurrent watchers (one per project root)
- Re-launches a watcher if its process dies
- Kills all watchers on shell exit (`EXIT` trap)

Shell-specific implementations are emitted for portability:
- **bash** — uses a newline-delimited string to track watchers (compatible with bash 3.2 on macOS)
- **zsh** — uses native `typeset -A` associative arrays with zsh syntax

To manually stop all watchers and remove the hook, run `_lhi_deactivate` in your shell.

Supports bash and zsh. Fish support is planned.

### `lhi watch [PATH]`

Watch a directory for file changes. Records every create, modify, and delete to `.lhi/`.

```
Options:
  -v, --verbose  Print events as JSON to stdout
```

Runs in the foreground (blocking). Useful for troubleshooting or one-off use. The `lhi activate` shell hook uses this command internally.

On first run, captures a baseline snapshot of all existing files. Respects `.gitignore`. Debounces rapid writes (100ms window). Files over 10MB are skipped.

### `lhi log [FILE]`

Show change history.

```
Options:
  --since <DURATION>  Filter by time (e.g. 5m, 1h, 2d)
  --branch <NAME>     Filter by git branch
  --json              Output as JSON
```

When git branch tracking is available, each entry shows the branch it was recorded on.

### `lhi cat <HASH>`

Print the content of a stored file version by its SHA-256 hash (from `lhi log` output).

### `lhi diff <HASH1> <HASH2>`

Show a unified diff between two stored file versions. Output is colorized when stdout is a terminal.

### `lhi search <QUERY>`

Search through stored file contents for a text pattern (case-insensitive).

```
Options:
  --file <PATH>  Search only versions of this file
```

Searches each unique blob once, showing matching lines with file path, timestamp, and line numbers.

### `lhi info`

Show storage statistics: index entries, files tracked, blob count, blob size, and total `.lhi/` disk usage.

### `lhi restore [FILE] --before <TIME>`

Restore files to their state before a point in time.

```
Options:
  --before <TIME>  Required. Accepts: 5m, 1h, 14:30, ISO 8601
  --dry-run        Preview without making changes
  --json           Output as JSON
```

Compares stored hashes against current disk state — only overwrites files that actually changed. Restores Unix file permissions. Deletes files that didn't exist at the target time.

### `lhi snapshot [--label <LABEL>]`

Capture a full project snapshot. Useful before risky changes.

### `lhi compact`

Compact the index to keep only the latest entry per file. Reduces `.lhi/index.jsonl` size.

## How it works

```
.lhi/
├── index.jsonl    Append-only event log (one JSON line per change)
└── blobs/         Content-addressed file storage (SHA-256, zstd-compressed)
    ├── a1b2c3...
    └── d4e5f6...
```

- **Blob store:** Files are stored by their SHA-256 hash. Identical content is automatically deduplicated. Blobs are zstd-compressed on write; old uncompressed blobs are read transparently. Writes are atomic (temp file + rename).
- **Index:** JSONL format — each line records timestamp, event type, file path, content hash, size, and git branch. Append-only during normal operation; `compact` rewrites it.
- **Watcher:** Uses OS-native filesystem notifications (`notify` crate) with 100ms debouncing.
- **Git integration:** Automatically records the current git branch with each event (captured at watcher startup and snapshot time).

## Logging

`lhi` uses `tracing` for structured logging. Control verbosity with `RUST_LOG`:

```bash
RUST_LOG=lhi=debug lhi watch    # verbose
RUST_LOG=lhi=trace lhi watch    # very verbose
```

Default level is `info` (warnings and errors only).

## Project structure

```
src/
├── lib.rs              Module root
├── util.rs             Shared utilities (SHA-256, file mode, git branch)
├── core/
│   ├── event.rs        Event data model (EventType, LhiEvent, etc.)
│   ├── index.rs        JSONL index (read/write/query/compact)
│   └── store.rs        Content-addressed blob store (zstd-compressed)
├── commands/
│   ├── activate.rs     lhi activate (shell hook generation, bash + zsh)
│   ├── cat.rs          lhi cat
│   ├── diff.rs         lhi diff
│   ├── info.rs         lhi info
│   ├── log.rs          lhi log
│   ├── search.rs       lhi search
│   ├── compact.rs      lhi compact
│   ├── snapshot.rs     lhi snapshot
│   ├── restore.rs      lhi restore
│   └── watch.rs        lhi watch
├── watcher/
│   ├── mod.rs          LhiWatcher, baseline snapshot
│   ├── events.rs       Debounced event loop
│   └── helpers.rs      Watcher-specific helpers
└── bin/lhi/
    ├── main.rs         Entry point
    └── cli.rs          Clap CLI definition
docs/
├── reviewsmith-report.md   Code review report
└── developer-guide.md      Developer walkthrough for activate module
```

## License

MIT

# lhi

Local history for your code — like IntelliJ's Local History, but for any editor.

`lhi` watches a directory for file changes and maintains a local version history. Every save is captured automatically with content-addressed storage and a JSONL index. No server, no network, no config — just a `.lhi/` directory at your project root.

## Install

```bash
cargo install --path .
```

## Quick start

```bash
# Start watching the current directory
lhi watch

# In another terminal — check what changed
lhi log
lhi log src/main.rs
lhi log --since 30m

# View an old version
lhi cat <hash>

# Restore files to 5 minutes ago
lhi restore --before 5m --dry-run
lhi restore --before 5m

# Take a manual snapshot
lhi snapshot --label "before refactor"

# Shrink the index
lhi compact
```

## Commands

### `lhi watch [PATH]`

Watch a directory for file changes. Records every create, modify, and delete to `.lhi/`.

```
Options:
  -v, --verbose  Print events as JSON to stdout
  -d, --daemon   Run as a background daemon

Subcommands:
  stop           Stop the background watcher daemon
  status         Check if the background watcher is running
```

On first run, captures a baseline snapshot of all existing files. Respects `.gitignore`. Debounces rapid writes (100ms window). Files over 10MB are skipped.

**Daemon mode:**

```bash
lhi watch --daemon          # start in background
lhi watch status            # check if running
lhi watch stop              # stop the daemon
```

When running as a daemon, output goes to `.lhi/watch.log` and the PID is stored in `.lhi/watch.pid`.

### `lhi log [FILE]`

Show change history.

```
Options:
  --since <DURATION>  Filter by time (e.g. 5m, 1h, 2d)
  --json              Output as JSON
```

### `lhi cat <HASH>`

Print the content of a stored file version by its SHA-256 hash (from `lhi log` output).

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
└── blobs/         Content-addressed file storage (SHA-256)
    ├── a1b2c3...
    └── d4e5f6...
```

- **Blob store:** Files are stored by their SHA-256 hash. Identical content is automatically deduplicated. Writes are atomic (temp file + rename).
- **Index:** JSONL format — each line records timestamp, event type, file path, content hash, and size. Append-only during normal operation; `compact` rewrites it.
- **Watcher:** Uses OS-native filesystem notifications (`notify` crate) with 100ms debouncing.

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
├── util.rs             Shared utilities (SHA-256, file mode)
├── core/
│   ├── event.rs        Event data model (EventType, LhiEvent, etc.)
│   ├── index.rs        JSONL index (read/write/query/compact)
│   └── store.rs        Content-addressed blob store
├── commands/
│   ├── cat.rs          lhi cat
│   ├── log.rs          lhi log
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
```

## License

MIT

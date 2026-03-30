# lhi

Local history for your code — like IntelliJ's Local History, but for any editor.

`lhi` watches a directory for file changes and maintains a local version history. Every save is captured automatically with content-addressed storage and a JSONL index. No server, no network, no config — just a `.lhi/` directory at your project root.

## Install

### Homebrew (macOS/Linux)

```bash
brew install dnatag/tap/lhi
```

### From source

```bash
cargo install --path .
```

### From GitHub releases

Download the latest binary from [Releases](https://github.com/dnatag/lhi/releases), extract, and place in your `$PATH`.

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
lhi diff a1b2c3d4 e5f6a7b8   # by short hash prefixes

# Search through stored file versions
lhi search "fn main"
lhi search "TODO" --file src/lib.rs

# Restore files
lhi restore src/main.rs ~5           # restore single file to revision
lhi restore --at a1b2c3d4            # restore project to that moment
lhi restore --at a1b2c3d4 --dry-run  # preview first
lhi restore --before 5m              # time-based restore

# Other commands
lhi info                              # storage statistics
lhi snapshot --label "before refactor"  # manual snapshot
lhi compact                           # shrink the index
```

## Commands

### `lhi init [PATH]`

Initialize a `.lhi/` directory for a project. Creates `.lhi/blobs/` and adds `.lhi/` to `.gitignore` if one exists. Safe to run multiple times (idempotent).

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
- Logs watcher errors to `~/.lhi-watch.log` and warns on failed launches
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

Only one watcher can run per project — a PID lock file (`.lhi/watcher.pid`) prevents duplicates. If a watcher is already running, the command prints a message and exits cleanly. Stale lock files from crashed processes are handled automatically (the OS releases the file lock on process exit).

On first run, captures a baseline snapshot of all existing files. Respects `.gitignore`. Debounces rapid writes (100ms window). Files over 10MB are skipped. Metadata-only events (where the file content hasn't changed) are silently dropped.

### `lhi log [FILE]`

Show change history. When filtered to a single file, shows `~N` revision numbers (`~1` = newest).

```
Options:
  --since <DURATION>  Filter by time (e.g. 5m, 1h, 2d)
  --branch <NAME>     Filter by git branch
  --json              Output as JSON
  -f, --follow        Continuously watch for new entries (like tail -f)
```

When git branch tracking is available, each entry shows the branch it was recorded on.

With `--follow`, prints existing history then polls for new index entries every 500ms. Combines with `--since`, `--branch`, and file filters. Press `q` to stop (or Ctrl+C).

### `lhi cat <TARGET> [~N]`

Print the content of a stored file version. Accepts a hash (or short prefix), a file path (shows latest), or a file path with `~N` revision.

```bash
lhi cat a1b2c3d4          # by hash prefix
lhi cat src/main.rs        # latest version
lhi cat src/main.rs ~3     # 3rd most recent
```

When stdout is a terminal, output is syntax-highlighted with line numbers and a grid border (powered by [bat](https://github.com/sharkdp/bat)). The language is auto-detected from the filename in the index. When piped, raw content is emitted for composability.

### `lhi diff <ARG1> [ARG2] [ARG3]`

Show a unified diff between file versions. Supports multiple forms:

```bash
lhi diff a1b2c3d4 e5f6a7b8   # two hash prefixes
lhi diff src/main.rs ~3 ~1    # file with two revisions
lhi diff src/main.rs ~5       # revision vs current file on disk
lhi diff src/main.rs          # latest stored vs current disk
```

When stdout is a terminal, the diff is rendered with syntax highlighting. If [delta](https://github.com/dandavison/delta) is installed, it is used automatically for rich side-by-side output. Otherwise, falls back to bat's Diff syntax highlighting. When piped, standard unified diff format is emitted.

### `lhi search <QUERY>`

Search through stored file contents for a text pattern (case-insensitive).

```
Options:
  --file <PATH>  Search only versions of this file
```

Searches each unique blob once. When stdout is a terminal, matching lines are shown with syntax-highlighted context (2 lines above and below), line numbers, and highlighted match lines. When piped, plain text output is emitted.

### `lhi info`

Show storage statistics: index entries, files tracked, blob count, blob size, and total `.lhi/` disk usage.

### `lhi restore [FILE] [~N]`

Restore files to a previous state. Supports multiple modes:

```bash
lhi restore src/main.rs ~5           # single file to revision
lhi restore src/main.rs --at a1b2    # single file to specific hash
lhi restore --at a1b2c3d4            # all files to that moment
lhi restore --before 5m              # all files to 5 minutes ago
```

```
Options:
  --at <HASH>    Restore to the moment a specific hash was recorded
  --before <TIME>  Restore to before a time (5m, 1h, 14:30, ISO 8601)
  --dry-run        Preview without making changes
  --json           Output as JSON
```

Compares stored hashes against current disk state — only overwrites files that actually changed. Restores Unix file permissions. Deletes files that didn't exist at the target time.

### `lhi snapshot [--label <LABEL>]`

Capture a full project snapshot. Useful before risky changes.

### `lhi compact`

Compact the index to keep only the latest entry per file. Reduces `.lhi/index.jsonl` size.

```
Options:
  --dedup-only  Only remove consecutive duplicate entries (preserve history)
```

Without flags, first deduplicates consecutive identical entries, then collapses to one entry per file. With `--dedup-only`, removes duplicates while preserving the full change history.

## How it works

```
.lhi/
├── index.jsonl    Append-only event log (one JSON line per change)
└── blobs/         Content-addressed file storage (SHA-256, zstd-compressed)
    ├── a1b2c3...
    └── d4e5f6...
```

- **Blob store:** Files are stored by their SHA-256 hash. Identical content is automatically deduplicated. Blobs are zstd-compressed on write; old uncompressed blobs are read transparently. Writes are atomic (temp file + rename). Short hash prefixes are resolved by scanning the blobs directory.
- **Index:** JSONL format — each line records timestamp, event type, file path, content hash, size, and git branch. Append-only during normal operation; `compact` rewrites it. Appends are protected by `fs2` file locks to prevent interleaved writes from concurrent processes.
- **Watcher:** Uses OS-native filesystem notifications (`notify` crate) with 100ms debouncing. Ignores `.lhi/` directories at any nesting depth. A PID lock file (`.lhi/watcher.pid`) ensures only one watcher runs per project. Metadata-only events (unchanged content) are silently dropped.
- **Git integration:** Automatically records the current git branch with each event (captured at watcher startup and snapshot time).

## Logging

`lhi` uses `tracing` for structured logging. Control verbosity with `RUST_LOG`:

```bash
RUST_LOG=lhi=debug lhi watch    # verbose
RUST_LOG=lhi=trace lhi watch    # very verbose
```

Default level is `info` (warnings and errors only).

The shell hook logs watcher stderr to `~/.lhi-watch.log` for troubleshooting.

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
│   ├── cat.rs          lhi cat (syntax-highlighted file viewing)
│   ├── diff.rs         lhi diff (delta/bat-powered diff)
│   ├── info.rs         lhi info
│   ├── init.rs         lhi init
│   ├── log.rs          lhi log
│   ├── search.rs       lhi search (highlighted context matches)
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
doc/
└── src/                mdbook documentation source
```

## Troubleshooting

### `(eval):2: parse error near '}'` on `source ~/.zshrc`

This happened in older versions when the shell hook was eval'd twice (e.g. re-sourcing your rc file). The hook tried to save the existing `cd` function using `declare -f cd | tail -n +2`, which strips the opening `{` in zsh. This was fixed — update `lhi` and open a fresh shell. If your current session is stuck, reset it:

```bash
unset -f cd _lhi_orig_cd pushd popd _lhi_hook _lhi_find_root _lhi_deactivate 2>/dev/null
source ~/.zshrc
```

### `lhi` commands are killed immediately (`killed` or exit code 137)

macOS Gatekeeper can SIGKILL unsigned or invalidly-signed binaries. This happens if you copy the `lhi` binary manually (e.g. `cp`) — the copy invalidates the code signature. Fix it by re-signing:

```bash
codesign -f -s - $(which lhi)
```

Or reinstall with `cargo install --path .`, which produces a properly signed binary.

### `zsh: command not found: _lhi_orig_cd`

The shell hook failed to load (usually due to the parse error above), leaving `cd` in a broken state. Open a new terminal, or reset manually:

```bash
unset -f cd _lhi_orig_cd 2>/dev/null
source ~/.zshrc
```

## License

MIT

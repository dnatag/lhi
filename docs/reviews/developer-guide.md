# lhi — Developer Guide

<!-- reviewsmith:guide -->
<!-- reviewsmith:generated:2026-03-15T22:32:00-04:00 -->
<!-- reviewsmith:scope:full -->
<!-- reviewsmith:modules:bin_lhi,commands,core,watcher -->

> This guide is written for a human developer working on this project without
> AI assistance. It explains what every module and function does, how they
> connect, what the known issues are, and how to fix them.

## Table of Contents

- [What is lhi?](#what-is-lhi)
- [How it works (big picture)](#how-it-works-big-picture)
- [Project layout](#project-layout)
- [Module-by-module walkthrough](#module-by-module-walkthrough)
  - [bin/lhi — CLI entry point](#binlhi--cli-entry-point)
  - [core: event — Data model](#core-event--data-model)
  - [core: index — JSONL index](#core-index--jsonl-index)
  - [core: store — Blob storage](#core-store--blob-storage)
  - [commands — CLI command implementations](#commands--cli-command-implementations)
  - [watcher — Filesystem monitoring](#watcher--filesystem-monitoring)
- [Data flow](#data-flow)
- [Known issues & how to fix them](#known-issues--how-to-fix-them)
- [Things to watch out for when making changes](#things-to-watch-out-for-when-making-changes)

## What is lhi?

lhi (Local History for IntelliJ-like local history) is a CLI tool that watches a directory for file changes and maintains a local version history. Think of it as a lightweight, always-on version control that captures every save — similar to IntelliJ's "Local History" feature but for any editor.

Users interact with it through subcommands: `lhi watch` to start monitoring, `lhi log` to browse history, `lhi cat` to view old file contents, `lhi restore` to roll back files, `lhi snapshot` to capture a manual checkpoint, and `lhi compact` to shrink the index.

All data lives in a `.lhi/` directory at the project root. There's no server, no network, no config file — just a blob store and a JSONL index.

## How it works (big picture)

When you run `lhi watch`, the tool:

1. Canonicalizes the target directory and loads `.gitignore` rules
2. If this is the first run (empty index), walks all files and records a "baseline" snapshot — storing each file's content in the blob store and recording metadata in the index
3. Starts a `notify` filesystem watcher with an mpsc channel
4. Enters a debounce loop: raw filesystem events are coalesced (100ms window) so rapid saves to the same file produce a single event
5. For each debounced event: reads the file, stores the blob (content-addressed by SHA-256), records an index entry, and optionally emits JSON to stdout

The blob store (``.lhi/blobs/``) is content-addressed: each file is stored under its SHA-256 hash. Identical content is automatically deduplicated. The index (`.lhi/index.jsonl`) is an append-only JSONL file where each line records a timestamp, event type, file path, content hash, and size.

Commands like `log`, `restore`, and `cat` read from these two data structures. `restore` compares the stored state at a point in time against the current disk state and overwrites changed files.

## Project layout

```
src/
├── lib.rs                  — Module re-exports (commands, event, index, store, watcher)
├── event.rs                — Event data model: EventType, Project, FileInfo, Snapshot, Diff, LhiEvent
├── index.rs                — JSONL index: IndexEntry struct, Index with read/write/query/compact
├── store.rs                — Content-addressed blob store: BlobStore with store/read/has
├── bin/lhi/
│   ├── main.rs             — Entry point: delegates to cli::run(), handles exit code
│   └── cli.rs              — Clap CLI definition (Cli struct, Command enum) + dispatch
├── commands/
│   ├── mod.rs              — Shared utilities (time parsing, file mode) + re-exports
│   ├── cat.rs              — `lhi cat <hash>`: print blob content to stdout
│   ├── log.rs              — `lhi log`: display change history with filters
│   ├── compact.rs          — `lhi compact`: shrink index to latest-per-file
│   ├── snapshot.rs         — `lhi snapshot`: capture full project state
│   ├── restore.rs          — `lhi restore`: roll back files to a point in time
│   └── watch.rs            — `lhi watch`: start filesystem watcher
└── watcher/
    ├── mod.rs              — LhiWatcher struct, constructor, baseline snapshot
    ├── events.rs           — Debounced event loop: next_event, flush_ready, build_event, is_ignored
    └── helpers.rs          — Shared helpers: hex_sha256, get_file_mode, is_ignored_by (test-only)
```

All files except `bin/lhi/` are library code (`lib.rs` is the crate root). Tests are inline (`#[cfg(test)] mod tests`) in most files.

## Module-by-module walkthrough

<!-- reviewsmith:module:bin_lhi -->
### bin/lhi — CLI entry point

This is the thinnest possible CLI layer. `main.rs` calls `cli::run()` and converts errors to a user-friendly message with exit code 1. `cli.rs` defines the `Cli` struct and `Command` enum using clap's derive macros, then dispatches each variant to the corresponding function in `lhi::commands`.

**Functions:**

- `main()` (main.rs:3) — Entry point. Calls `cli::run()`, prints `"lhi: {error}"` on failure, exits with code 1. No business logic here.
- `run()` (cli.rs:65) — Parses CLI args via `Cli::parse()`, matches on `Command` variants, delegates to `commands::*` functions. All type conversions happen here (`as_deref()`, reference passing).

**If you're modifying this module:** Adding a new command means adding a variant to the `Command` enum, a match arm in `run()`, and a new file in `commands/`. The pattern is consistent across all existing commands. All commands return `io::Result<()>` — if you need richer errors, this is where you'd change the return type.

<!-- reviewsmith:end:bin_lhi -->

<!-- reviewsmith:module:core_event -->
### core: event — Data model

`event.rs` defines the serializable data structures that represent filesystem events. These types are used by both the watcher (which creates events) and the commands (which read them back from the index).

**Types:**

- `EventType` — Enum: Modify, Create, Delete, Rename. Serializes as snake_case strings.
- `Project` — Root path + whether gitignore is respected. `gitignore_respected` defaults to `true` via `default_true()`.
- `FileInfo` — Absolute path, relative path, optional old_path (for renames).
- `Snapshot` — Content hash, size in bytes, optional label.
- `Diff` — Previous hash + patch string. **Note: patch is never populated** — it's always an empty string. This is a placeholder for a future feature.
- `LhiEvent` — The top-level event combining all of the above with a version number and timestamp.

**Functions:**

- `default_true()` (line 20) — Serde default function for `Project.gitignore_respected`. Returns `true`.

**If you're modifying this module:** These types are serialized to JSONL. Any field changes affect the on-disk format. Adding fields is safe (use `#[serde(default)]`). Removing or renaming fields will break reading old index entries. The `Diff.patch` field is dead code — it's serialized but never contains useful data.

<!-- reviewsmith:end:core_event -->

<!-- reviewsmith:module:core_index -->
### core: index — JSONL index

`index.rs` manages the append-only JSONL index at `.lhi/index.jsonl`. Each line is a JSON-serialized `IndexEntry`. The `Index` struct wraps a `PathBuf` to this file and provides read/write/query operations.

**Types:**

- `IndexEntry` — Timestamp, event_type (String, not the enum), path, relative_path, content_hash, size_bytes, label, file_mode. Note that `event_type` is a plain String here, not the `EventType` enum from `event.rs` — this is a design inconsistency but means the index format is decoupled from the enum.

**Functions:**

- `Index::open(root)` (line 25) — Creates `.lhi/` directory if needed, returns `Index` pointing to `index.jsonl`. Does not create the file itself — that happens on first `append`.
- `Index::append(entry)` (line 32) — Opens file in append mode, serializes entry to JSON, writes a line. No file locking — concurrent writes from multiple processes could interleave.
- `Index::read_all()` (line 42) — Reads all entries. Returns empty vec if file doesn't exist. **Silently skips malformed lines** — if the JSONL is corrupted, you won't know.
- `Index::query_file(relative_path)` (line 56) — Reads all entries, filters by relative_path. O(n) every call.
- `Index::query_since(since)` (line 63) — Reads all entries, filters by timestamp >= cutoff. O(n).
- `Index::state_at(before)` (line 70) — Returns the latest entry per file at or before the timestamp. Uses a HashMap keyed by relative_path, so the last entry seen for each file wins. This is the core of `restore`.
- `Index::all_known_paths()` (line 89) — Returns a HashSet of all relative paths ever recorded. Used by `restore` to find files that should be deleted.
- `Index::compact()` (line 97) — Rewrites the index keeping only the latest entry per file. Uses atomic write-to-temp-then-rename pattern. Sorts by timestamp before writing.

**If you're modifying this module:** Every query method calls `read_all()` which reads the entire file. This is fine for small-to-medium indices but would need optimization (e.g., an in-memory cache or SQLite) for very large projects. The `compact()` method is the only one that does atomic writes — `append()` does not, so a crash mid-append could leave a partial line (which `read_all` would silently skip).

<!-- reviewsmith:end:core_index -->

<!-- reviewsmith:module:core_store -->
### core: store — Blob storage

`store.rs` implements content-addressed blob storage in `.lhi/blobs/`. Files are stored under their SHA-256 hash, providing automatic deduplication.

**Functions:**

- `BlobStore::init(root)` (line 12) — Creates `.lhi/blobs/` directory, returns `BlobStore`.
- `BlobStore::store_blob(content)` (line 18) — Hashes content with SHA-256, writes to `blobs/{hash}` if not already present. Returns the hash. **Not atomic** — a crash mid-write leaves a partial blob that won't be overwritten on retry (the hash file already "exists").
- `BlobStore::read_blob(hash)` (line 26) — Reads and returns blob bytes. Errors if not found.
- `BlobStore::has_blob(hash)` (line 30) — Checks if blob file exists. Does not verify content integrity.
- `BlobStore::blob_path(hash)` (line 34) — Returns `blobs_dir.join(hash)`. Private helper.
- `hex_sha256(data)` (line 40) — Computes SHA-256 hash as lowercase hex string. **Duplicated** in `watcher/helpers.rs`.

**If you're modifying this module:** The blob store has no garbage collection — blobs are never deleted even after `compact`. The flat directory structure (all blobs in one dir) could become slow with many thousands of files; a two-level hash prefix scheme (like git's `objects/ab/cdef...`) would help. The `hex_sha256` function is private to this module but identical code exists in `watcher/helpers.rs` — these should be unified.

<!-- reviewsmith:end:core_store -->

<!-- reviewsmith:module:commands -->
### commands — CLI command implementations

The `commands/` directory contains one file per CLI subcommand plus `mod.rs` with shared utilities. Each command function takes parsed arguments and returns `io::Result<()>`.

**Shared utilities (mod.rs):**

- `MAX_FILE_SIZE` — 10MB constant. Files larger than this are skipped.
- `parse_since(s)` (line 21) — Delegates to `parse_duration_ago`. Accepts "5m", "1h", "2d".
- `parse_before(s)` (line 25) — Tries three formats in order: relative duration ("5m"), ISO 8601 ("2026-03-14T10:30:00Z"), HH:MM local time ("14:30"). **Bug: `unwrap()` on `and_local_timezone()` panics during DST transitions.**
- `parse_duration_ago(s)` (line 38) — Strips " ago" suffix, splits number from unit, supports s/m/h/d and their long forms. Accepts negative numbers (produces future timestamps — minor bug).
- `get_file_mode(meta)` (line 57/64) — Platform-conditional: returns Unix permissions on Unix, `None` elsewhere. **Duplicated** in `watcher/helpers.rs`.

**Commands:**

- `cat(hash)` (cat.rs:6) — Inits BlobStore from current dir, reads blob by hash, writes raw bytes to stdout. Simple and correct.
- `log(file, since, json)` (log.rs:10) — Opens Index, applies file/since filters, outputs as formatted table or JSON. **Bug: `h[..8]` panics on hashes shorter than 8 chars.**
- `compact()` (compact.rs:6) — Opens Index, calls `index.compact()`, prints count. Thin wrapper.
- `snapshot(label)` (snapshot.rs:12) — Walks project with `ignore::WalkBuilder`, skips non-files/symlinks/.lhi/large files, stores each blob, appends index entry with "snapshot" event type. Each file gets a separate `Utc::now()` timestamp (ideally should be one timestamp for the whole snapshot).
- `restore(file, before, dry_run, json)` (restore.rs:13) — The most complex command. Parses cutoff time, gets `state_at(cutoff)`, computes `RestoreAction` for each entry by comparing current disk content hash against stored hash. In non-dry-run mode, restores file content and Unix permissions, deletes files created after cutoff. Supports `--file` filter, `--dry-run`, `--json`.
- `to_restore_action(root, entry)` (restore.rs:93) — Compares an index entry against current disk state. Returns `Some(RestoreAction)` if the file needs restoring or deleting, `None` if it matches.
- `watch(path, verbose)` (watch.rs:8) — Creates `LhiWatcher`, prints canonical path to stderr, loops `next_event()` printing JSON to stdout if verbose. When verbose=false, events are still recorded to the index but nothing is printed.

**If you're modifying this module:** All commands assume `current_dir()` is the project root. The `restore` command is the most complex and has the most edge cases — test thoroughly. The time parsing in `parse_before` needs the DST fix before it's safe for production use.

<!-- reviewsmith:end:commands -->

<!-- reviewsmith:module:watcher -->
### watcher — Filesystem monitoring

The `watcher/` module handles real-time filesystem monitoring. It wraps the `notify` crate with debouncing, gitignore filtering, blob storage, and index recording.

**Types:**

- `LhiWatcher` — Holds the root path, gitignore matcher, BlobStore, Index, a HashMap of previous content hashes (for diff support), the notify watcher, and an mpsc receiver for events.

**Functions (mod.rs):**

- `LhiWatcher::new(root)` (line 33) — Canonicalizes root, loads `.gitignore`, inits store and index, runs `baseline_snapshot` on first run, creates `notify::RecommendedWatcher` with mpsc channel. Returns `Box<dyn Error>` (inconsistent with rest of codebase which uses `io::Error`). The `previous_hashes` map starts empty — it's not seeded from the baseline, so the first modification after startup won't have diff context.
- `LhiWatcher::baseline_snapshot(root, store, index)` (line 60) — Only runs when index is empty. Walks all files with `ignore::WalkBuilder`, stores blobs, records "baseline" entries. **Bug: a single file read error aborts the entire baseline** via `?` operator — should `continue` on per-file errors.

**Functions (events.rs):**

- `LhiWatcher::next_event()` (line 14) — Blocking iterator. Maintains a local `HashMap<PathBuf, (EventKind, Instant)>` of pending events. Receives from the mpsc channel, coalesces events within the 100ms debounce window, calls `flush_ready` when a timeout fires. Default timeout is 60 seconds when no events are pending.
- `LhiWatcher::flush_ready(pending)` (line 57) — Finds the first pending event whose debounce window has elapsed, removes it from the map, calls `build_event`. HashMap iteration is non-deterministic so events may not emit in chronological order.
- `LhiWatcher::build_event(path, kind)` (line 73) — The core event construction function. Maps `EventKind` to `EventType`, skips symlinks, reads file content, stores blob, computes diff against previous hash, updates `previous_hashes`, appends to index, returns `LhiEvent`. **Multiple issues:** Diff.patch is always empty, read errors silently return None, index.append errors silently ignored, two redundant metadata() calls.
- `LhiWatcher::is_ignored(path)` (line 148) — Checks gitignore rules. Loaded once at startup, never refreshed.

**Functions (helpers.rs):**

- `get_file_mode(meta)` (line 5/12) — Platform-conditional Unix permissions. **Duplicate** of `commands/mod.rs` version.
- `hex_sha256(data)` (line 30) — SHA-256 hash. **Duplicate** of `store.rs` version.
- `is_ignored_by(gitignore, root, path)` (line 19) — Test-only helper. Uses relative paths and checks both file/dir modes, which differs from production `is_ignored()`.

**If you're modifying this module:** The debounce logic is subtle — `next_event()` rebuilds the pending map from scratch each call, which is correct because it's local. The `build_event` function is the highest-risk code in the project due to its many silent failure modes. The TOCTOU races (checking file state then reading) are inherent to filesystem watching and mostly unavoidable, but the silent error swallowing should be fixed with proper logging.

<!-- reviewsmith:end:watcher -->

## Data flow

### Watch flow (recording changes)

```
Filesystem change
    │
    ▼
notify::RecommendedWatcher  →  mpsc channel
    │
    ▼
LhiWatcher::next_event()    →  debounce (100ms window)
    │
    ▼
LhiWatcher::flush_ready()   →  pick first ready event
    │
    ▼
LhiWatcher::build_event()
    ├─► is_ignored()         →  skip if gitignored
    ├─► std::fs::read()      →  read file content
    ├─► BlobStore::store_blob()  →  SHA-256 hash, write to .lhi/blobs/{hash}
    ├─► Index::append()      →  write JSONL line to .lhi/index.jsonl
    └─► return LhiEvent      →  optionally printed as JSON (verbose mode)
```

### Restore flow (reading back)

```
lhi restore --before "1h"
    │
    ▼
parse_before("1h")          →  UTC timestamp
    │
    ▼
Index::state_at(cutoff)     →  latest IndexEntry per file before cutoff
    │
    ▼
to_restore_action()         →  compare stored hash vs current disk hash
    │                           (reads file, computes SHA-256)
    ▼
RestoreAction { restore | delete }
    │
    ▼
BlobStore::read_blob(hash)  →  get stored content
    │
    ▼
fs::write(target, content)  →  overwrite file on disk
```

### Snapshot flow

```
lhi snapshot --label "before refactor"
    │
    ▼
ignore::WalkBuilder::new()  →  walk all files (respects .gitignore)
    │
    ▼
For each file:
    ├─► fs::read()           →  read content
    ├─► BlobStore::store_blob()  →  store blob
    └─► Index::append()      →  record "snapshot" entry with label
```

## Known issues & how to fix them

### 1. parse_before() panics during DST transitions

**File:** `src/commands/mod.rs`, line 31
**Severity:** High

**The problem:** When a user passes a HH:MM time like "02:30" during a spring-forward DST transition, `and_local_timezone(chrono::Local)` returns `LocalResult::None` (the time doesn't exist). The code calls `.unwrap()` on this, causing a panic.

**The fix:** Replace `.unwrap()` with a match on `LocalResult`:
```rust
use chrono::LocalResult;
match dt.and_local_timezone(chrono::Local) {
    LocalResult::Single(t) => Ok(t.with_timezone(&Utc)),
    LocalResult::Ambiguous(t, _) => Ok(t.with_timezone(&Utc)),
    LocalResult::None => Err(io::Error::new(io::ErrorKind::InvalidInput,
        format!("Ambiguous or nonexistent local time: '{s}'"))),
}
```

### 2. log() hash slice panics on short hashes

**File:** `src/commands/log.rs`, line 33
**Severity:** Medium

**The problem:** `&h[..8]` panics if `content_hash` is shorter than 8 characters (e.g., corrupted index data).

**The fix:** Replace `&h[..8]` with `h.get(..8).unwrap_or(h)`.

### 3. baseline_snapshot() aborts on single file error

**File:** `src/watcher/mod.rs`, line 60
**Severity:** High

**The problem:** The `?` operator on `path.metadata()` and `std::fs::read(path)` means one permission-denied file kills the entire baseline snapshot for all remaining files.

**The fix:** Wrap per-file operations in a match/if-let and `continue` on error, logging the skipped file.

### 4. store_blob() is not atomic

**File:** `src/store.rs`, line 18
**Severity:** High

**The problem:** `fs::write` is not atomic. A crash mid-write leaves a partial blob file. Since the hash already "exists" on disk, subsequent calls won't overwrite it — the corruption is permanent.

**The fix:** Write to a temp file in the same directory, then rename:
```rust
let tmp = path.with_extension("tmp");
fs::write(&tmp, content)?;
fs::rename(&tmp, &path)?;
```

### 5. build_event() silently drops errors

**File:** `src/watcher/events.rs`, line 73
**Severity:** Critical

**The problem:** Read errors cause `build_event` to return `None` (event lost). `index.append` errors are ignored with `let _ =`. The user has no way to know events are being dropped.

**The fix:** Use `tracing::warn!` for read errors and `tracing::error!` for index append failures. Still return `None` for read errors (the file may be gone), but at least log it.

### 6. Diff.patch is always empty

**File:** `src/watcher/events.rs`, line 107
**Severity:** Medium (dead code)

**The problem:** `Diff { previous_hash, patch: String::new() }` — the patch field is never populated with actual diff content. It gets serialized to JSON as an empty string.

**The fix:** Either implement actual diffing or remove the `patch` field from `Diff` and the `diff` field from `LhiEvent`. Removing is simpler and avoids confusing consumers.

### 7. Duplicated code: hex_sha256 and get_file_mode

**Files:** `src/store.rs:40` + `src/watcher/helpers.rs:30`, `src/commands/mod.rs:57` + `src/watcher/helpers.rs:5`
**Severity:** High (maintenance risk)

**The problem:** Two identical copies of `hex_sha256` and two identical copies of `get_file_mode` exist in different modules. If one is updated and the other isn't, behavior diverges silently.

**The fix:** Create a `src/util.rs` module with `pub fn hex_sha256` and `pub fn get_file_mode`, then import from both `store.rs` and `watcher/helpers.rs`.

### 8. read_all() silently skips malformed lines

**File:** `src/index.rs`, line 42
**Severity:** Medium

**The problem:** `if let Ok(entry) = serde_json::from_str(&line)` silently skips any line that doesn't parse. Index corruption is invisible.

**The fix:** Add `tracing::warn!` for skipped lines, including the line number and parse error.

## Things to watch out for when making changes

- **Serialization compatibility:** `IndexEntry` and `LhiEvent` are serialized to JSONL. Adding fields with `#[serde(default)]` is safe. Removing or renaming fields breaks reading old data. The `event_type` field in `IndexEntry` is a plain `String`, not the `EventType` enum — they're decoupled.

- **Duplicated code that must stay in sync:** `hex_sha256` exists in both `store.rs` and `watcher/helpers.rs`. `get_file_mode` exists in both `commands/mod.rs` and `watcher/helpers.rs`. Until these are unified, changes to one must be mirrored in the other.

- **Platform-specific code:** `get_file_mode` uses `#[cfg(unix)]` / `#[cfg(not(unix))]`. The `restore` command's permission restoration is also Unix-only. Test on both platforms if modifying file mode handling.

- **Error type inconsistency:** Most of the codebase uses `io::Result`, but `LhiWatcher::new()` and `baseline_snapshot()` return `Box<dyn Error>`. The `watch` command maps this to `io::Error` at the boundary. If you're adding error handling, be aware of this split.

- **No concurrency protection:** The index has no file locking. Running `lhi watch` and `lhi snapshot` simultaneously could produce interleaved JSONL lines. This is a known limitation for a single-user CLI tool.

- **The .lhi directory:** Both `baseline_snapshot` and `snapshot` command skip `.lhi` paths using string prefix checks (`rel_str.starts_with(".lhi")`). The watcher relies on `.gitignore` containing `.lhi/` instead. If `.gitignore` doesn't have this entry, the watcher will try to record changes to its own data files.

- **Complexity hotspot:** `build_event()` in `watcher/events.rs` is the most complex function. It touches the filesystem, blob store, index, previous_hashes map, and constructs the event. Most bugs will surface here.

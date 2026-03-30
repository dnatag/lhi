# AGENTS.md

Instructions for AI agents working on this codebase.

## Project overview

`lhi` is a local file history CLI tool written in Rust. It watches directories for file changes, stores versions in a content-addressed blob store (zstd-compressed), and records metadata in a JSONL index. All data lives in `.lhi/` at the project root.

## Architecture

```
bin/lhi  →  commands  →  core (index, store, event)
                │                    ▲
                └──►  watcher  ──────┘
```

- **core/** — Data layer. `Index` manages the JSONL index, `BlobStore` handles content-addressed blobs (zstd-compressed, backward-compatible with uncompressed), `event` defines serializable types. Core types return `io::Result`. `BlobStore::resolve_prefix` resolves short hash prefixes by scanning the blobs directory.
- **commands/** — One file per CLI subcommand. All return `anyhow::Result`. Shared time-parsing utilities and revision helpers (`file_revision`, `parse_rev`) in `mod.rs`. `activate.rs` is special: it generates shell scripts (one per supported shell) rather than performing direct actions. `cat.rs`, `diff.rs`, and `search.rs` use `bat` as a library for syntax-highlighted terminal output, with `diff.rs` also piping to `delta` if available.
- **watcher/** — Real-time filesystem monitoring with `notify` crate. Debounces events (100ms), respects `.gitignore`, stores blobs and index entries. Captures git branch at startup.
- **util.rs** — Shared `hex_sha256`, `get_file_mode`, and `current_git_branch` used by core, commands, and watcher.
- **bin/lhi/** — Thin CLI layer using `clap`. `main.rs` initializes tracing, `cli.rs` dispatches to commands.

## Key conventions

- Error handling: `anyhow::Result` for commands/watcher, `io::Result` for core types.
- Logging: `tracing` crate. Use `tracing::warn!` for recoverable issues, `tracing::error!` for failures that lose data. Subscriber is initialized in `main.rs` with `RUST_LOG` env filter (default: `lhi=info`).
- Serialization: `serde` with JSON. Index entries use `IndexEntry` (flat struct with String event_type). Events use `LhiEvent` (nested struct with enum EventType). These are decoupled — changing one doesn't require changing the other.
- Blob writes are atomic (temp file + rename), zstd-compressed. Reads detect magic bytes for backward compat with old uncompressed blobs.
- Index appends are not atomic (append mode).
- Tests are inline (`#[cfg(test)] mod tests`) in each file.
- Revision references: `~N` means Nth most recent version of a file (`~1` = latest). Parsed by `parse_rev()`, resolved by `file_revision()` in `commands/mod.rs`.

## Important constraints

- **Serialization compatibility:** `IndexEntry` and `LhiEvent` are persisted to disk as JSONL. Adding fields with `#[serde(default)]` is safe. Removing or renaming fields breaks reading old data.
- **Blob compatibility:** Old uncompressed blobs are read transparently (magic byte detection). New blobs are always zstd-compressed.
- **No file locking:** The index has no concurrency protection. Running `lhi watch` and `lhi snapshot` simultaneously could interleave writes.
- **Platform-specific code:** `get_file_mode` uses `#[cfg(unix)]`. Restore permission handling is Unix-only.
- **`.lhi` filtering:** The watcher's `is_ignored()` rejects any path containing a `.lhi` component at any nesting depth (not dependent on `.gitignore`). `baseline_snapshot` also skips paths containing `.lhi` at any level. This prevents double-recording in nested project setups.
- **Git branch:** Captured once at watcher startup and snapshot time, not per-event. Stored as `Option<String>` — `None` when not in a git repo.
- **Shell hook portability:** `activate.rs` emits separate scripts for bash and zsh. Bash script avoids associative arrays (bash 3.2 on macOS lacks them) and uses a newline-delimited string instead. Zsh script uses native `typeset -A` with zsh-specific key iteration (`${(k)arr[@]}`) and existence checks (`${+arr[key]}`). These syntaxes are not interchangeable — do not attempt a single "portable" script for both shells.
- **Shell hook error handling:** Watcher stderr is logged to `~/.lhi-watch.log`. After launching a watcher, the hook sleeps briefly and checks `kill -0` to detect immediate crashes, warning the user if the watcher failed to start.
- **Terminal output:** `cat`, `diff`, and `search` use `bat` as a library (`PrettyPrinter`) for syntax-highlighted output when stdout is a terminal. When piped, they emit plain/raw output for composability. `diff` additionally tries piping to `delta` CLI if installed before falling back to bat. The `bat` dependency uses `default-features = false` with `regex-fancy` (pure Rust, no C deps). Filenames for syntax auto-detection are resolved from the index via hash lookup.

## Running tests

```bash
cargo test           # all tests
cargo test core::    # core module only
cargo test watcher:: # watcher tests (includes filesystem integration tests)
```

Watcher tests create real temp directories and filesystem events — they may be flaky under heavy system load due to timing-sensitive debounce assertions.

## Adding a new command

1. Create `src/commands/<name>.rs` with `pub fn <name>(...) -> anyhow::Result<()>`
2. Add `mod <name>;` and `pub use <name>::<name>;` in `src/commands/mod.rs`
3. Add a variant to `Command` enum in `src/bin/lhi/cli.rs`
4. Add a match arm in `cli::run()`

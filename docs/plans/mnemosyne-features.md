# Feature Plan: Mnemosyne-Inspired Enhancements

**Date:** 2026-03-16
**Status:** Planned

## Features (in implementation order)

### 1. Blob Compression (zstd)

- `store_blob()`: zstd encode (level 3) before write
- `read_blob()`: check zstd magic bytes (`0x28B52FFD`), decompress if present, raw if not
- Backward compatible with existing uncompressed blobs

**New dep:** `zstd = "0.13"`
**Files:** `Cargo.toml`, `src/core/store.rs`

### 2. Git Branch Tracking

- Add `git_branch: Option<String>` to `IndexEntry` with `#[serde(default)]`
- `fn current_git_branch(root: &Path) -> Option<String>` in `util.rs` — shell to `git rev-parse --abbrev-ref HEAD`
- Capture branch at watcher startup (cached on struct), snapshot time, baseline time — not per-event
- Add `--branch <name>` filter to `lhi log`

**Files:** `src/core/index.rs`, `src/util.rs`, `src/commands/log.rs`, `src/watcher/watcher.rs`, `src/watcher/events.rs`, `src/commands/snapshot.rs`

### 3. `lhi diff <hash1> <hash2>`

- Read both blobs, produce unified diff with `similar` crate
- Color output when stdout is a terminal

**New dep:** `similar = "2"`
**Files:** `src/commands/diff.rs`, `src/commands/mod.rs`

### 4. `lhi search <query>`

- Walk index entries (latest per file, or all with `--all`)
- Read blobs, grep for query string
- Show: relative path, timestamp, matching lines with context
- Optional `--file <path>` filter

**Files:** `src/commands/search.rs`, `src/commands/mod.rs`

### 5. `lhi info`

- Count blobs, total blob size, index entries, unique files tracked, `.lhi/` disk usage

**Files:** `src/commands/info.rs`, `src/commands/mod.rs`

### 6. Wire CLI + Update Docs

- Add all new commands to clap enum and `cli::run()`
- Update README.md and AGENTS.md

## Dropped

- **`.lhiignore`** — `.gitignore` is sufficient; extra ignore file adds complexity for little value

## New Dependencies

```toml
zstd = "0.13"
similar = "2"
```

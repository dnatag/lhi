# Feature Plan: Mnemosyne-Inspired Enhancements

**Date:** 2026-03-16
**Status:** Planned
**Motivation:** Gap analysis against [Mnemosyne](https://github.com/alessandrobrunoh/Mnemosyne) identified 6 practical features worth borrowing — no over-engineering, no protocol layers, just things that make lhi more useful day-to-day.

## Features (in implementation order)

### 1. Blob Compression (zstd)

**Problem:** lhi stores raw file content. A 10MB project with 100 saves = 1GB of blobs. Source code compresses ~3:1 with zstd.

**Approach:**
- Add `zstd` crate dependency
- `store_blob()`: compress with zstd before writing (level 3 — fast)
- `read_blob()`: detect zstd magic bytes (`0x28 0xB5 0x2F 0xFD`) on read; decompress if present, return raw if not
- Backward compatible — existing uncompressed blobs continue to work
- Skip compression for already-compressed content (check magic bytes of common formats)

**Files:** `Cargo.toml`, `src/core/store.rs`
**Tests:** roundtrip compressed, read old uncompressed, dedup still works

---

### 2. Git Branch Tracking

**Problem:** `lhi log` mixes history from all branches. No way to filter by branch.

**Approach:**
- Add `git_branch: Option<String>` to `IndexEntry` with `#[serde(default)]` (backward compatible with old index files)
- Add `fn current_git_branch(root: &Path) -> Option<String>` to `util.rs` — shells to `git rev-parse --abbrev-ref HEAD`
- Populate `git_branch` in watcher events, baseline, and snapshot
- Add `--branch <name>` filter to `lhi log`

**Files:** `src/core/index.rs`, `src/util.rs`, `src/commands/log.rs`, `src/watcher/watcher.rs`, `src/watcher/events.rs`, `src/commands/snapshot.rs`
**Tests:** IndexEntry serde with/without git_branch, log filtering

---

### 3. .lhiignore Support

**Problem:** lhi only respects `.gitignore`. No way to ignore additional patterns (e.g. large generated files, build artifacts not in `.gitignore`).

**Approach:**
- Load `.lhiignore` from project root (same glob syntax as `.gitignore`)
- Merge with `.gitignore` patterns using `ignore` crate's `GitignoreBuilder`
- Apply in: watcher `is_ignored()`, `baseline_snapshot()`, `snapshot` command
- If `.lhiignore` doesn't exist, behavior unchanged

**Files:** `src/watcher/watcher.rs`, `src/watcher/events.rs`, `src/commands/snapshot.rs`
**Tests:** watcher ignores patterns from .lhiignore, snapshot skips .lhiignore patterns

---

### 4. `lhi diff <hash1> <hash2>`

**Problem:** No way to see what changed between two versions of a file.

**Approach:**
- New command: `lhi diff <hash1> <hash2>`
- Add `similar` crate for unified diff output
- Read both blobs, produce unified diff with context lines
- Color output: green for additions, red for deletions (when stdout is a terminal)

**Files:** `Cargo.toml`, `src/commands/diff.rs`, `src/commands/mod.rs`
**Tests:** diff of identical blobs (empty output), diff of different blobs (correct hunks)

---

### 5. `lhi search <query>`

**Problem:** Can't search content of old file versions.

**Approach:**
- New command: `lhi search <query> [--file <path>]`
- Walk index entries, read corresponding blobs, grep for query
- Show: relative path, timestamp, matching lines with context
- Optional `--file` filter to scope to one file
- No trigram index needed — at lhi's scale, linear scan through blobs is fine

**Files:** `src/commands/search.rs`, `src/commands/mod.rs`
**Tests:** search finds match in blob, --file filter works, no match returns empty

---

### 6. `lhi info`

**Problem:** No visibility into storage usage or tracking stats.

**Approach:**
- New command: `lhi info`
- Report: total blobs, total blob size, index entries, unique files tracked, `.lhi/` total disk usage, compression ratio (if compressed blobs exist)

**Files:** `src/commands/info.rs`, `src/commands/mod.rs`
**Tests:** info on empty project, info after some writes

---

## Wiring & Docs (final step)

- Add all new commands to `Command` enum in `src/bin/lhi/cli.rs`
- Add match arms in `cli::run()`
- Update `README.md` with new commands
- Update `AGENTS.md` with new conventions

## Dependencies to Add

```toml
zstd = "0.13"
similar = "2"
```

## Design Principles

- **Backward compatible** — old `.lhi/` directories work without migration
- **No gold plating** — linear scan over trigram index, shell to git over git2 crate, simple grep over full-text search engine
- **Minimal new deps** — only `zstd` and `similar`, both small and well-maintained

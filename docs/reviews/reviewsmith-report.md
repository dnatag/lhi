# Code Review Report

**Generated:** 2026-03-15T22:32:00-04:00
**Reviewed:** src/
**Strategy:** full (every function reviewed)

## Executive Summary

- Total: 39 functions, 16 files, 4 modules
- Assessment: Needs work — solid architecture with several correctness bugs
- Critical issues: 1 (build_event has multiple compounding problems)
- High issues: 4 (after filtering theoretical/DRY-only findings)
- Confidence: High

## Findings by Severity

### Critical

1. **src/watcher/events.rs:73 `build_event()`**
   - **Issue:** Multiple compounding problems: (1) `Diff.patch` is always empty string — diff feature is non-functional placeholder. (2) TOCTOU race between symlink/file/metadata checks and content read. (3) Silently returns `None` on read errors, losing events. (4) `index.append` errors silently ignored with `let _ =`. (5) Two redundant `metadata()` syscalls.
   - **Impact:** Silent data loss on transient read errors. Silent index corruption on append failures. Dead code in serialized output.
   - **Recommendation:** Log read errors with tracing, propagate append errors, remove or implement Diff.patch, consolidate metadata calls.

### High

1. **src/commands/mod.rs:25 `parse_before()`**
   - **Issue:** `unwrap()` on `and_local_timezone()` panics during DST transitions (spring-forward gap produces nonexistent local time).
   - **Impact:** CLI crashes when user runs `lhi restore --before 02:30` during spring-forward.
   - **Recommendation:** Match on `LocalResult` variants, return error for `None`/`Ambiguous`.

2. **src/watcher/mod.rs:60 `baseline_snapshot()`**
   - **Issue:** Single file read failure (e.g. permission denied) aborts entire baseline for all remaining files via `?` operator.
   - **Impact:** One unreadable file prevents baseline snapshot of entire project.
   - **Recommendation:** Use `continue` on per-file errors, log with tracing.

3. **src/store.rs:18 `store_blob()`**
   - **Issue:** Non-atomic `fs::write`. Crash mid-write leaves partial blob that will never be overwritten (hash already "exists").
   - **Impact:** Corrupted blob permanently stored. `compact()` already uses atomic rename — inconsistent safety.
   - **Recommendation:** Write to temp file, then rename (same pattern as `compact()`).

4. **src/commands/restore.rs:13 `restore()`**
   - **Issue:** When `--file` filter is used, the delete scan for post-cutoff files is skipped entirely.
   - **Impact:** Files created after cutoff matching the filter won't be cleaned up.
   - **Recommendation:** Apply file filter to the delete scan as well.

### Medium

1. **src/index.rs:42 `read_all()`** — Silent skip of malformed JSONL lines hides corruption. Add tracing for skipped lines.
2. **src/commands/log.rs:10 `log()`** — `h[..8]` slice panics if content_hash is shorter than 8 chars. Use `h.get(..8).unwrap_or(h)`.
3. **src/commands/snapshot.rs:12 `snapshot()`** — Per-file `Utc::now()` timestamps instead of single snapshot timestamp.
4. **src/watcher/events.rs:14 `next_event()`** — Undocumented 60-second blocking timeout when idle.
5. **src/watcher/events.rs:148 `is_ignored()`** — Gitignore loaded once at startup, never refreshed.
6. **Code duplication** — `hex_sha256` (store.rs vs helpers.rs) and `get_file_mode` (commands/mod.rs vs helpers.rs).

### Low

1. `parse_duration_ago` accepts negative numbers producing future timestamps.
2. `flush_ready` non-deterministic event ordering from HashMap iteration.
3. `cat` missing hash validation and stdout flush.

## Function-Level Analysis

- Functions reviewed: 39
- Common patterns: Content-addressed storage, JSONL append-only index, debounced filesystem events
- Common issues: Silent error swallowing, code duplication across modules, inconsistent error types

### bin_lhi (2 functions)
- `main()` — ✅ Clean entry point
- `run()` — ✅ Well-structured CLI dispatch (medium: io::Result loses context for non-IO errors)

### core (15 functions)
- `default_true()` — ✅ Simple serde default
- `Index::open()` — ✅ Clean directory creation
- `Index::append()` — ⚠️ No file locking (acceptable for single-process)
- `Index::read_all()` — ⚠️ Silent skip of malformed lines
- `Index::query_file()` — ✅ O(n) acceptable at scale
- `Index::query_since()` — ✅ Same pattern
- `Index::state_at()` — ✅ HashMap-based, order-independent
- `Index::all_known_paths()` — ✅ Clean
- `Index::compact()` — ⚠️ No fsync before rename
- `BlobStore::init()` — ✅ Clean
- `BlobStore::store_blob()` — ⚠️ Non-atomic write
- `BlobStore::read_blob()` — ✅ Clean
- `BlobStore::has_blob()` — ✅ No integrity check (acceptable)
- `BlobStore::blob_path()` — ✅ Safe (hashes internally generated)
- `hex_sha256()` — ✅ Correct (duplicated in watcher)

### commands (12 functions)
- `parse_since()` — ✅ Clean delegation
- `parse_before()` — ❌ DST panic via unwrap()
- `parse_duration_ago()` — ⚠️ Accepts negative numbers
- `get_file_mode()` (unix/non-unix) — ✅ Correct (duplicated in watcher)
- `cat()` — ⚠️ No hash validation
- `log()` — ⚠️ Hash slice panic on short strings
- `compact()` — ✅ Clean wrapper
- `snapshot()` — ⚠️ Per-file timestamps
- `restore()` — ⚠️ Delete scan skipped with --file filter
- `to_restore_action()` — ✅ Solid logic
- `watch()` — ⚠️ Silent when verbose=false

### watcher (10 functions)
- `LhiWatcher::new()` — ⚠️ Box<dyn Error> inconsistency, previous_hashes not seeded
- `baseline_snapshot()` — ⚠️ Aborts on single file error
- `next_event()` — ⚠️ Undocumented 60s timeout
- `flush_ready()` — ⚠️ Non-deterministic ordering
- `build_event()` — ❌ Multiple compounding issues
- `is_ignored()` — ⚠️ Static gitignore
- `get_file_mode()` (unix/non-unix) — ⚠️ Duplicate of commands version
- `hex_sha256()` — ⚠️ Duplicate of store version
- `is_ignored_by()` — ⚠️ Test helper diverges from production behavior

## File-Level Analysis

| File | Cohesion | Coupling | Notes |
|---|---|---|---|
| event.rs | High | Low | Pure data model, well-isolated |
| index.rs | High | Low | Single responsibility: JSONL index |
| store.rs | High | Low | Single responsibility: blob storage |
| commands/mod.rs | Medium | Low | Mixes time parsing + file mode + re-exports |
| commands/restore.rs | High | Medium | Most complex command, well-structured |
| watcher/mod.rs | Medium | High | Struct + baseline + tests in one file |
| watcher/events.rs | High | High | Core event loop, highest complexity |
| watcher/helpers.rs | Low | Low | Grab-bag of duplicated utilities |

## Module-Level Analysis

- Modules: 4 (bin_lhi, commands, core, watcher)
- Dependency graph:
  ```
  bin/lhi ──► commands ──► core (index, store, event)
                  │                    ▲
                  └──► watcher ────────┘
  ```
- Circular dependencies: None
- Boundary clarity: Good — clean module interfaces

## Architectural Analysis

- Architecture type: Clean modular CLI with content-addressed storage
- Strengths:
  - Clean layered architecture with good separation of concerns
  - Content-addressed storage with SHA-256 dedup
  - JSONL format is simple, human-readable, appendable
  - Atomic rename in compact() shows crash-safety awareness
  - Comprehensive test coverage
  - Platform-conditional compilation for Unix file modes
- Weaknesses:
  - Inconsistent error handling (io::Error vs Box<dyn Error>, silent drops)
  - Inconsistent crash safety (compact uses atomic rename, store_blob doesn't)
  - Code duplication across module boundaries
  - Diff.patch is dead feature code
  - No concurrency story (fine for single-process CLI)
- Missing components: Structured logging/tracing, unified error type

## Recommendations (Prioritized)

### Immediate (Critical)

1. Fix `parse_before()` DST panic — replace `unwrap()` with `LocalResult` handling
2. Fix `log()` hash slice panic — use `h.get(..8).unwrap_or(h)`
3. Make `baseline_snapshot()` continue on per-file errors

### Short-term (High)

4. Decide on `Diff.patch` — remove or implement
5. Extract shared `hex_sha256` and `get_file_mode` into common utility module
6. Add tracing for skipped malformed index lines in `read_all()`
7. Make `store_blob()` atomic (write to temp, rename)
8. Handle `index.append` errors in `build_event()` instead of silently ignoring

### Long-term (Medium/Low)

9. Migrate to `anyhow::Result` or custom error type
10. Use single timestamp for `snapshot()` command
11. Add file locking if concurrent access becomes a concern
12. Consider refreshing gitignore periodically in watcher

## Human Comprehension Assessment

- **Can you explain how the system works?** Yes
- **Would you trust this in production?** With immediate fixes — yes for personal/team use
- **Could you modify it safely?** Yes — clean module boundaries, good test coverage
- **Confidence in understanding:** High

**Explain to a junior developer:**
lhi is a local file history tool (like IntelliJ's local history). It watches a directory for file changes, stores each version's content in a content-addressed blob store (`.lhi/blobs/`), and records metadata in a JSONL index (`.lhi/index.jsonl`). You can browse history with `lhi log`, view old versions with `lhi cat`, restore files to a point in time with `lhi restore`, or take manual snapshots with `lhi snapshot`. The codebase is cleanly split: `event.rs` defines the data model, `index.rs` and `store.rs` handle persistence, `commands/` implements each CLI command, and `watcher/` handles real-time filesystem monitoring with debouncing.

## Next Steps

1. Add `anyhow` and `tracing` crates
2. Fix the 3 immediate issues (DST panic, hash slice, baseline abort)
3. Address short-term items (Diff.patch, code dedup, atomic store, error handling)
4. Migrate error types to anyhow throughout

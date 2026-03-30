# How It Works

## Storage layout

```
.lhi/
├── index.jsonl    Append-only event log (one JSON line per change)
└── blobs/         Content-addressed file storage (SHA-256, zstd-compressed)
    ├── a1b2c3...
    └── d4e5f6...
```

## Blob store

Files are stored by their SHA-256 hash. Identical content is automatically deduplicated. Blobs are zstd-compressed on write; old uncompressed blobs are read transparently (magic byte detection). Writes are atomic (temp file + rename). Short hash prefixes are resolved by scanning the blobs directory.

## Index

JSONL format — each line records timestamp, event type, file path, content hash, size, and git branch. Append-only during normal operation; `compact` rewrites it.

## Watcher

Uses OS-native filesystem notifications (`notify` crate) with 100ms debouncing. On first run, captures a baseline snapshot of all existing files. Respects `.gitignore`. Ignores `.lhi/` directories at any nesting depth. Files over 10MB are skipped. Symlinks are ignored.

## Git integration

Automatically records the current git branch with each event (captured at watcher startup and snapshot time via `git rev-parse --abbrev-ref HEAD`). Stored as `Option<String>` — `None` when not in a git repo.

## Terminal output

`cat`, `diff`, and `search` use [bat](https://github.com/sharkdp/bat) as a library (`PrettyPrinter`) for syntax-highlighted output when stdout is a terminal. When piped, they emit plain/raw output for composability.

`diff` additionally tries piping to [delta](https://github.com/dandavison/delta) if installed before falling back to bat. The `bat` dependency uses `default-features = false` with `regex-fancy` (pure Rust, no C dependencies). Filenames for syntax auto-detection are resolved from the index via hash lookup.

## Architecture

```
bin/lhi  →  commands  →  core (index, store, event)
                │                    ▲
                └──►  watcher  ──────┘
```

- **core/** — Data layer. `Index` manages the JSONL index, `BlobStore` handles content-addressed blobs, `event` defines serializable types. Core types return `io::Result`.
- **commands/** — One file per CLI subcommand. All return `anyhow::Result`. `cat.rs`, `diff.rs`, and `search.rs` use `bat` for syntax-highlighted output, with `diff.rs` also piping to `delta` if available.
- **watcher/** — Real-time filesystem monitoring. Debounces events, respects `.gitignore`, stores blobs and index entries.
- **util.rs** — Shared `hex_sha256`, `get_file_mode`, and `current_git_branch`.
- **bin/lhi/** — Thin CLI layer using `clap`.

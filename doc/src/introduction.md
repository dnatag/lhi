# lhi

Local history for your code — like IntelliJ's Local History, but for any editor.

`lhi` watches a directory for file changes and maintains a local version history. Every save is captured automatically with content-addressed storage and a JSONL index. No server, no network, no config — just a `.lhi/` directory at your project root.

## Features

- **Editor-agnostic** — works with vim, VS Code, Helix, IntelliJ, or any editor
- **Automatic capture** — every file save is recorded via OS-native filesystem notifications
- **Content-addressed storage** — SHA-256 hashing with automatic deduplication, zstd compression
- **Git branch tracking** — each event is tagged with the current git branch
- **Syntax-highlighted output** — `cat`, `diff`, and `search` use [bat](https://github.com/sharkdp/bat) for rich terminal output; `diff` also supports [delta](https://github.com/dandavison/delta) if installed
- **Full-text search** — search through all historical file versions
- **Point-in-time restore** — restore individual files or entire projects to any previous state
- **Shell integration** — automatic watcher activation via `cd` hook (bash, zsh)
- **Scriptable** — JSON output and plain text when piped

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
```

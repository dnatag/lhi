# Commands

## `lhi activate`

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

## `lhi watch [PATH]`

Watch a directory for file changes. Records every create, modify, and delete to `.lhi/`.

```
Options:
  -v, --verbose  Print events as JSON to stdout
```

Runs in the foreground (blocking). Useful for troubleshooting or one-off use. The `lhi activate` shell hook uses this command internally.

On first run, captures a baseline snapshot of all existing files. Respects `.gitignore`. Debounces rapid writes (100ms window). Files over 10MB are skipped.

## `lhi log [FILE]`

Show change history.

```
Options:
  --since <DURATION>  Filter by time (e.g. 5m, 1h, 2d)
  --branch <NAME>     Filter by git branch
  --json              Output as JSON
```

When git branch tracking is available, each entry shows the branch it was recorded on.

## `lhi cat <HASH>`

Print the content of a stored file version by its SHA-256 hash (from `lhi log` output).

When stdout is a terminal, output is syntax-highlighted with line numbers and a grid border (powered by [bat](https://github.com/sharkdp/bat)). The language is auto-detected from the filename in the index. When piped, raw content is emitted for composability.

## `lhi diff <HASH1> <HASH2>`

Show a unified diff between two stored file versions.

When stdout is a terminal, the diff is rendered with syntax highlighting. If [delta](https://github.com/dandavison/delta) is installed, it is used automatically for rich side-by-side output. Otherwise, falls back to bat's Diff syntax highlighting. When piped, standard unified diff format is emitted.

## `lhi search <QUERY>`

Search through stored file contents for a text pattern (case-insensitive).

```
Options:
  --file <PATH>  Search only versions of this file
```

Searches each unique blob once. When stdout is a terminal, matching lines are shown with syntax-highlighted context (2 lines above and below), line numbers, and highlighted match lines. When piped, plain text output is emitted.

## `lhi info`

Show storage statistics: index entries, files tracked, blob count, blob size, and total `.lhi/` disk usage.

## `lhi restore [FILE] --before <TIME>`

Restore files to their state before a point in time.

```
Options:
  --before <TIME>  Required. Accepts: 5m, 1h, 14:30, ISO 8601
  --dry-run        Preview without making changes
  --json           Output as JSON
```

Compares stored hashes against current disk state — only overwrites files that actually changed. Restores Unix file permissions. Deletes files that didn't exist at the target time.

## `lhi snapshot [--label <LABEL>]`

Capture a full project snapshot. Useful before risky changes.

## `lhi compact`

Compact the index to keep only the latest entry per file. Reduces `.lhi/index.jsonl` size.

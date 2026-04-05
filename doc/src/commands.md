# Commands

## `lhi init [PATH]`

Initialize a `.lhi/` directory for a project. Creates `.lhi/blobs/` and adds `.lhi/` to `.gitignore` if one exists. Safe to run multiple times (idempotent).

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
- Logs watcher errors to `~/.lhi-watch.log` and warns on failed launches
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

## `lhi cat <TARGET> [~N]`

Print the content of a stored file version. Accepts a hash (or short prefix), a file path (shows latest), or a file path with `~N` revision.

```bash
lhi cat a1b2c3d4          # by hash prefix
lhi cat src/main.rs        # latest version
lhi cat src/main.rs ~3     # 3rd most recent
```

When stdout is a terminal, output is syntax-highlighted with line numbers and a grid border (powered by [bat](https://github.com/sharkdp/bat)). The language is auto-detected from the filename in the index. When piped, raw content is emitted for composability.

## `lhi diff <ARG1> [ARG2] [ARG3]`

Show a unified diff between file versions. Supports multiple forms:

```bash
lhi diff a1b2c3d4 e5f6a7b8   # two hash prefixes
lhi diff src/main.rs ~3 ~1    # file with two revisions
lhi diff src/main.rs ~5       # revision vs current file on disk
lhi diff src/main.rs          # latest stored vs current disk
```

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

## `lhi restore [FILE] [~N]`

Restore files to a previous state. Supports multiple modes:

```bash
lhi restore src/main.rs ~5           # single file to revision
lhi restore src/main.rs --at a1b2    # single file to specific hash
lhi restore --at a1b2c3d4            # all files to that moment
lhi restore --before 5m              # all files to 5 minutes ago
lhi restore --snapshot "before refactor"  # restore to a named snapshot
```

```
Options:
  --at <HASH>       Restore to the moment a specific hash was recorded
  --before <TIME>   Restore to before a time (5m, 1h, 14:30, ISO 8601)
  --snapshot <LABEL> Restore to a named snapshot (from lhi snapshot --label)
  --dry-run          Preview without making changes
  --json             Output as JSON
```

Compares stored hashes against current disk state — only overwrites files that actually changed. Restores Unix file permissions. Deletes files that didn't exist at the target time.

## `lhi snapshot [--label <LABEL>]`

Capture a full project snapshot. Useful before risky changes.

## `lhi compact`

Compact the index to keep only the latest entry per file. Reduces `.lhi/index.jsonl` size.

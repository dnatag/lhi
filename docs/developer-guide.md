# lhi activate — Developer Guide

<!-- reviewsmith:guide -->
<!-- reviewsmith:generated:2026-03-18T21:40Z -->
<!-- reviewsmith:scope:partial -->
<!-- reviewsmith:modules:commands::activate -->

> This guide is written for a human developer working on this project without
> AI assistance. It explains what the `activate` module does, how the shell
> hook works, what the known issues are, and how to fix them.

## Table of Contents

- [What does lhi activate do?](#what-does-lhi-activate-do)
- [Project layout (relevant files)](#project-layout-relevant-files)
- [Module walkthrough: commands::activate](#module-walkthrough-commandsactivate)
- [Data flow](#data-flow)
- [Known issues & how to fix them](#known-issues--how-to-fix-them)
- [Things to watch out for when making changes](#things-to-watch-out-for-when-making-changes)

## What does lhi activate do?

`lhi activate` prints a shell script to stdout. The user puts `eval "$(lhi activate)"` in their `.bashrc` or `.zshrc`, and the script installs hooks on `cd`, `pushd`, and `popd`. Every time the user changes directories, the hook walks up the directory tree looking for a `.lhi/` folder. If it finds one, it starts `lhi watch` in the background for that project.

Multiple projects can be watched concurrently — each gets its own background process. The hook tracks which projects are being watched and re-launches watchers if they die. When the shell exits, all watchers are killed via an EXIT trap. The user can also run `_lhi_deactivate` to manually stop everything and restore the original shell behavior.

The Rust side is minimal: validate the shell from `$SHELL`, then print the appropriate script. All the real logic lives in the emitted shell script.

## Project layout (relevant files)

```
src/commands/
├── activate.rs     — This module. Shell hook generation.
├── mod.rs          — Exports activate(). Wires it into the command system.
└── watch.rs        — The lhi watch command that the hook launches in background.

src/bin/lhi/
├── cli.rs          — Clap CLI. Has Activate variant that calls activate().
└── main.rs         — Entry point, tracing init.
```

## Module walkthrough: commands::activate

<!-- reviewsmith:module:commands::activate -->

This module has three functions and no types. It generates shell code — the Rust is just a delivery mechanism.

**Functions:**

- `activate()` (line 8) — Entry point called by the CLI. Reads `$SHELL`, validates it via `detect_shell()`, then prints the hook script to stdout. The return value from `detect_shell` is currently unused (`_shell`) because only one hook function exists. When fish support is added, this will need to dispatch to different hook generators per shell.

- `detect_shell(shell_path: &str)` (line 14) — Pure function that extracts the shell name from a path like `/usr/local/bin/zsh`. Uses `rsplit('/')` to get the basename, matches against `"bash"` and `"zsh"`. Returns `&'static str`. Errors with a descriptive message on unsupported shells. Takes a `&str` parameter (not reading env directly) so it can be unit-tested without mutating environment variables — important because `env::set_var` is unsafe in Rust edition 2024.

- `bash_zsh_hook()` (line 26) — Returns a `&'static str` containing ~80 lines of shell script. This is the heart of the module. The script defines:
  - `_LHI_PIDS` — associative array mapping project root paths to watcher PIDs
  - `_lhi_find_root()` — walks up from a directory looking for `.lhi/`
  - `_lhi_hook()` — called after every cd; finds root, checks if already watching, launches `lhi watch` if needed
  - `_lhi_deactivate()` — kills all watchers, restores original cd/pushd/popd, removes all hook functions
  - cd/pushd/popd overrides — call the original or builtin, then `_lhi_hook`
  - EXIT trap — calls `_lhi_deactivate` on shell exit
  - Immediate `_lhi_hook` call — activates for the current directory at eval time

  The script preserves existing `cd` overrides (e.g., from other tools) by copying the current `cd` function body into `_lhi_orig_cd` before installing its own override. On deactivate, it copies the body back.

**If you're modifying this module:**
- The shell script is a raw string literal — no syntax highlighting, no linting in your editor. Run `bash -n` and `zsh -n` on the output after changes, but be aware these only check syntax, not runtime behavior.
- `detect_shell` takes `&str` instead of reading env directly because `env::set_var` is unsafe in edition 2024. Don't change this to read env in tests.
- The `declare -f cd | tail -n +2` idiom extracts a function body. It works in both bash and zsh but the output format differs slightly — test in both.

<!-- reviewsmith:end:commands::activate -->

## Data flow

```
User's .bashrc / .zshrc
    │
    ▼
eval "$(lhi activate)"
    │
    ▼
activate()  ──►  detect_shell($SHELL)  ──►  "bash" | "zsh" | error
    │
    ▼
bash_zsh_hook()  ──►  prints shell script to stdout
    │
    ▼
Shell evals the script, installing:
    ├── cd() / pushd() / popd()  ──►  _lhi_hook()
    │                                      │
    │                                      ▼
    │                                _lhi_find_root($PWD)
    │                                      │
    │                                      ▼
    │                                Check _LHI_PIDS[$root]
    │                                      │
    │                              ┌───────┴────────┐
    │                              │                 │
    │                         Already watching   Not watching
    │                         (kill -0 check)         │
    │                              │                  ▼
    │                           return 0      lhi watch $root &
    │                                         _LHI_PIDS[$root]=$!
    │
    ├── trap EXIT  ──►  _lhi_deactivate()
    │                        │
    │                        ▼
    │                   Kill all PIDs in _LHI_PIDS
    │                   Restore original cd
    │                   Unset all _lhi_* functions
    │
    └── _lhi_hook()  (immediate, for current directory)
```

## Known issues & how to fix them

### 1. Associative array syntax not portable between bash and zsh

**File:** `src/commands/activate.rs`, `bash_zsh_hook()` (line 26)
**Severity:** Critical

**The problem:** The script uses a single associative array syntax that doesn't work on either target platform:
- macOS ships bash 3.2, which has no associative arrays at all. `_LHI_PIDS["/path"]=pid` fails with an arithmetic evaluation error because bash 3.2 treats `[...]` as arithmetic context.
- zsh has associative arrays but uses different syntax: `${(k)_LHI_PIDS[@]}` for key iteration (not `${!_LHI_PIDS[@]}`), and `${arr[key]+x}` existence checks don't work for associative array elements.

**The fix:** Emit shell-specific scripts. The `_shell` return value from `detect_shell()` is already captured — use it to dispatch between `bash_hook()` and `zsh_hook()`. Each uses the correct syntax for its shell. The shared logic (function structure, `_lhi_find_root`, cd overrides) can stay the same; only the associative array operations differ.

### 2. Unused `_shell` variable in activate()

**File:** `src/commands/activate.rs`, `activate()` (line 8)
**Severity:** Low (design decision, not a defect)

**The problem:** `detect_shell()` returns the shell name but `activate()` discards it with `_shell`. This is intentional — fish support is deferred and only one hook generator exists. When fish support is added, this becomes the dispatch point.

**The fix:** No action needed now. When adding fish support, change to `match shell { "bash" | "zsh" => ..., "fish" => ..., }`.

## Things to watch out for when making changes

- **bash 3.2 on macOS:** Apple ships bash 3.2 (2007) due to GPLv3 licensing. Associative arrays, `declare -g`, `&>>`, `|&`, and many other bash 4+ features are unavailable. If you target macOS bash users, stick to bash 3.2 syntax or require them to install bash 4+ via Homebrew.
- **zsh array syntax:** zsh associative arrays use `typeset -A`, `${(k)arr}` for keys, `${(v)arr}` for values, and `(( ${+arr[key]} ))` for existence checks. None of these work in bash.
- **`declare -f` output format:** Both bash and zsh support `declare -f funcname` to print a function definition, but the exact output format differs. The `tail -n +2` idiom to strip the first line (function name) works in both, but test after changes.
- **Shell script is a raw string:** No editor support for the embedded shell. After any change, run: `cargo run -- activate 2>/dev/null | bash -n && echo ok` and the same with `zsh -n`. But remember these only catch syntax errors, not runtime failures.
- **Edition 2024 env safety:** `std::env::set_var` is unsafe. Tests use `detect_shell(&str)` to avoid env mutation. Don't add tests that call `env::set_var` without an unsafe block.

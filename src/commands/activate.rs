use anyhow::{Result, bail};
use std::env;

/// Prints a shell hook script to stdout that auto-starts `lhi watch`
/// when the user `cd`s into a directory tree containing `.lhi/`.
///
/// Usage: eval "$(lhi activate)"
pub fn activate() -> Result<()> {
    let shell = detect_shell(&env::var("SHELL").unwrap_or_default())?;
    match shell {
        "bash" => print!("{}", bash_hook()),
        "zsh" => print!("{}", zsh_hook()),
        _ => unreachable!(),
    }
    Ok(())
}

fn detect_shell(shell_path: &str) -> Result<&'static str> {
    match shell_path.rsplit('/').next().unwrap_or("") {
        "bash" => Ok("bash"),
        "zsh" => Ok("zsh"),
        _ => bail!(
            "Cannot detect a supported shell from $SHELL={shell_path:?}. \
             Only bash and zsh are supported."
        ),
    }
}

#[cfg(test)]
fn hook_for(shell: &str) -> &'static str {
    match shell {
        "bash" => bash_hook(),
        "zsh" => zsh_hook(),
        _ => unreachable!(),
    }
}

/// Bash hook — uses newline-delimited _LHI_WATCHERS="root\tpid\n..." for
/// bash 3.2 compatibility (no associative arrays before bash 4).
fn bash_hook() -> &'static str {
    r#"# lhi shell hook — auto-start watcher on cd into .lhi projects
# eval "$(lhi activate)" in your .bashrc / .zshrc

_LHI_WATCHERS=""
touch "$HOME/.lhi-watch.log" 2>/dev/null

_lhi_find_root() {
    local dir="$1"
    while [ "$dir" != "/" ]; do
        if [ -d "$dir/.lhi" ]; then
            printf '%s' "$dir"
            return 0
        fi
        dir="$(dirname "$dir")"
    done
    return 1
}

_lhi_get_pid() {
    local IFS_SAVE="$IFS" r p
    IFS=$'\n'
    for line in $_LHI_WATCHERS; do
        IFS=$'\t' read -r r p <<< "$line"
        if [ "$r" = "$1" ]; then
            IFS="$IFS_SAVE"
            printf '%s' "$p"
            return 0
        fi
    done
    IFS="$IFS_SAVE"
    return 1
}

_lhi_remove() {
    local new="" IFS_SAVE="$IFS" r p
    IFS=$'\n'
    for line in $_LHI_WATCHERS; do
        IFS=$'\t' read -r r p <<< "$line"
        [ "$r" != "$1" ] && new="${new:+${new}
}${r}	$p"
    done
    IFS="$IFS_SAVE"
    _LHI_WATCHERS="$new"
}

_lhi_hook() {
    local root pid
    root="$(_lhi_find_root "$PWD")" || return 0
    pid="$(_lhi_get_pid "$root")" && {
        kill -0 "$pid" 2>/dev/null && return 0
        _lhi_remove "$root"
    }
    lhi watch "$root" >/dev/null 2>>"$HOME/.lhi-watch.log" &
    local wpid=$!
    disown "$wpid" 2>/dev/null
    sleep 0.1
    if kill -0 "$wpid" 2>/dev/null; then
        _LHI_WATCHERS="${_LHI_WATCHERS:+${_LHI_WATCHERS}
}${root}	$wpid"
    fi
}

_lhi_deactivate() {
    printf '%s\n' "$_LHI_WATCHERS" | while IFS='	' read -r r p; do
        [ -n "$p" ] && kill "$p" 2>/dev/null
    done
    _LHI_WATCHERS=""
    trap - EXIT

    if declare -f _lhi_orig_cd >/dev/null 2>&1; then
        eval "$(declare -f _lhi_orig_cd | sed '1s/_lhi_orig_cd/cd/')"
        unset -f _lhi_orig_cd 2>/dev/null
    else
        unset -f cd 2>/dev/null
    fi
    unset -f pushd popd 2>/dev/null
    unset -f _lhi_find_root _lhi_hook _lhi_deactivate _lhi_get_pid _lhi_remove 2>/dev/null
}

# Save existing cd override if present (e.g. from another tool)
if declare -f cd >/dev/null 2>&1; then
    eval "$(declare -f cd | sed '1s/cd ()/_lhi_orig_cd ()/')"
    cd() { _lhi_orig_cd "$@" && _lhi_hook; }
else
    cd() { builtin cd "$@" && _lhi_hook; }
fi
pushd() { builtin pushd "$@" && _lhi_hook; }
popd() { builtin popd "$@" && _lhi_hook; }

trap '_lhi_deactivate' EXIT

# Activate for current directory immediately
_lhi_hook
"#
}

/// Zsh hook — uses native typeset -A associative array with zsh syntax.
fn zsh_hook() -> &'static str {
    r#"# lhi shell hook — auto-start watcher on cd into .lhi projects
# eval "$(lhi activate)" in your .bashrc / .zshrc

typeset -A _LHI_PIDS
touch "$HOME/.lhi-watch.log" 2>/dev/null

_lhi_find_root() {
    local dir="$1"
    while [ "$dir" != "/" ]; do
        if [ -d "$dir/.lhi" ]; then
            printf '%s' "$dir"
            return 0
        fi
        dir="$(dirname "$dir")"
    done
    return 1
}

_lhi_hook() {
    local root
    root="$(_lhi_find_root "$PWD")" || return 0
    if (( ${+_LHI_PIDS[$root]} )); then
        kill -0 "${_LHI_PIDS[$root]}" 2>/dev/null && return 0
        unset '_LHI_PIDS['"$root"']'
    fi
    lhi watch "$root" >/dev/null 2>>"$HOME/.lhi-watch.log" &
    local wpid=$!
    disown "$wpid" 2>/dev/null
    sleep 0.1
    if kill -0 "$wpid" 2>/dev/null; then
        _LHI_PIDS[$root]=$wpid
    fi
}

_lhi_deactivate() {
    local root pid
    for root in "${(k)_LHI_PIDS[@]}"; do
        pid="${_LHI_PIDS[$root]}"
        kill "$pid" 2>/dev/null
    done
    unset _LHI_PIDS
    trap - EXIT

    if declare -f _lhi_orig_cd >/dev/null 2>&1; then
        eval "$(declare -f _lhi_orig_cd | sed '1s/_lhi_orig_cd/cd/')"
        unset -f _lhi_orig_cd 2>/dev/null
    else
        unset -f cd 2>/dev/null
    fi
    unset -f pushd popd 2>/dev/null
    unset -f _lhi_find_root _lhi_hook _lhi_deactivate 2>/dev/null
}

# Save existing cd override if present (e.g. from another tool)
if declare -f cd >/dev/null 2>&1; then
    eval "$(declare -f cd | sed '1s/cd ()/_lhi_orig_cd ()/')"
    cd() { _lhi_orig_cd "$@" && _lhi_hook; }
else
    cd() { builtin cd "$@" && _lhi_hook; }
fi
pushd() { builtin pushd "$@" && _lhi_hook; }
popd() { builtin popd "$@" && _lhi_hook; }

trap '_lhi_deactivate' EXIT

# Activate for current directory immediately
_lhi_hook
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_shell_bash() {
        assert_eq!(detect_shell("/bin/bash").unwrap(), "bash");
    }

    #[test]
    fn detect_shell_zsh() {
        assert_eq!(detect_shell("/bin/zsh").unwrap(), "zsh");
    }

    #[test]
    fn detect_shell_usr_local() {
        assert_eq!(detect_shell("/usr/local/bin/zsh").unwrap(), "zsh");
    }

    #[test]
    fn detect_shell_unsupported() {
        assert!(detect_shell("/usr/bin/fish").is_err());
    }

    #[test]
    fn detect_shell_empty() {
        assert!(detect_shell("").is_err());
    }

    fn assert_common_hook_properties(script: &str) {
        assert!(script.contains("_lhi_find_root()"));
        assert!(script.contains("_lhi_hook()"));
        assert!(script.contains("_lhi_deactivate()"));
        assert!(script.contains("lhi watch"));
        assert!(script.contains("trap '_lhi_deactivate' EXIT"));
        assert!(script.contains("cd()"));
        assert!(script.contains("pushd()"));
        assert!(script.contains("popd()"));
        assert!(script.contains("kill -0"));
        assert!(script.contains("dirname"));
        assert!(script.contains("while"));
        assert!(script.contains(".lhi"));
        assert!(script.contains("_lhi_orig_cd"));
        assert!(script.contains("declare -f cd"));
        assert!(script.contains("disown"));
        let last_line = script.lines().rev().find(|l| !l.trim().is_empty()).unwrap();
        assert_eq!(last_line.trim(), "_lhi_hook");
    }

    #[test]
    fn bash_hook_common_properties() {
        assert_common_hook_properties(bash_hook());
    }

    #[test]
    fn zsh_hook_common_properties() {
        assert_common_hook_properties(zsh_hook());
    }

    #[test]
    fn bash_hook_uses_portable_watchers_string() {
        let script = bash_hook();
        assert!(script.contains("_LHI_WATCHERS"));
        assert!(!script.contains("declare -A"));
        assert!(!script.contains("typeset -A"));
    }

    #[test]
    fn zsh_hook_uses_native_associative_array() {
        let script = zsh_hook();
        assert!(script.contains("typeset -A _LHI_PIDS"));
        assert!(script.contains("${(k)_LHI_PIDS[@]}"));
        assert!(script.contains("${+_LHI_PIDS[$root]}"));
    }

    #[test]
    fn hook_for_dispatches_correctly() {
        assert!(hook_for("bash").contains("_LHI_WATCHERS"));
        assert!(hook_for("zsh").contains("typeset -A _LHI_PIDS"));
    }
}

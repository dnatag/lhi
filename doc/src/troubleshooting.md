# Troubleshooting

## `(eval):2: parse error near '}'` on `source ~/.zshrc`

This happened in older versions when the shell hook was eval'd twice (e.g. re-sourcing your rc file). The hook tried to save the existing `cd` function using `declare -f cd | tail -n +2`, which strips the opening `{` in zsh (where the brace is on the same line as the function signature, unlike bash). This was fixed — update `lhi` and open a fresh shell.

If your current session is stuck, reset it:

```bash
unset -f cd _lhi_orig_cd pushd popd _lhi_hook _lhi_find_root _lhi_deactivate 2>/dev/null
source ~/.zshrc
```

## `lhi` commands are killed immediately (`killed` or exit code 137)

macOS Gatekeeper can SIGKILL unsigned or invalidly-signed binaries. This happens if you copy the `lhi` binary manually (e.g. `cp`) — the copy invalidates the ad-hoc code signature that `cargo install` creates. Fix it by re-signing:

```bash
codesign -f -s - $(which lhi)
```

Or reinstall with `cargo install --path .`, which produces a properly signed binary.

## `zsh: command not found: _lhi_orig_cd`

The shell hook failed to load (usually due to the parse error above), leaving `cd` overridden to call `_lhi_orig_cd` which was never defined. Open a new terminal, or reset manually:

```bash
unset -f cd _lhi_orig_cd 2>/dev/null
source ~/.zshrc
```

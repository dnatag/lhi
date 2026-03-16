pub(crate) use crate::util::get_file_mode;
#[cfg(test)]
pub(super) use crate::util::hex_sha256;

/// Checks whether a path is ignored by the given gitignore rules.
/// Tests both file and directory matching against the relative path.
#[cfg(test)]
pub(super) fn is_ignored_by(gitignore: &ignore::gitignore::Gitignore, root: &std::path::Path, path: &std::path::Path) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    gitignore
        .matched_path_or_any_parents(relative, false)
        .is_ignore()
        || gitignore
            .matched_path_or_any_parents(relative, true)
            .is_ignore()
}

//! Which workspaces this machine has opened, most recent first.
//!
//! **In the shell, not in `localStorage`.** Opening a workspace ends with the
//! webview reloading — the shell `eval`s `window.location.reload()` before
//! `open_project` even returns — so a page that tried to record the visit after
//! its `await` would race a navigation it cannot win. The shell is also the
//! only side that knows whether the gateway actually came up, and recording a
//! workspace that failed to open would put a dead entry at the top of the list.

use std::path::{Path, PathBuf};

/// Kept on disk. Deeper than the panel shows so a deleted folder — filtered out
/// when the list is read — does not leave the list short.
const CAP: usize = 5;

/// Read the remembered workspaces, most recent first.
///
/// A missing or unreadable store is an empty list, never an error: this is a
/// convenience, and no part of opening a workspace depends on it.
pub fn list(store: &Path) -> Vec<PathBuf> {
    let Ok(raw) = std::fs::read_to_string(store) else {
        return Vec::new();
    };
    let Ok(paths) = serde_json::from_str::<Vec<String>>(&raw) else {
        // A truncated write or a hand-edit. Starting over beats refusing to
        // show anything for the rest of the install's life.
        return Vec::new();
    };
    paths.into_iter().map(PathBuf::from).collect()
}

/// Put `project` at the front, keeping the rest in order.
pub fn record(store: &Path, project: &Path) {
    let mut paths = list(store);
    paths.retain(|p| p != project);
    paths.insert(0, project.to_path_buf());
    paths.truncate(CAP);

    let rendered: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();

    if let Some(parent) = store.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(&rendered) {
        let _ = std::fs::write(store, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(tmp: &tempfile::TempDir) -> PathBuf {
        tmp.path().join("recent-workspaces.json")
    }

    #[test]
    fn most_recent_comes_first() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);

        record(&store, Path::new("/a"));
        record(&store, Path::new("/b"));

        assert_eq!(list(&store), vec![PathBuf::from("/b"), PathBuf::from("/a")]);
    }

    #[test]
    fn reopening_moves_rather_than_duplicates() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);

        record(&store, Path::new("/a"));
        record(&store, Path::new("/b"));
        record(&store, Path::new("/a"));

        assert_eq!(list(&store), vec![PathBuf::from("/a"), PathBuf::from("/b")]);
    }

    #[test]
    fn the_oldest_falls_off() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);

        for i in 0..CAP + 2 {
            record(&store, &PathBuf::from(format!("/w{i}")));
        }

        let kept = list(&store);
        assert_eq!(kept.len(), CAP);
        assert_eq!(kept[0], PathBuf::from(format!("/w{}", CAP + 1)));
        assert!(!kept.contains(&PathBuf::from("/w0")));
    }

    #[test]
    fn nothing_remembered_yet_is_an_empty_list() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(list(&tmp.path().join("nope.json")).is_empty());
    }

    #[test]
    fn a_damaged_store_is_survivable() {
        // Half a write, or someone's editor. The next `record` rewrites it.
        let tmp = tempfile::tempdir().unwrap();
        let store = store(&tmp);
        std::fs::write(&store, "[\"/a\", ").unwrap();

        assert!(list(&store).is_empty());

        record(&store, Path::new("/b"));
        assert_eq!(list(&store), vec![PathBuf::from("/b")]);
    }
}

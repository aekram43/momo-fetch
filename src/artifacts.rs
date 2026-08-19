//! What a turn changed on disk.
//!
//! The Artifacts panel used to be derived from tool calls alone, which meant it
//! only ever knew about writes that went through `file_write` / `file_edit`.
//! Everything the agent did through the shell — a heredoc, `sed -i`, `>`, a
//! formatter, a codegen step — left no trace in the UI at all.
//!
//! So don't infer the writes: look. Stamp the tree before the turn, stamp it
//! after, and report the difference. That catches every path, whoever wrote it,
//! and it cannot drift from what a tool *claims* to have done.
//!
//! Scope is the sandbox root — the same tree the agent is allowed to touch.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Stop walking past this many files.
///
/// A project root is normally a repository; it can also be a home directory
/// someone pointed the harness at. A turn must not pay an unbounded filesystem
/// walk, and an incomplete stamp is not diffed at all — see [`changes`].
const MAX_FILES: usize = 50_000;

/// Directories never worth stamping.
///
/// `.git` alone would drown the panel: every `git` invocation rewrites index,
/// refs and logs, none of which is a file the user wrote. The rest are build
/// output that `.gitignore` usually covers already — listed here so an
/// un-ignored `node_modules` cannot make a turn crawl.
const SKIP_DIRS: [&str; 4] = [".git", "node_modules", "target", ".next"];

/// How a path differs between two stamps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Created,
    Modified,
    Deleted,
}

impl ChangeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Modified => "modified",
            Self::Deleted => "deleted",
        }
    }
}

/// One changed path, relative to the root that was stamped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: String,
    pub kind: ChangeKind,
}

/// Size and mtime of one file.
///
/// Both, not either: a rewrite that keeps the length is common (a one-character
/// edit), and a filesystem with coarse mtime granularity would miss two writes
/// in the same tick. Together they catch what each alone would not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

/// The state of a tree at one moment.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    files: HashMap<PathBuf, Stamp>,
    /// False when the walk hit [`MAX_FILES`] and stopped early.
    complete: bool,
}

impl Snapshot {
    /// Stamp every file under `root`.
    ///
    /// Blocking: call it from `spawn_blocking`, not from an async task.
    ///
    /// Honours `.gitignore` (and `.agentignore`, which the sandbox already
    /// loads for the agent) so build output does not swamp the result. Hidden
    /// files are *not* skipped: `.env` and `.harness/settings.json` are edits a
    /// user very much wants to see.
    pub fn take(root: &Path) -> Self {
        let mut files = HashMap::new();
        let mut complete = true;

        let walker = ignore::WalkBuilder::new(root)
            .hidden(false)
            // `.gitignore` is honoured only inside a repository unless this is
            // off, and a project root is not always one.
            .require_git(false)
            .add_custom_ignore_filename(".agentignore")
            .filter_entry(|entry| {
                !entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| SKIP_DIRS.contains(&name))
            })
            .build();

        for entry in walker.flatten() {
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            if files.len() >= MAX_FILES {
                complete = false;
                tracing::debug!(
                    root = %root.display(),
                    "artifact stamp stopped at {MAX_FILES} files"
                );
                break;
            }
            let Ok(meta) = entry.metadata() else { continue };
            files.insert(
                entry.into_path(),
                Stamp {
                    len: meta.len(),
                    modified: meta.modified().ok(),
                },
            );
        }

        Self { files, complete }
    }
}

/// What changed between two stamps of the same root.
///
/// Returns nothing when either stamp is incomplete. A partial walk would report
/// every unvisited file as deleted and every newly visited one as created —
/// confident nonsense, which is worse than an empty panel.
pub fn changes(before: &Snapshot, after: &Snapshot, root: &Path) -> Vec<Change> {
    if !before.complete || !after.complete {
        return Vec::new();
    }

    let mut out = Vec::new();

    for (path, stamp) in &after.files {
        match before.files.get(path) {
            None => out.push((path, ChangeKind::Created)),
            Some(was) if was != stamp => out.push((path, ChangeKind::Modified)),
            Some(_) => {}
        }
    }
    for path in before.files.keys() {
        if !after.files.contains_key(path) {
            out.push((path, ChangeKind::Deleted));
        }
    }

    let mut changes: Vec<Change> = out
        .into_iter()
        .map(|(path, kind)| Change {
            path: display_path(path, root),
            kind,
        })
        .collect();

    // Stable order: the panel is read top to bottom, and a HashMap walk would
    // reshuffle it on every turn.
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    changes
}

/// A path as the user thinks of it: relative to the project, when it is inside.
fn display_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }

    fn changed(dir: &Path, before: &Snapshot) -> Vec<(String, ChangeKind)> {
        let after = Snapshot::take(dir);
        changes(before, &after, dir)
            .into_iter()
            .map(|c| (c.path, c.kind))
            .collect()
    }

    #[test]
    fn a_new_file_is_created() {
        let tmp = tempfile::tempdir().unwrap();
        let before = Snapshot::take(tmp.path());

        write(tmp.path(), "notes.md", "hello");

        assert_eq!(
            changed(tmp.path(), &before),
            vec![("notes.md".to_string(), ChangeKind::Created)]
        );
    }

    #[test]
    fn an_edit_that_keeps_the_length_is_still_a_change() {
        // The case a size-only stamp misses, and the most common shape of a
        // real edit: one character swapped.
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "config.toml", "debug = true ");
        let before = Snapshot::take(tmp.path());

        write(tmp.path(), "config.toml", "debug = false");

        assert_eq!(
            changed(tmp.path(), &before),
            vec![("config.toml".to_string(), ChangeKind::Modified)]
        );
    }

    #[test]
    fn a_removed_file_is_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "old.txt", "x");
        let before = Snapshot::take(tmp.path());

        std::fs::remove_file(tmp.path().join("old.txt")).unwrap();

        assert_eq!(
            changed(tmp.path(), &before),
            vec![("old.txt".to_string(), ChangeKind::Deleted)]
        );
    }

    #[test]
    fn a_turn_that_touched_nothing_reports_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "a.txt", "a");
        let before = Snapshot::take(tmp.path());

        assert!(changed(tmp.path(), &before).is_empty());
    }

    #[test]
    fn nested_paths_are_reported_relative_to_the_root() {
        let tmp = tempfile::tempdir().unwrap();
        let before = Snapshot::take(tmp.path());

        write(tmp.path(), "src/deep/mod.rs", "fn main() {}");

        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(
            changed(tmp.path(), &before),
            vec![(format!("src{sep}deep{sep}mod.rs"), ChangeKind::Created)]
        );
    }

    #[test]
    fn dotfiles_are_reported() {
        // `.env` and `.harness/settings.json` are exactly the edits someone
        // wants to see, so the walk must not skip hidden entries.
        let tmp = tempfile::tempdir().unwrap();
        let before = Snapshot::take(tmp.path());

        write(tmp.path(), ".env", "KEY=1");

        assert_eq!(
            changed(tmp.path(), &before),
            vec![(".env".to_string(), ChangeKind::Created)]
        );
    }

    #[test]
    fn git_internals_are_not_artifacts() {
        // Any `git` command rewrites these. Reporting them would bury the one
        // file the user actually edited.
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), ".git/HEAD", "ref: refs/heads/main");
        let before = Snapshot::take(tmp.path());

        write(tmp.path(), ".git/index", "binary");
        write(tmp.path(), "README.md", "hi");

        assert_eq!(
            changed(tmp.path(), &before),
            vec![("README.md".to_string(), ChangeKind::Created)]
        );
    }

    #[test]
    fn gitignored_output_is_not_an_artifact() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), ".gitignore", "build/\n");
        let before = Snapshot::take(tmp.path());

        write(tmp.path(), "build/app.js", "compiled");
        write(tmp.path(), "app.ts", "source");

        assert_eq!(
            changed(tmp.path(), &before),
            vec![("app.ts".to_string(), ChangeKind::Created)]
        );
    }

    #[test]
    fn an_incomplete_stamp_is_not_diffed() {
        // Better an empty panel than a list claiming the turn deleted a tree it
        // never visited.
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "a.txt", "a");

        let mut before = Snapshot::take(tmp.path());
        before.complete = false;
        let after = Snapshot::take(tmp.path());

        assert!(changes(&before, &after, tmp.path()).is_empty());
    }
}

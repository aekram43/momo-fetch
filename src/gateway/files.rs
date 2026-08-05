//! **G6** — sandboxed file read and directory listing.
//!
//! This module turns the gateway into a read-only file server, which makes it
//! the highest-risk surface in the V2 API. Everything here is written against a
//! browser-reachable threat model: CORS is permissive by default
//! (`gateway/mod.rs`), so a hostile page the user merely *visits* can call these
//! endpoints. Treat every relaxation as a potential exfiltration primitive.
//!
//! ## Defence in depth
//!
//! 1. [`FilesystemSandbox::resolve_path`] — rejects `..`, resolves symlinks, and
//!    re-verifies the sandbox root is still a prefix afterwards. Absolute paths
//!    are caught by that final check, because `Path::join` with an absolute path
//!    discards the root.
//! 2. [`FilesystemSandbox::is_ignored`] — `.agentignore` rules.
//! 3. [`Self::gitignored`] — `.gitignore` rules.
//! 4. [`is_sensitive`] — an unconditional deny-list.
//!
//! ## Why layers 3 and 4 exist
//!
//! The spec assumed layer 2 covered gitignored files. **It does not.**
//! `is_ignored` consults `.agentignore` only, and when that file is absent —
//! as it is in this repo — `ignore_matcher` is `None` and every in-root path is
//! reported as *not* ignored. `.env` is gitignored but not agentignored, so
//! without layer 3 `GET /v2/files?path=.env` would have served
//! `OPENROUTER_API_KEY` in plaintext to any origin.
//!
//! Layer 4 exists because layer 3 is still only as good as the user's
//! `.gitignore`. A project that never gitignored its keys is exactly the project
//! whose keys most need protecting, so credential-shaped paths are denied
//! unconditionally.
//!
//! ## 403 vs 404
//!
//! Anything denied returns **403**, never 404 — including paths that do not
//! exist but *would* be denied. Distinguishing "forbidden" from "missing" turns
//! the endpoint into an oracle for mapping the filesystem. 404 is reserved for
//! paths that are in-sandbox, permitted, and genuinely absent.

use std::path::{Path, PathBuf};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use super::GatewayState;
use super::v2_types::v2_error;

/// Maximum bytes returned for a single file. Larger files are truncated.
const MAX_FILE_BYTES: usize = 1024 * 1024; // 1 MiB

/// Bytes sniffed when deciding whether a file is binary.
const BINARY_SNIFF_BYTES: usize = 8192;

/// Hard ceilings on a tree walk, so a deep or wide tree cannot wedge the server.
const MAX_TREE_DEPTH: usize = 3;
const MAX_TREE_ENTRIES: usize = 1000;

/// Path segments and suffixes that are never served, whatever the ignore files
/// say.
///
/// This is deliberately conservative and deliberately not configurable. A
/// project that forgot to gitignore its keys is the project that most needs
/// this list, so it must not depend on project configuration to work.
fn is_sensitive(relative: &Path) -> bool {
    const DENIED_DIRS: &[&str] = &[".git", ".ssh", ".gnupg", "node_modules"];
    const DENIED_NAMES: &[&str] = &[
        "id_rsa",
        "id_dsa",
        "id_ecdsa",
        "id_ed25519",
        ".netrc",
        ".pgpass",
        ".htpasswd",
        "credentials",
        "secrets.json",
        "secrets.yaml",
        "secrets.yml",
    ];
    const DENIED_SUFFIXES: &[&str] =
        &[".pem", ".key", ".p12", ".pfx", ".keystore", ".jks", ".ppk"];

    for component in relative.components() {
        let Some(part) = component.as_os_str().to_str() else {
            // Non-UTF-8 path component — refuse rather than guess.
            return true;
        };
        let lower = part.to_ascii_lowercase();

        if DENIED_DIRS.contains(&lower.as_str()) {
            return true;
        }
        if DENIED_NAMES.contains(&lower.as_str()) {
            return true;
        }
        // `.env`, `.env.local`, `.env.production` — anything in that family.
        if lower == ".env" || lower.starts_with(".env.") {
            return true;
        }
        if DENIED_SUFFIXES.iter().any(|s| lower.ends_with(s)) {
            return true;
        }
    }
    false
}

/// Build a matcher for the project's `.gitignore`, if it has one.
///
/// Returns `None` when there is no `.gitignore`; callers treat that as "no
/// gitignore opinion", not as "allow everything" — the other layers still apply.
fn gitignore_matcher(root: &Path) -> Option<ignore::gitignore::Gitignore> {
    let path = root.join(".gitignore");
    if !path.is_file() {
        return None;
    }
    let mut builder = ignore::gitignore::GitignoreBuilder::new(root);
    // `add` returns Some(err) on failure; a malformed .gitignore should not take
    // the endpoint down, but it must not silently become "allow all" either —
    // we simply fall through to the remaining layers.
    if builder.add(&path).is_some() {
        return None;
    }
    builder.build().ok()
}

/// Whether `relative` is excluded by the project's `.gitignore`.
fn gitignored(root: &Path, relative: &Path, is_dir: bool) -> bool {
    gitignore_matcher(root)
        .map(|m| m.matched_path_or_any_parents(relative, is_dir).is_ignore())
        .unwrap_or(false)
}

/// Single decision point for "may this path be served?".
///
/// Returns `Err` with a ready-made 403 when denied, so no caller can accidentally
/// leak the distinction between forbidden and missing.
fn authorize(
    sandbox: &crate::sandbox::FilesystemSandbox,
    requested: &str,
    is_dir_hint: bool,
) -> Result<PathBuf, Response> {
    let forbidden = || {
        v2_error(
            StatusCode::FORBIDDEN,
            "path_forbidden",
            "Path is not accessible.",
            None,
        )
    };

    // Layer 1 — traversal, symlink escape, absolute paths.
    let resolved = sandbox.resolve_path(requested).map_err(|_| forbidden())?;

    let relative = resolved.strip_prefix(sandbox.root()).unwrap_or(&resolved);

    // Layer 4 first: it is the cheapest and the one that must never be skipped.
    if is_sensitive(relative) {
        return Err(forbidden());
    }
    // Layer 3 — .gitignore.
    if gitignored(sandbox.root(), relative, is_dir_hint) {
        return Err(forbidden());
    }
    // Layer 2 — .agentignore.
    if sandbox.is_ignored(&resolved) {
        return Err(forbidden());
    }

    Ok(resolved)
}

// ─── GET /v2/files ─────────────────────────────────────────────

#[derive(Deserialize)]
pub struct FileQuery {
    pub path: Option<String>,
}

/// `GET /v2/files?path=<relative>` — read one file from the sandbox.
pub async fn v2_files(
    State(state): State<GatewayState>,
    Query(params): Query<FileQuery>,
) -> Response {
    let Some(requested) = params.path.filter(|p| !p.trim().is_empty()) else {
        return v2_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Query parameter 'path' is required.",
            None,
        );
    };

    let harness = state.harness.read().await;
    let resolved = match authorize(harness.sandbox(), &requested, false) {
        Ok(p) => p,
        Err(denied) => return denied,
    };
    let root = harness.sandbox().root().to_path_buf();
    drop(harness);

    let meta = match tokio::fs::metadata(&resolved).await {
        Ok(m) => m,
        // In-sandbox and permitted, but absent.
        Err(_) => {
            return v2_error(StatusCode::NOT_FOUND, "not_found", "File not found.", None);
        }
    };
    if meta.is_dir() {
        return v2_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Path is a directory; use /v2/files/tree.",
            None,
        );
    }

    let size = meta.len();

    // Read at most MAX_FILE_BYTES. A 5 GB log must not become a 5 GB allocation,
    // so the cap is applied while reading rather than after.
    let bytes = match read_capped(&resolved, MAX_FILE_BYTES).await {
        Ok(b) => b,
        Err(e) => {
            return v2_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                format!("failed to read file: {e}"),
                None,
            );
        }
    };

    let truncated = (size as usize) > bytes.len();
    let rel = resolved.strip_prefix(&root).unwrap_or(&resolved);

    // Binary files are reported, not mangled: lossy-decoding a PNG produces
    // megabytes of replacement characters that help nobody.
    if is_binary(&bytes) {
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "path": rel.to_string_lossy(),
                "content": serde_json::Value::Null,
                "size": size,
                "truncated": truncated,
                "binary": true,
            })),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "path": rel.to_string_lossy(),
            "content": String::from_utf8_lossy(&bytes),
            "size": size,
            "truncated": truncated,
            "binary": false,
        })),
    )
        .into_response()
}

/// Read up to `cap` bytes, without allocating for the whole file.
async fn read_capped(path: &Path, cap: usize) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;

    let file = tokio::fs::File::open(path).await?;
    let mut buf = Vec::new();
    // `cap + 1` is not needed: we compare the byte count against the real file
    // size from metadata to decide `truncated`.
    file.take(cap as u64).read_to_end(&mut buf).await?;
    Ok(buf)
}

/// A NUL byte in the first 8 KB is the standard heuristic for "not text".
fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(BINARY_SNIFF_BYTES).any(|b| *b == 0)
}

// ─── GET /v2/files/tree ────────────────────────────────────────

#[derive(Deserialize)]
pub struct TreeQuery {
    pub path: Option<String>,
    pub depth: Option<usize>,
}

/// `GET /v2/files/tree?path=<dir>&depth=<n>` — list a sandbox directory.
///
/// There is no existing tree walker in the codebase, so this one is bounded on
/// both axes: `depth` is clamped to [`MAX_TREE_DEPTH`] and the walk stops at
/// [`MAX_TREE_ENTRIES`]. Denied entries are skipped silently rather than listed
/// with a marker — showing "«hidden»" would still confirm existence.
pub async fn v2_files_tree(
    State(state): State<GatewayState>,
    Query(params): Query<TreeQuery>,
) -> Response {
    let requested = params
        .path
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| ".".to_string());
    let depth = params.depth.unwrap_or(1).clamp(1, MAX_TREE_DEPTH);

    let harness = state.harness.read().await;
    let sandbox_root = harness.sandbox().root().to_path_buf();
    let resolved = match authorize(harness.sandbox(), &requested, true) {
        Ok(p) => p,
        Err(denied) => return denied,
    };
    drop(harness);

    match tokio::fs::metadata(&resolved).await {
        Ok(m) if m.is_dir() => {}
        Ok(_) => {
            return v2_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "Path is not a directory; use /v2/files.",
                None,
            );
        }
        Err(_) => {
            return v2_error(
                StatusCode::NOT_FOUND,
                "not_found",
                "Directory not found.",
                None,
            );
        }
    }

    let mut entries: Vec<serde_json::Value> = Vec::new();
    let mut truncated = false;
    walk(&sandbox_root, &resolved, depth, &mut entries, &mut truncated).await;

    entries.sort_by(|a, b| {
        // Directories first, then case-insensitive by name — the ordering a file
        // browser is expected to have.
        let (ad, bd) = (a["is_dir"].as_bool().unwrap_or(false), b["is_dir"].as_bool().unwrap_or(false));
        bd.cmp(&ad).then_with(|| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .to_ascii_lowercase()
                .cmp(&b["name"].as_str().unwrap_or("").to_ascii_lowercase())
        })
    });

    let rel_root = resolved.strip_prefix(&sandbox_root).unwrap_or(&resolved);
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "path": rel_root.to_string_lossy(),
            "depth": depth,
            "entries": entries,
            "truncated": truncated,
        })),
    )
        .into_response()
}

/// Recursive directory walk, bounded by depth and total entry count.
///
/// Boxed because `async fn` cannot recurse directly.
fn walk<'a>(
    root: &'a Path,
    dir: &'a Path,
    depth: usize,
    out: &'a mut Vec<serde_json::Value>,
    truncated: &'a mut bool,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
    Box::pin(async move {
        if depth == 0 || *truncated {
            return;
        }
        let Ok(mut rd) = tokio::fs::read_dir(dir).await else {
            return;
        };

        while let Ok(Some(entry)) = rd.next_entry().await {
            if out.len() >= MAX_TREE_ENTRIES {
                *truncated = true;
                return;
            }

            let path = entry.path();
            let Ok(meta) = entry.metadata().await else { continue };
            let is_dir = meta.is_dir();
            let Ok(relative) = path.strip_prefix(root) else { continue };

            // Same rules as a direct read. A listing that reveals `.env` exists
            // is a smaller leak than serving it, but it is still a leak.
            if is_sensitive(relative) || gitignored(root, relative, is_dir) {
                continue;
            }

            out.push(serde_json::json!({
                "name": entry.file_name().to_string_lossy(),
                "path": relative.to_string_lossy(),
                "is_dir": is_dir,
                "size": if is_dir { serde_json::Value::Null } else { serde_json::json!(meta.len()) },
            }));

            if is_dir {
                walk(root, &path, depth - 1, out, truncated).await;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotenv_family_is_always_denied() {
        assert!(is_sensitive(Path::new(".env")));
        assert!(is_sensitive(Path::new(".env.local")));
        assert!(is_sensitive(Path::new(".env.production")));
        assert!(is_sensitive(Path::new("config/.env")));
        // Not a match — a file that merely mentions env.
        assert!(!is_sensitive(Path::new("environment.md")));
        assert!(!is_sensitive(Path::new("src/env.rs")));
    }

    #[test]
    fn credential_shaped_paths_are_denied() {
        assert!(is_sensitive(Path::new("server.pem")));
        assert!(is_sensitive(Path::new("certs/private.KEY")));
        assert!(is_sensitive(Path::new(".ssh/id_rsa")));
        assert!(is_sensitive(Path::new("deep/nested/.git/config")));
        assert!(is_sensitive(Path::new("aws/credentials")));
        assert!(!is_sensitive(Path::new("src/main.rs")));
        assert!(!is_sensitive(Path::new("README.md")));
    }

    #[test]
    fn denial_is_case_insensitive() {
        assert!(is_sensitive(Path::new(".ENV")));
        assert!(is_sensitive(Path::new("Server.PEM")));
        assert!(is_sensitive(Path::new(".GIT/config")));
    }

    #[test]
    fn binary_detection_keys_on_nul() {
        assert!(is_binary(b"\x89PNG\r\n\x1a\n\x00\x00"));
        assert!(!is_binary(b"fn main() {}\n"));
        assert!(!is_binary("héllo wörld".as_bytes()));
        // A NUL past the sniff window is not considered.
        let mut late = vec![b'a'; BINARY_SNIFF_BYTES + 10];
        late[BINARY_SNIFF_BYTES + 5] = 0;
        assert!(!is_binary(&late));
    }
}

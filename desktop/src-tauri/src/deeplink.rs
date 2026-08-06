//! **T9** — the `momo://` URL scheme.
//!
//! # This handles untrusted input
//!
//! A deep link can be fired by any web page, any document, any chat message.
//! `momo://open?path=…` proposes a new **sandbox root** — the directory the
//! agent's file tools can read and write, and that `/v2/files` will serve. A
//! link that silently re-rooted the agent at `~/` or `/` would be a
//! one-click filesystem grant.
//!
//! So the rule here is: **parse, validate, then ask.** The link can only ever
//! *propose*; the user confirms in the UI (spec §9.7). Nothing in this module
//! starts a gateway or touches the harness.

use tauri::{AppHandle, Emitter, Manager, Runtime};

/// A validated request extracted from a `momo://` URL.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct OpenRequest {
    pub path: String,
}

/// Reasons a link is refused, kept distinct so the UI can explain itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    NotOurScheme,
    UnknownAction,
    MissingPath,
    NotAbsolute,
    Traversal,
    NotADirectory,
}

impl Rejection {
    pub fn message(self) -> &'static str {
        match self {
            Rejection::NotOurScheme => "Not a momo:// link.",
            Rejection::UnknownAction => "That momo:// link asks for something this app does not do.",
            Rejection::MissingPath => "That link is missing a path.",
            Rejection::NotAbsolute => "Deep links must give an absolute path.",
            Rejection::Traversal => "That path contains traversal segments.",
            Rejection::NotADirectory => "That path is not a directory on this machine.",
        }
    }
}

/// Parse and validate a `momo://open?path=…` URL.
///
/// Deliberately strict — everything not explicitly allowed is refused:
///
/// * only the `open` action exists; anything else is rejected rather than
///   ignored, so a future action cannot be silently mis-handled by an old build
/// * the path must be absolute. A relative path would resolve against whatever
///   the app's working directory happens to be, which is not something the
///   sender should get to influence
/// * `..` in any position is refused outright. This is not a sandbox-escape
///   check — there is no sandbox yet at this point — it is a "say what you
///   mean" check, so the directory the user is shown in the confirmation is
///   literally the one that will be opened
/// * it must already exist and be a directory, so the confirmation names
///   something real
pub fn parse(url: &str) -> Result<OpenRequest, Rejection> {
    let rest = url
        .strip_prefix("momo://")
        .ok_or(Rejection::NotOurScheme)?;

    // `momo://open?path=/x` — the "host" is the action.
    let (action, query) = match rest.split_once('?') {
        Some((a, q)) => (a, q),
        None => (rest, ""),
    };
    if action.trim_end_matches('/') != "open" {
        return Err(Rejection::UnknownAction);
    }

    let raw = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("path="))
        .ok_or(Rejection::MissingPath)?;

    let path = percent_decode(raw);
    if path.is_empty() {
        return Err(Rejection::MissingPath);
    }
    if !std::path::Path::new(&path).is_absolute() {
        return Err(Rejection::NotAbsolute);
    }
    if std::path::Path::new(&path)
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(Rejection::Traversal);
    }
    if !std::path::Path::new(&path).is_dir() {
        return Err(Rejection::NotADirectory);
    }

    Ok(OpenRequest { path })
}

/// Minimal percent-decoding. Only `%XX` and `+`; no other rewriting.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    Ok(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Forward a validated link to the UI as a *proposal*.
///
/// The UI shows the same confirmation as the Files panel's "open another
/// project", because it is the same consequential action arriving by a less
/// trustworthy route.
pub fn dispatch<R: Runtime>(app: &AppHandle<R>, url: &str) {
    // Raise the window first: a confirmation the user cannot see is worse than
    // no confirmation, and a deep link often arrives while the app is hidden.
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }

    match parse(url) {
        Ok(req) => {
            log::info!("deep link proposes opening {}", req.path);
            let _ = app.emit("momo:open-project-request", req);
        }
        Err(reason) => {
            log::warn!("rejected deep link {url:?}: {}", reason.message());
            let _ = app.emit("momo:deep-link-rejected", reason.message());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_an_existing_absolute_directory() {
        let tmp = std::env::temp_dir();
        let url = format!("momo://open?path={}", tmp.display());
        assert_eq!(parse(&url).unwrap().path, tmp.display().to_string());
    }

    #[test]
    fn percent_encoded_spaces_survive() {
        let tmp = std::env::temp_dir().join("momo deep link test");
        std::fs::create_dir_all(&tmp).unwrap();
        let encoded = tmp.display().to_string().replace(' ', "%20");
        assert_eq!(
            parse(&format!("momo://open?path={encoded}")).unwrap().path,
            tmp.display().to_string()
        );
        let _ = std::fs::remove_dir(&tmp);
    }

    #[test]
    fn rejects_traversal() {
        // The danger case: a link that looks scoped but climbs out.
        let url = "momo://open?path=/tmp/../etc";
        assert_eq!(parse(url), Err(Rejection::Traversal));
        assert_eq!(
            parse("momo://open?path=%2Ftmp%2F..%2Fetc"),
            Err(Rejection::Traversal),
            "encoding the traversal must not slip past"
        );
    }

    #[test]
    fn rejects_relative_paths() {
        assert_eq!(parse("momo://open?path=project"), Err(Rejection::NotAbsolute));
        assert_eq!(parse("momo://open?path=./project"), Err(Rejection::NotAbsolute));
    }

    #[test]
    fn rejects_files_and_missing_paths() {
        assert_eq!(parse("momo://open"), Err(Rejection::MissingPath));
        assert_eq!(parse("momo://open?path="), Err(Rejection::MissingPath));
        assert_eq!(
            parse("momo://open?path=/definitely/not/here/xyzzy"),
            Err(Rejection::NotADirectory)
        );
    }

    #[test]
    fn rejects_other_schemes_and_actions() {
        assert_eq!(parse("https://evil.example/?path=/tmp"), Err(Rejection::NotOurScheme));
        // An action this build does not know must be refused, not ignored.
        assert_eq!(parse("momo://exec?path=/tmp"), Err(Rejection::UnknownAction));
        assert_eq!(parse("momo://open/../exec?path=/tmp"), Err(Rejection::UnknownAction));
    }
}

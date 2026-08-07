use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use adk_session::{
    CreateRequest, DeleteRequest, Event, GetRequest, ListRequest, Session, SessionService,
    SqliteSessionService,
};

const APP_NAME: &str = "momo-fetch";
const DEFAULT_USER: &str = "default-user";

/// Session-state key holding the human-readable title.
const TITLE_KEY: &str = "momo.title";

/// Session-state flag: the title was typed by a person, so auto-titling must
/// leave it alone. Without this, the first message of the *next* turn would
/// silently overwrite a name someone chose.
const TITLE_LOCKED_KEY: &str = "momo.title_locked";

/// Titles are a sidebar label, not a summary. Long enough to tell two
/// conversations apart, short enough not to wrap in a 288px rail.
const TITLE_MAX: usize = 60;

/// First line of `text`, trimmed and clipped to [`TITLE_MAX`].
///
/// A prompt is often a paragraph; the first line is almost always the ask.
/// Clipping mid-word is fine — this is a label, and the full text is one click
/// away in the transcript.
pub fn title_from_prompt(text: &str) -> Option<String> {
    let first = text.lines().find(|l| !l.trim().is_empty())?.trim();
    if first.is_empty() {
        return None;
    }
    let mut out: String = first.chars().take(TITLE_MAX).collect();
    if first.chars().count() > TITLE_MAX {
        out.push('…');
    }
    Some(out)
}

/// Manages session persistence using adk-session SQLite backend.
///
/// Wraps the adk-session `SessionService` trait with convenience methods
/// for creating, listing, resuming, and deleting sessions.
pub struct SessionManager {
    service: Arc<dyn SessionService>,
}

impl SessionManager {
    /// Create a new session manager backed by SQLite.
    ///
    /// The database file is created if it doesn't exist. Migrations are run
    /// automatically to ensure the schema is up-to-date.
    pub async fn new(db_path: &Path) -> anyhow::Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let db_url = format!("sqlite:{}?mode=rwc", db_path.display());
        let service = SqliteSessionService::new(&db_url).await?;

        // Run migrations (idempotent — safe on both fresh and existing DBs)
        service.migrate().await?;

        Ok(Self {
            service: Arc::new(service),
        })
    }

    /// Create an in-memory session manager (for tests).
    #[allow(dead_code)]
    pub fn new_in_memory() -> Self {
        use adk_session::InMemorySessionService;
        Self {
            service: Arc::new(InMemorySessionService::new()),
        }
    }

    /// Get the underlying session service (for passing to adk-runner).
    pub fn service(&self) -> Arc<dyn SessionService> {
        self.service.clone()
    }

    /// Create a new session with an optional explicit ID.
    ///
    /// If `session_id` is `None`, a UUID is auto-generated.
    /// Returns the created session.
    pub async fn create_session(
        &self,
        session_id: Option<&str>,
    ) -> anyhow::Result<Box<dyn Session>> {
        let req = CreateRequest {
            app_name: APP_NAME.to_string(),
            user_id: DEFAULT_USER.to_string(),
            session_id: session_id.map(|s| s.to_string()),
            state: HashMap::new(),
        };
        let session = self.service.create(req).await?;
        Ok(session)
    }

    /// Get an existing session by ID.
    pub async fn get_session(&self, session_id: &str) -> anyhow::Result<Box<dyn Session>> {
        let req = GetRequest {
            app_name: APP_NAME.to_string(),
            user_id: DEFAULT_USER.to_string(),
            session_id: session_id.to_string(),
            num_recent_events: None,
            after: None,
        };
        let session = self.service.get(req).await?;
        Ok(session)
    }

    /// List all sessions for the default user.
    pub async fn list_sessions(&self) -> anyhow::Result<Vec<SessionInfo>> {
        let req = ListRequest {
            app_name: APP_NAME.to_string(),
            user_id: DEFAULT_USER.to_string(),
            limit: None,
            offset: None,
        };
        let sessions = self.service.list(req).await?;
        let mut infos: Vec<SessionInfo> = sessions
            .into_iter()
            .map(|s| SessionInfo {
                id: s.id().to_string(),
                updated_at: s.last_update_time(),
                // Free: `SessionService::list` selects the `state` column even
                // though it skips events, so the title costs no extra query and
                // there is no N+1 here.
                title: s
                    .state()
                    .get(TITLE_KEY)
                    .and_then(|v| v.as_str().map(str::to_string)),
                // Not `s.events().len()` — see the field docs. `list` does not
                // load events, so that expression is always 0.
                event_count: None,
            })
            .collect();
        // Most recently updated first
        infos.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(infos)
    }

    /// Write a session's title into its persisted state.
    ///
    /// The only way to persist state through `SessionService` is a `state_delta`
    /// on an appended event, which the SQLite backend merges into the sessions
    /// row. The event carries no content, and `/v2/sessions/{id}/messages` skips
    /// content-free events, so this never shows up in a transcript. It does add
    /// one to the event count returned by `get_session` — worth knowing, not
    /// worth a schema of our own to avoid.
    async fn write_title(
        &self,
        session_id: &str,
        title: &str,
        locked: bool,
    ) -> anyhow::Result<()> {
        // The invocation id is bookkeeping for a turn; this event is not one, so
        // it gets a name that says what it is if anyone reads the table.
        let mut event = Event::new("momo-title");
        event
            .actions
            .state_delta
            .insert(TITLE_KEY.to_string(), serde_json::json!(title));
        if locked {
            event
                .actions
                .state_delta
                .insert(TITLE_LOCKED_KEY.to_string(), serde_json::json!(true));
        }
        self.service.append_event(session_id, event).await?;
        Ok(())
    }

    /// Set a title chosen by a person. Locks it against auto-titling.
    pub async fn rename_session(&self, session_id: &str, title: &str) -> anyhow::Result<()> {
        let title = title.trim();
        if title.is_empty() {
            anyhow::bail!("A session title cannot be empty.");
        }
        let clipped: String = title.chars().take(TITLE_MAX).collect();
        self.write_title(session_id, &clipped, true).await
    }

    /// Give a session a title derived from its first prompt, if it has neither
    /// a title already nor one a person chose.
    ///
    /// Called at the start of a turn. Best-effort by design: a session that
    /// cannot be titled is a cosmetic problem, and failing a turn over a label
    /// would be absurd.
    pub async fn auto_title(&self, session_id: &str, prompt: &str) {
        let Some(title) = title_from_prompt(prompt) else {
            return;
        };
        let Ok(session) = self.get_session(session_id).await else {
            return;
        };
        let state = session.state();
        if state.get(TITLE_KEY).is_some() || state.get(TITLE_LOCKED_KEY).is_some() {
            return;
        }
        let _ = self.write_title(session_id, &title, false).await;
    }

    /// Delete a session by ID.
    #[allow(dead_code)]
    pub async fn delete_session(&self, session_id: &str) -> anyhow::Result<()> {
        let req = DeleteRequest {
            app_name: APP_NAME.to_string(),
            user_id: DEFAULT_USER.to_string(),
            session_id: session_id.to_string(),
        };
        self.service.delete(req).await?;
        Ok(())
    }

}

/// Summary info about a session (for display in /sessions).
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// Human-readable label, when the session has one.
    ///
    /// `None` means untitled — the UI shows the id, which is what every session
    /// looked like before titles existed. Never invent one here; an id is
    /// honest, and a made-up name is worse than a hex string.
    pub title: Option<String>,
    /// `None` when the count is unknown.
    ///
    /// [`SessionManager::list_sessions`] cannot fill this in: the backing
    /// `SessionService::list` is a metadata-only query that returns sessions
    /// with an empty event vector, so counting there always yields 0. Reporting
    /// a confident `0` for a session with 21 events is worse than admitting we
    /// do not know — same reasoning as `tool_count` in G4.
    ///
    /// Use [`SessionManager::get_session`] when the real count matters.
    pub event_count: Option<usize>,
}

impl std::fmt::Display for SessionInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let time = self.updated_at.format("%Y-%m-%d %H:%M");
        match self.event_count {
            Some(n) => write!(f, "{}  ({n} events, updated {time})", self.id),
            None => write!(f, "{}  (updated {time})", self.id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_and_list_sessions() {
        let mgr = SessionManager::new_in_memory();

        let s1 = mgr.create_session(None).await.unwrap();
        let s2 = mgr.create_session(None).await.unwrap();

        let ids: Vec<String> = mgr.list_sessions().await.unwrap().iter().map(|s| s.id.clone()).collect();
        assert!(ids.contains(&s1.id().to_string()));
        assert!(ids.contains(&s2.id().to_string()));
    }

    #[tokio::test]
    async fn test_create_session_with_explicit_id() {
        let mgr = SessionManager::new_in_memory();

        let s = mgr.create_session(Some("test-session-123")).await.unwrap();
        assert_eq!(s.id(), "test-session-123");
    }

    #[tokio::test]
    async fn test_get_session() {
        let mgr = SessionManager::new_in_memory();

        let created = mgr.create_session(Some("get-test")).await.unwrap();
        let fetched = mgr.get_session("get-test").await.unwrap();
        assert_eq!(fetched.id(), "get-test");
    }

    #[tokio::test]
    async fn test_delete_session() {
        let mgr = SessionManager::new_in_memory();

        mgr.create_session(Some("delete-me")).await.unwrap();
        mgr.delete_session("delete-me").await.unwrap();

        let sessions = mgr.list_sessions().await.unwrap();
        let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
        assert!(!ids.contains(&"delete-me"));
    }

    #[tokio::test]
    async fn test_session_info_display() {
        let info = SessionInfo {
            id: "abc-123".to_string(),
            updated_at: chrono::Utc::now(),
            title: None,
            event_count: Some(5),
        };
        let display = format!("{info}");
        assert!(display.contains("abc-123"));
        assert!(display.contains("5 events"));

        // Unknown count omits the clause rather than printing "0 events".
        let unknown = SessionInfo {
            id: "abc-123".to_string(),
            updated_at: chrono::Utc::now(),
            title: None,
            event_count: None,
        };
        let display = format!("{unknown}");
        assert!(display.contains("abc-123"));
        assert!(!display.contains("events"));
    }

    #[tokio::test]
    async fn test_sqlite_session_manager() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test-sessions.db");

        let mgr = SessionManager::new(&db_path).await.unwrap();
        let s = mgr.create_session(Some("sqlite-test")).await.unwrap();
        assert_eq!(s.id(), "sqlite-test");

        // Verify DB file was created
        assert!(db_path.exists());

        // List sessions
        let sessions = mgr.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "sqlite-test");
    }

    #[tokio::test]
    async fn test_sqlite_session_persistence() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("persist.db");

        // Create session with first manager
        {
            let mgr = SessionManager::new(&db_path).await.unwrap();
            mgr.create_session(Some("persist-test")).await.unwrap();
        }

        // Reopen and verify session still exists
        let mgr = SessionManager::new(&db_path).await.unwrap();
        let sessions = mgr.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "persist-test");
    }
}

#[cfg(test)]
mod title_tests {
    use super::*;

    #[test]
    fn takes_the_first_non_blank_line() {
        assert_eq!(
            title_from_prompt("\n\n  fix the login bug  \nand then deploy"),
            Some("fix the login bug".to_string())
        );
    }

    #[test]
    fn clips_long_prompts_and_marks_the_cut() {
        let long = "a".repeat(200);
        let t = title_from_prompt(&long).unwrap();
        assert_eq!(t.chars().count(), TITLE_MAX + 1, "60 chars plus the ellipsis");
        assert!(t.ends_with('…'));
    }

    #[test]
    fn a_prompt_exactly_at_the_limit_is_not_marked() {
        let exact = "b".repeat(TITLE_MAX);
        assert_eq!(title_from_prompt(&exact), Some(exact));
    }

    #[test]
    fn counts_characters_not_bytes() {
        // Clipping by byte would split a multi-byte char and panic.
        let thai = "ทดสอบ".repeat(40);
        let t = title_from_prompt(&thai).unwrap();
        assert_eq!(t.chars().count(), TITLE_MAX + 1);
    }

    #[test]
    fn nothing_to_title_yields_none() {
        assert_eq!(title_from_prompt(""), None);
        assert_eq!(title_from_prompt("   \n\t\n  "), None);
    }
}

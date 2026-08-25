use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::Arc;

use adk_session::{
    CreateRequest, DeleteRequest, Event, GetRequest, ListRequest, Session, SessionService,
    SqliteSessionService,
};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

/// How long a writer waits for the lock before reporting failure. Long enough
/// to cover a team of workers landing on the same turn boundary, short enough
/// that a genuinely stuck database still surfaces as an error.
const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// How long to wait for a free connection from the pool.
const ACQUIRE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// A single process only ever runs one turn at a time; the concurrency that
/// matters here is between processes, not inside one.
const MAX_CONNECTIONS: u32 = 5;

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

/// Wraps a session service, retrying the writes SQLite refuses outright.
///
/// WAL and a busy timeout get a lone writer to wait its turn. Neither helps a
/// transaction that began as a reader and then tried to write: SQLite fails
/// that one immediately and on purpose, because two readers both waiting to
/// upgrade would deadlock forever. adk-session opens every write exactly that
/// way — `BEGIN`, read the existing state, insert — so two agents writing in
/// the same instant still cost one of them its turn, with the same
/// `database is locked (code: 5)` a team of workers used to die on.
///
/// The transaction is already rolled back by the time the error reaches us, so
/// the retry starts clean. Backoff is jittered because the whole point is that
/// two processes are in step, and retrying in step would keep them there.
struct RetryingSessionService {
    inner: Arc<dyn SessionService>,
}

/// Enough attempts to cover a whole team arriving at once; the last delay is
/// still under a second, so a genuinely stuck database is not hidden behind
/// minutes of waiting.
const RETRY_ATTEMPTS: usize = 6;
const RETRY_BASE_DELAY: std::time::Duration = std::time::Duration::from_millis(20);

impl RetryingSessionService {
    fn new(inner: impl SessionService + 'static) -> Self {
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Run `op`, retrying while SQLite says the database is busy.
    async fn retrying<T, F, Fut>(&self, mut op: F) -> adk_rust::Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = adk_rust::Result<T>>,
    {
        let mut delay = RETRY_BASE_DELAY;

        for attempt in 1..=RETRY_ATTEMPTS {
            match op().await {
                Err(e) if attempt < RETRY_ATTEMPTS && is_locked(&e) => {
                    tracing::debug!(
                        attempt,
                        "session store busy, retrying in {}ms",
                        delay.as_millis()
                    );
                    tokio::time::sleep(delay + jitter(delay)).await;
                    delay *= 2;
                }
                other => return other,
            }
        }

        unreachable!("the loop returns on its final attempt")
    }
}

/// Whether an error is SQLite saying "come back later" rather than "no".
fn is_locked(error: &adk_rust::AdkError) -> bool {
    let text = error.to_string();
    text.contains("database is locked")
        || text.contains("database table is locked")
        || text.contains("(code: 5)")
        || text.contains("(code: 6)")
}

/// Up to a full delay of extra wait, so two processes that collided do not
/// simply collide again one delay later.
fn jitter(delay: std::time::Duration) -> std::time::Duration {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let span = delay.as_millis().max(1) as u64;
    std::time::Duration::from_millis(nanos % span)
}

#[adk_rust::async_trait]
impl SessionService for RetryingSessionService {
    async fn create(&self, req: CreateRequest) -> adk_rust::Result<Box<dyn Session>> {
        self.retrying(|| self.inner.create(req.clone())).await
    }

    async fn get(&self, req: GetRequest) -> adk_rust::Result<Box<dyn Session>> {
        self.retrying(|| self.inner.get(req.clone())).await
    }

    async fn list(&self, req: ListRequest) -> adk_rust::Result<Vec<Box<dyn Session>>> {
        self.retrying(|| self.inner.list(req.clone())).await
    }

    async fn delete(&self, req: DeleteRequest) -> adk_rust::Result<()> {
        self.retrying(|| self.inner.delete(req.clone())).await
    }

    async fn append_event(&self, session_id: &str, event: Event) -> adk_rust::Result<()> {
        // The hot one: every event of every turn is a write, so this is where
        // two agents in the same second actually meet.
        self.retrying(|| self.inner.append_event(session_id, event.clone()))
            .await
    }
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
    ///
    /// The pool is built here rather than through
    /// `SqliteSessionService::new`, which calls `SqlitePool::connect` with
    /// defaults: rollback journal and **no busy timeout**. One database is
    /// shared by every momo-fetch process on the machine, and a team starts
    /// three at once — under the default settings the second writer to arrive
    /// does not wait, it fails instantly with `database is locked (code: 5)`,
    /// which killed a worker mid-turn before it had said anything. WAL lets
    /// readers run while one writer holds the lock, and the busy timeout makes
    /// writers queue for it instead of giving up.
    pub async fn new(db_path: &Path) -> anyhow::Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let options = SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            // Applied per connection, so it has to live on the options rather
            // than be issued once after connecting.
            .busy_timeout(BUSY_TIMEOUT)
            // `SqliteSessionService::new` issued this as a PRAGMA; `from_pool`
            // documents it as the caller's job.
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(MAX_CONNECTIONS)
            .acquire_timeout(ACQUIRE_TIMEOUT)
            .connect_with(options)
            .await?;

        let service = SqliteSessionService::from_pool(pool);

        // Run migrations (idempotent — safe on both fresh and existing DBs).
        // Concurrent starts serialize on the busy timeout above.
        service.migrate().await?;

        Ok(Self {
            service: Arc::new(RetryingSessionService::new(service)),
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

#[cfg(test)]
mod retry_tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A session store that reports the database busy a fixed number of times
    /// before working, so the retry loop can be tested without two processes
    /// and a real lock.
    struct Flaky {
        failures_left: Mutex<usize>,
        calls: AtomicUsize,
    }

    /// A cloneable handle, so the test can still read the call count after the
    /// service has taken ownership.
    #[derive(Clone)]
    struct FlakyHandle(Arc<Flaky>);

    impl FlakyHandle {
        fn new(failures: usize) -> Self {
            Self(Arc::new(Flaky {
                failures_left: Mutex::new(failures),
                calls: AtomicUsize::new(0),
            }))
        }

        fn calls(&self) -> usize {
            self.0.calls.load(Ordering::SeqCst)
        }
    }

    #[adk_rust::async_trait]
    impl SessionService for FlakyHandle {
        async fn create(&self, _req: CreateRequest) -> adk_rust::Result<Box<dyn Session>> {
            unimplemented!("delete is the method under test")
        }

        async fn get(&self, _req: GetRequest) -> adk_rust::Result<Box<dyn Session>> {
            unimplemented!("delete is the method under test")
        }

        async fn list(&self, _req: ListRequest) -> adk_rust::Result<Vec<Box<dyn Session>>> {
            unimplemented!("delete is the method under test")
        }

        async fn delete(&self, _req: DeleteRequest) -> adk_rust::Result<()> {
            self.0.calls.fetch_add(1, Ordering::SeqCst);

            let mut left = self.0.failures_left.lock().unwrap();
            if *left > 0 {
                *left -= 1;
                return Err(adk_rust::AdkError::session(
                    "insert failed: error returned from database: (code: 5) database is locked",
                ));
            }
            Ok(())
        }

        async fn append_event(&self, _session_id: &str, _event: Event) -> adk_rust::Result<()> {
            Ok(())
        }
    }

    fn delete_request() -> DeleteRequest {
        DeleteRequest {
            app_name: APP_NAME.to_string(),
            user_id: DEFAULT_USER.to_string(),
            session_id: "s1".to_string(),
        }
    }

    #[test]
    fn locked_errors_are_recognised() {
        assert!(is_locked(&adk_rust::AdkError::session(
            "insert failed: error returned from database: (code: 5) database is locked"
        )));
        assert!(is_locked(&adk_rust::AdkError::session(
            "database table is locked"
        )));

        // Anything else is a real failure and must surface on the first try.
        assert!(!is_locked(&adk_rust::AdkError::session("no such table: sessions")));
        assert!(!is_locked(&adk_rust::AdkError::session("UNIQUE constraint failed")));
    }

    #[tokio::test]
    async fn a_busy_database_is_retried_until_it_yields() {
        let flaky = FlakyHandle::new(3);
        let service = RetryingSessionService::new(flaky.clone());

        service.delete(delete_request()).await.unwrap();

        // Three refusals, then the one that worked.
        assert_eq!(flaky.calls(), 4);
    }

    #[tokio::test]
    async fn a_database_that_stays_busy_still_reports_the_error() {
        // Retrying forever would turn a wedged database into a hang, which is
        // worse than the error it replaced.
        let flaky = FlakyHandle::new(RETRY_ATTEMPTS + 1);
        let service = RetryingSessionService::new(flaky.clone());

        let err = service.delete(delete_request()).await.unwrap_err();

        assert!(err.to_string().contains("database is locked"), "{err}");
        assert_eq!(flaky.calls(), RETRY_ATTEMPTS);
    }
}

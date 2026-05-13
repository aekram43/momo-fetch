use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use adk_session::{
    CreateRequest, DeleteRequest, GetRequest, ListRequest, Session, SessionService,
    SqliteSessionService,
};

const APP_NAME: &str = "momo-fetch";
const DEFAULT_USER: &str = "default-user";

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
                event_count: s.events().len(),
            })
            .collect();
        // Most recently updated first
        infos.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(infos)
    }

    /// Delete a session by ID.
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
    pub event_count: usize,
}

impl std::fmt::Display for SessionInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let time = self.updated_at.format("%Y-%m-%d %H:%M");
        write!(f, "{}  ({} events, updated {})", self.id, self.event_count, time)
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
            event_count: 5,
        };
        let display = format!("{info}");
        assert!(display.contains("abc-123"));
        assert!(display.contains("5 events"));
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

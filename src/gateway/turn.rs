//! Turn admission control and the tool-approval handshake.
//!
//! The harness is a single mutable object with one `current_session_id`, one
//! runner and one provider. Turn concurrency is therefore **process-wide**, not
//! per session: a second in-flight turn would interleave on shared state. This
//! module admits one turn at a time and hands out an RAII lease.
//!
//! It also brokers tool approvals. adk surfaces a confirmation request by
//! *ending* the event stream with `actions.tool_confirmation` set; the decision
//! is then applied by starting a new turn. The gateway hides that seam behind a
//! single SSE response, and this registry is where the streaming task parks
//! while it waits for the user's click.
//!
//! Approvals are keyed by `(turn_id, tool_name)` because that is what adk
//! actually records — `RunConfig::tool_confirmation_decisions` is keyed by tool
//! name. The `call_id` is carried for display and staleness checks only.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::oneshot;

/// A turn currently occupying the harness.
#[derive(Debug, Clone)]
pub struct TurnState {
    pub turn_id: String,
    pub session_id: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub interrupted: bool,
}

/// Why an approve/deny call could not be applied.
#[derive(Debug, PartialEq, Eq)]
pub enum ApprovalError {
    /// No pending approval for this `(turn_id, tool_name)`.
    NotPending,
    /// The `call_id` doesn't match the pending request — a stale click.
    StaleCallId,
}

#[derive(Default)]
struct Inner {
    active: Option<TurnState>,
    /// Pending approvals keyed by `(turn_id, tool_name)`.
    approvals: HashMap<(String, String), Pending>,
}

struct Pending {
    call_id: Option<String>,
    tx: oneshot::Sender<bool>,
}

/// Process-wide turn admission + approval broker.
#[derive(Clone, Default)]
pub struct TurnRegistry {
    inner: Arc<Mutex<Inner>>,
}

impl TurnRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Try to claim the harness for a new turn.
    ///
    /// On success returns a lease that releases the claim when dropped — which
    /// includes the client disconnecting mid-turn, since dropping the SSE body
    /// drops the task that owns the lease.
    pub fn try_begin(&self, session_id: &str) -> Result<TurnLease, TurnState> {
        let mut inner = self.lock();
        if let Some(active) = &inner.active {
            return Err(active.clone());
        }
        let turn_id = format!("t_{}", uuid::Uuid::new_v4().simple());
        let state = TurnState {
            turn_id: turn_id.clone(),
            session_id: session_id.to_string(),
            started_at: chrono::Utc::now(),
            interrupted: false,
        };
        inner.active = Some(state);
        Ok(TurnLease {
            registry: self.clone(),
            turn_id,
        })
    }

    /// The turn currently holding the harness, if any.
    pub fn active(&self) -> Option<TurnState> {
        self.lock().active.clone()
    }

    /// Park until the user decides on `tool_name`.
    ///
    /// The returned receiver resolves to the decision. A dropped sender (turn
    /// released, interrupt, client gone) surfaces as `Err`, which callers treat
    /// as a denial — failing closed is the only safe default for a tool the
    /// user never actually approved.
    pub fn register_approval(
        &self,
        turn_id: &str,
        tool_name: &str,
        call_id: Option<String>,
    ) -> oneshot::Receiver<bool> {
        let (tx, rx) = oneshot::channel();
        self.lock().approvals.insert(
            (turn_id.to_string(), tool_name.to_string()),
            Pending { call_id, tx },
        );
        rx
    }

    /// Apply a user decision to a pending approval.
    pub fn resolve(
        &self,
        turn_id: &str,
        tool_name: &str,
        call_id: Option<&str>,
        approved: bool,
    ) -> Result<(), ApprovalError> {
        let key = (turn_id.to_string(), tool_name.to_string());
        let pending = {
            let mut inner = self.lock();
            match inner.approvals.get(&key) {
                None => return Err(ApprovalError::NotPending),
                Some(p) => {
                    // Only enforce when both sides carry an id; adk omits the
                    // call id for providers that don't emit one.
                    if let (Some(want), Some(got)) = (p.call_id.as_deref(), call_id) {
                        if want != got {
                            return Err(ApprovalError::StaleCallId);
                        }
                    }
                }
            }
            inner.approvals.remove(&key)
        };
        match pending {
            Some(p) => {
                // Receiver gone means the turn already moved on; the decision
                // is simply obsolete, not an error worth surfacing.
                let _ = p.tx.send(approved);
                Ok(())
            }
            None => Err(ApprovalError::NotPending),
        }
    }

    /// Deny every approval pending for a turn. Used by interrupt so a turn
    /// parked on a confirmation unblocks instead of waiting out the timeout.
    pub fn deny_all(&self, turn_id: &str) {
        let drained: Vec<Pending> = {
            let mut inner = self.lock();
            let keys: Vec<_> = inner
                .approvals
                .keys()
                .filter(|(t, _)| t == turn_id)
                .cloned()
                .collect();
            keys.into_iter()
                .filter_map(|k| inner.approvals.remove(&k))
                .collect()
        };
        for p in drained {
            let _ = p.tx.send(false);
        }
    }

    /// Flag the active turn as interrupted so it can report the right
    /// `stop_reason`. Returns false if the turn id isn't the active one.
    pub fn mark_interrupted(&self, turn_id: &str) -> bool {
        let mut inner = self.lock();
        match &mut inner.active {
            Some(active) if active.turn_id == turn_id => {
                active.interrupted = true;
                true
            }
            _ => false,
        }
    }

    fn is_interrupted(&self, turn_id: &str) -> bool {
        self.lock()
            .active
            .as_ref()
            .is_some_and(|a| a.turn_id == turn_id && a.interrupted)
    }

    /// Release a turn and drop anything still parked on it.
    fn release(&self, turn_id: &str) {
        let mut inner = self.lock();
        if inner
            .active
            .as_ref()
            .is_some_and(|a| a.turn_id == turn_id)
        {
            inner.active = None;
        }
        // Dropping the senders wakes any waiter with Err → denial.
        inner.approvals.retain(|(t, _), _| t != turn_id);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // A poisoned lock here would mean a panic inside one of these tiny
        // critical sections; the data stays consistent, so recover rather than
        // wedge every subsequent turn.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// RAII claim on the harness for one turn. Releases on drop.
pub struct TurnLease {
    registry: TurnRegistry,
    turn_id: String,
}

impl TurnLease {
    pub fn turn_id(&self) -> &str {
        &self.turn_id
    }

    pub fn is_interrupted(&self) -> bool {
        self.registry.is_interrupted(&self.turn_id)
    }
}

impl Drop for TurnLease {
    fn drop(&mut self) {
        self.registry.release(&self.turn_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_turn_is_rejected_while_one_is_active() {
        let reg = TurnRegistry::new();
        let lease = reg.try_begin("s1").expect("first turn admitted");
        let Err(busy) = reg.try_begin("s2") else {
            panic!("second turn must be refused while one is active");
        };
        assert_eq!(busy.turn_id, lease.turn_id());
        assert_eq!(busy.session_id, "s1");
    }

    #[test]
    fn dropping_the_lease_frees_the_harness() {
        let reg = TurnRegistry::new();
        {
            let _lease = reg.try_begin("s1").unwrap();
            assert!(reg.active().is_some());
        }
        assert!(reg.active().is_none());
        reg.try_begin("s2").expect("next turn admitted after release");
    }

    #[tokio::test]
    async fn approval_resolves_the_waiter() {
        let reg = TurnRegistry::new();
        let lease = reg.try_begin("s1").unwrap();
        let rx = reg.register_approval(lease.turn_id(), "shell_exec", Some("call_1".into()));
        reg.resolve(lease.turn_id(), "shell_exec", Some("call_1"), true)
            .expect("resolve succeeds");
        assert!(rx.await.unwrap());
    }

    #[tokio::test]
    async fn stale_call_id_is_rejected() {
        let reg = TurnRegistry::new();
        let lease = reg.try_begin("s1").unwrap();
        let _rx = reg.register_approval(lease.turn_id(), "shell_exec", Some("call_1".into()));
        assert_eq!(
            reg.resolve(lease.turn_id(), "shell_exec", Some("call_OLD"), true),
            Err(ApprovalError::StaleCallId)
        );
        // The pending approval survives a stale click.
        assert!(reg
            .resolve(lease.turn_id(), "shell_exec", Some("call_1"), true)
            .is_ok());
    }

    #[tokio::test]
    async fn resolving_an_unknown_approval_is_an_error() {
        let reg = TurnRegistry::new();
        let lease = reg.try_begin("s1").unwrap();
        assert_eq!(
            reg.resolve(lease.turn_id(), "never_asked", None, true),
            Err(ApprovalError::NotPending)
        );
    }

    #[tokio::test]
    async fn dropping_the_lease_denies_pending_approvals() {
        let reg = TurnRegistry::new();
        let rx = {
            let lease = reg.try_begin("s1").unwrap();
            reg.register_approval(lease.turn_id(), "shell_exec", None)
        };
        // Sender dropped with the lease: the waiter fails closed rather than
        // hanging until the timeout.
        assert!(rx.await.is_err());
    }

    #[tokio::test]
    async fn interrupt_denies_a_parked_approval() {
        let reg = TurnRegistry::new();
        let lease = reg.try_begin("s1").unwrap();
        let rx = reg.register_approval(lease.turn_id(), "shell_exec", None);
        assert!(reg.mark_interrupted(lease.turn_id()));
        reg.deny_all(lease.turn_id());
        assert!(!rx.await.unwrap());
        assert!(lease.is_interrupted());
    }
}

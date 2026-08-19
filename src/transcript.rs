//! Persist what the agent actually said.
//!
//! **The gap.** In streaming mode adk emits one event per model chunk, all
//! sharing a single event id and each carrying only its own delta; the complete
//! text lives in an accumulator inside `LlmAgent` that is only turned into an
//! event when streaming to the client is *off* (`adk-agent`'s
//! `if !should_stream_to_client` branch). The runner, meanwhile, persists only
//! non-partial events — and the one non-partial event a text reply produces is
//! the provider's terminal chunk, which carries a finish reason and usage but
//! `content: null`. So every word the agent says is streamed to the UI and then
//! dropped.
//!
//! **Why it is not just cosmetic.** `Runner::run` reloads the session from the
//! session service at the top of *every* turn. A history with no assistant
//! replies is not only what a reopened session renders — it is what the model
//! is given next turn. It sees its own tool calls and their results, and no
//! trace of anything it told the user.
//!
//! This wraps a turn's event stream, reassembles the deltas, and writes one
//! event per LLM call. Best-effort throughout: a transcript that cannot be
//! written is worth a warning, never a failed turn.

use std::sync::Arc;

use adk_rust::{Content, Event, EventStream, Part};
use adk_session::SessionService;
use futures::StreamExt;

/// Text reassembled for one LLM call, waiting to be written.
struct Pending {
    /// The streamed event id every chunk of this call shares.
    event_id: String,
    invocation_id: String,
    author: String,
    /// The first chunk's time, not the flush time — see [`flush`].
    timestamp: chrono::DateTime<chrono::Utc>,
    text: String,
}

/// Wrap a turn's events so the agent's replies reach the session store.
///
/// Passes every event through untouched; the only effect is the extra write.
pub fn recording_replies(
    stream: EventStream,
    service: Arc<dyn SessionService>,
    session_id: String,
) -> EventStream {
    Box::pin(async_stream::stream! {
        let mut stream = stream;
        let mut pending: Option<Pending> = None;
        let mut written = 0usize;

        while let Some(item) = stream.next().await {
            if let Ok(event) = &item {
                // A new event id means a new LLM call: whatever was said in the
                // previous one is complete.
                if pending.as_ref().is_some_and(|p| p.event_id != event.id) {
                    flush(&service, &session_id, pending.take(), &mut written).await;
                }

                let text = text_of(event);
                if event.llm_response.partial {
                    if !text.is_empty() {
                        pending
                            .get_or_insert_with(|| Pending {
                                event_id: event.id.clone(),
                                invocation_id: event.invocation_id.clone(),
                                author: event.author.clone(),
                                timestamp: event.timestamp,
                                text: String::new(),
                            })
                            .text
                            .push_str(&text);
                    }
                } else {
                    // A non-partial event carrying text is one the runner
                    // persists itself — a non-streaming provider, or adk's own
                    // "confirmation required" notice. Writing our copy too
                    // would double it in both the transcript and the context.
                    if !text.is_empty() {
                        pending = None;
                    }

                    // **Write before yielding, not at end of stream.** Consumers
                    // stop polling the moment they see a final event — the SSE
                    // handler returns on `Event::is_final_response`, which drops
                    // this generator where it stands. Anything deferred past the
                    // yield below simply never runs.
                    flush(&service, &session_id, pending.take(), &mut written).await;
                }
            }

            yield item;
        }

        // A stream can also just end — an interrupt, or a provider that never
        // sends a terminal chunk.
        flush(&service, &session_id, pending.take(), &mut written).await;
    })
}

/// Every `Text` part of an event, concatenated.
///
/// Thinking traces are a different part and stay out of the transcript: they
/// are not what the agent said, and replaying them to the model as if they were
/// would change what it does next.
fn text_of(event: &Event) -> String {
    let Some(content) = event.content() else {
        return String::new();
    };
    content
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// Write one reassembled reply.
///
/// **The timestamp is the first chunk's, not now's.** Events are read back
/// `ORDER BY timestamp`, and a reply that precedes a tool call is streamed
/// before it but can only be flushed after — stamping it at flush time would
/// reorder the conversation around its own tool calls.
///
/// The id is derived from the streamed one and a per-stream counter: events are
/// inserted, not upserted, so two replies within one LLM call — text, a tool
/// call, then more text — must not land on the same primary key.
async fn flush(
    service: &Arc<dyn SessionService>,
    session_id: &str,
    pending: Option<Pending>,
    written: &mut usize,
) {
    let Some(pending) = pending else { return };
    if pending.text.trim().is_empty() {
        return;
    }
    *written += 1;

    let mut event = Event::with_id(
        format!("{}_text_{}", pending.event_id, written),
        pending.invocation_id.clone(),
    );
    event.author = pending.author.clone();
    event.timestamp = pending.timestamp;
    event.set_content(Content::new("model").with_text(&pending.text));

    if let Err(e) = service.append_event(session_id, event).await {
        tracing::warn!(
            session_id,
            event_id = pending.event_id,
            "could not persist the agent's reply: {e}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionManager;

    /// A streamed chunk: same id, `partial`, carrying its delta.
    fn chunk(id: &str, text: &str) -> Event {
        let mut event = Event::with_id(id, "inv-1");
        event.author = "momo-fetch".to_string();
        event.llm_response.partial = true;
        event.set_content(Content::new("model").with_text(text));
        event
    }

    /// The provider's terminal chunk: non-partial, no content.
    fn terminal(id: &str) -> Event {
        let mut event = Event::with_id(id, "inv-1");
        event.author = "momo-fetch".to_string();
        event.llm_response.turn_complete = true;
        event
    }

    fn stream_of(events: Vec<Event>) -> EventStream {
        Box::pin(futures::stream::iter(events.into_iter().map(Ok)))
    }

    /// Drain the wrapper over `events` and return what the session now holds.
    async fn persisted(events: Vec<Event>) -> Vec<Event> {
        let mgr = SessionManager::new_in_memory();
        mgr.create_session(Some("t")).await.unwrap();

        let mut out = recording_replies(stream_of(events), mgr.service(), "t".to_string());
        while out.next().await.is_some() {}

        mgr.get_session("t").await.unwrap().events().all()
    }

    #[tokio::test]
    async fn deltas_are_reassembled_into_one_reply() {
        let events = persisted(vec![
            chunk("llm-1", "สวัสดี"),
            chunk("llm-1", "ครับ"),
            terminal("llm-1"),
        ])
        .await;

        assert_eq!(events.len(), 1, "one reply, not one event per chunk");
        assert_eq!(text_of(&events[0]), "สวัสดีครับ");
        assert_eq!(events[0].content().unwrap().role, "model");
        assert!(!events[0].llm_response.partial, "a reply is not a chunk");
    }

    #[tokio::test]
    async fn a_reply_the_runner_persists_itself_is_not_duplicated() {
        // Non-streaming providers — and adk's own confirmation notice — deliver
        // the whole reply in one non-partial event, which the runner writes.
        let mut whole = chunk("llm-1", "approved");
        whole.llm_response.partial = false;

        assert!(persisted(vec![whole]).await.is_empty());
    }

    #[tokio::test]
    async fn each_llm_call_in_a_turn_gets_its_own_reply() {
        let events = persisted(vec![
            chunk("llm-1", "checking"),
            terminal("llm-1"),
            chunk("llm-2", "all good"),
            terminal("llm-2"),
        ])
        .await;

        let texts: Vec<String> = events.iter().map(text_of).collect();
        assert_eq!(texts, vec!["checking", "all good"]);
    }

    #[tokio::test]
    async fn a_reply_keeps_the_time_it_was_spoken() {
        // It is flushed only once the *next* call starts, so stamping it then
        // would sort it after tool calls it actually preceded.
        let first = chunk("llm-1", "one moment");
        let spoken_at = first.timestamp;

        let events = persisted(vec![first, terminal("llm-1")]).await;

        assert_eq!(events[0].timestamp, spoken_at);
    }

    #[tokio::test]
    async fn a_turn_that_only_calls_tools_writes_nothing() {
        let mut call = Event::with_id("llm-1", "inv-1");
        call.author = "momo-fetch".to_string();
        let mut content = Content::new("model");
        content.parts.push(Part::FunctionCall {
            name: "shell_exec".to_string(),
            args: serde_json::json!({}),
            id: Some("call-1".to_string()),
            thought_signature: None,
        });
        call.set_content(content);

        assert!(persisted(vec![call, terminal("llm-1")]).await.is_empty());
    }

    #[tokio::test]
    async fn whitespace_is_not_a_reply() {
        let events = persisted(vec![chunk("llm-1", "  \n "), terminal("llm-1")]).await;
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn a_reply_survives_a_consumer_that_stops_at_the_final_event() {
        // The regression that made the first cut of this useless: the SSE
        // handler returns on `is_final_response()`, which drops the stream —
        // so a reply written only after the loop is never written at all.
        let mgr = SessionManager::new_in_memory();
        mgr.create_session(Some("t")).await.unwrap();

        let input = vec![chunk("llm-1", "พร้อม"), terminal("llm-1")];
        let mut out = recording_replies(stream_of(input), mgr.service(), "t".to_string());

        out.next().await.unwrap().unwrap();
        let last = out.next().await.unwrap().unwrap();
        assert!(last.is_final_response(), "the event a consumer stops on");
        drop(out);

        let events = mgr.get_session("t").await.unwrap().events().all();
        assert_eq!(events.len(), 1);
        assert_eq!(text_of(&events[0]), "พร้อม");
    }

    #[tokio::test]
    async fn text_on_both_sides_of_a_tool_call_stays_two_replies() {
        // Same LLM call, so the same streamed id — the written ids must still
        // differ, or the second insert collides with the first.
        let mut call = Event::with_id("llm-1", "inv-1");
        call.author = "momo-fetch".to_string();
        let mut content = Content::new("model");
        content.parts.push(Part::FunctionCall {
            name: "shell_exec".to_string(),
            args: serde_json::json!({}),
            id: Some("call-1".to_string()),
            thought_signature: None,
        });
        call.set_content(content);

        let events = persisted(vec![
            chunk("llm-1", "checking"),
            call,
            chunk("llm-1", "all good"),
            terminal("llm-1"),
        ])
        .await;

        let texts: Vec<String> = events.iter().map(text_of).collect();
        assert_eq!(texts, vec!["checking", "all good"]);
    }

    #[tokio::test]
    async fn every_event_still_reaches_the_consumer() {
        // The wrapper is transparent: the REPL and the SSE handler must see the
        // same stream they saw before.
        let mgr = SessionManager::new_in_memory();
        mgr.create_session(Some("t")).await.unwrap();

        let input = vec![chunk("llm-1", "hi"), terminal("llm-1")];
        let mut out = recording_replies(stream_of(input), mgr.service(), "t".to_string());

        let mut seen = 0;
        while let Some(item) = out.next().await {
            assert!(item.is_ok());
            seen += 1;
        }
        assert_eq!(seen, 2);
    }
}

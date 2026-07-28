//! Authentication middleware and rate limiting.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use super::types::ErrorResponse;
use crate::gateway::{ApiKeyMeta, GatewayState};

/// Tracks request counts per API key.
#[derive(Debug)]
struct KeyUsage {
    /// Timestamp of the current minute window start.
    minute_start: Instant,
    /// Requests in the current minute.
    minute_count: u32,
    /// Timestamp of the current day window start.
    day_start: Instant,
    /// Requests in the current day.
    day_count: u32,
}

impl KeyUsage {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            minute_start: now,
            minute_count: 0,
            day_start: now,
            day_count: 0,
        }
    }

    fn record(&mut self) -> RateLimitResult {
        let now = Instant::now();

        // Reset minute counter if window expired
        if now.duration_since(self.minute_start).as_secs() >= 60 {
            self.minute_start = now;
            self.minute_count = 0;
        }
        self.minute_count += 1;

        // Reset day counter if window expired
        if now.duration_since(self.day_start).as_secs() >= 86400 {
            self.day_start = now;
            self.day_count = 0;
        }
        self.day_count += 1;

        RateLimitResult {
            minute_count: self.minute_count,
            day_count: self.day_count,
        }
    }
}

struct RateLimitResult {
    minute_count: u32,
    day_count: u32,
}

/// Rate limiter for API keys.
pub struct RateLimiter {
    auth_enabled: bool,
    keys: HashMap<String, ApiKeyMeta>,
    usage: Mutex<HashMap<String, KeyUsage>>,
}

impl RateLimiter {
    pub fn new(auth_enabled: bool, keys: &HashMap<String, ApiKeyMeta>) -> Self {
        Self {
            auth_enabled,
            keys: keys.clone(),
            usage: Mutex::new(HashMap::new()),
        }
    }

    /// Validate a token and check rate limits.
    /// Returns Ok(()) on success, or a JSON error response body on failure.
    pub fn check(&self, token: Option<&str>) -> Result<(), Response> {
        if !self.auth_enabled {
            return Ok(());
        }

        let token = match token {
            Some(t) if t.starts_with("Bearer ") => &t[7..],
            Some(t) if !t.is_empty() => t,
            _ => {
                return Err((
                    StatusCode::UNAUTHORIZED,
                    axum::Json(ErrorResponse::unauthorized()),
                )
                    .into_response());
            }
        };

        let meta = match self.keys.get(token) {
            Some(m) => m,
            None => {
                return Err((
                    StatusCode::UNAUTHORIZED,
                    axum::Json(ErrorResponse::unauthorized()),
                )
                    .into_response());
            }
        };

        // Check rate limits
        let mut usage_map = self.usage.lock().unwrap_or_else(|e| e.into_inner());
        let usage = usage_map.entry(token.to_string()).or_insert_with(KeyUsage::new);
        let result = usage.record();

        if meta.rate_limit > 0 && result.minute_count > meta.rate_limit {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                axum::Json(ErrorResponse::rate_limit()),
            )
                .into_response());
        }

        if meta.daily_quota > 0 && result.day_count > meta.daily_quota {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                axum::Json(ErrorResponse::rate_limit()),
            )
                .into_response());
        }

        Ok(())
    }
}

/// Axum middleware that checks Bearer token auth.
pub async fn auth_middleware(
    State(state): State<GatewayState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let token = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    match state.rate_limiter.check(token) {
        Ok(()) => next.run(req).await,
        Err(resp) => resp,
    }
}

use crate::domain::ModelProfile;
use std::{
    collections::hash_map::DefaultHasher,
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub(crate) const RATE_LIMIT_RETRY_EXHAUSTED: &str = "服务商限流重试已耗尽";
pub(crate) const MAX_RATE_LIMIT_RETRIES: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RateLimitKind {
    Tpm,
    Rpm,
    Generic,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct RateLimitScope(String);

impl RateLimitScope {
    pub(crate) fn for_profile(profile: &ModelProfile) -> Self {
        Self(format!(
            "{}|{}|{}|{}",
            profile.id.trim(),
            profile.provider.trim().to_ascii_lowercase(),
            profile
                .base_url
                .trim()
                .trim_end_matches('/')
                .to_ascii_lowercase(),
            profile.model.trim().to_ascii_lowercase()
        ))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
struct RateLimitState {
    cooldown_until: Instant,
    consecutive_limits: usize,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RateLimitCoordinator {
    states: Arc<Mutex<HashMap<RateLimitScope, RateLimitState>>>,
}

impl RateLimitCoordinator {
    pub(crate) fn cooldown_delay(
        &self,
        scope: &RateLimitScope,
    ) -> Result<Option<Duration>, String> {
        let mut states = self.states.lock().map_err(|error| error.to_string())?;
        let Some(state) = states.get(scope) else {
            return Ok(None);
        };
        let now = Instant::now();
        if state.cooldown_until <= now {
            states.remove(scope);
            Ok(None)
        } else {
            Ok(Some(state.cooldown_until.duration_since(now)))
        }
    }

    pub(crate) fn record_rate_limit(
        &self,
        scope: &RateLimitScope,
        retry_after: Option<Duration>,
        attempt: usize,
    ) -> Result<Duration, String> {
        let delay = retry_after.unwrap_or_else(|| default_backoff_delay(scope, attempt));
        let mut states = self.states.lock().map_err(|error| error.to_string())?;
        let state = states
            .entry(scope.clone())
            .or_insert_with(|| RateLimitState {
                cooldown_until: Instant::now(),
                consecutive_limits: 0,
            });
        state.consecutive_limits = state.consecutive_limits.saturating_add(1);
        state.cooldown_until = Instant::now() + delay;
        Ok(delay)
    }

    pub(crate) fn record_success(&self, scope: &RateLimitScope) -> Result<(), String> {
        let mut states = self.states.lock().map_err(|error| error.to_string())?;
        states.remove(scope);
        Ok(())
    }

    pub(crate) fn effective_parallelism(
        &self,
        configured: usize,
        profiles: &[&ModelProfile],
    ) -> Result<usize, String> {
        let configured = normalize_parallelism_for_runtime(configured);
        let states = self.states.lock().map_err(|error| error.to_string())?;
        let max_consecutive = profiles
            .iter()
            .filter_map(|profile| states.get(&RateLimitScope::for_profile(profile)))
            .map(|state| state.consecutive_limits)
            .max()
            .unwrap_or(0);
        Ok(apply_temporary_parallelism_drop(
            configured,
            max_consecutive,
        ))
    }
}

pub(crate) fn is_rate_limit_retry_exhausted(error: &str) -> bool {
    error.contains(RATE_LIMIT_RETRY_EXHAUSTED)
}

pub(crate) fn classify_rate_limit(status: Option<u16>, body: &str) -> Option<RateLimitKind> {
    let body = body.to_ascii_lowercase();
    if status != Some(429) {
        return None;
    }
    if body.contains("tpm") || body.contains("tokens per minute") || body.contains("token") {
        Some(RateLimitKind::Tpm)
    } else if body.contains("rpm") || body.contains("requests per minute") {
        Some(RateLimitKind::Rpm)
    } else {
        Some(RateLimitKind::Generic)
    }
}

pub(crate) fn parse_retry_after(value: Option<&str>) -> Option<Duration> {
    let seconds = value?.trim().parse::<u64>().ok()?;
    Some(Duration::from_secs(seconds.clamp(1, 600)))
}

pub(crate) fn default_backoff_delay(scope: &RateLimitScope, attempt: usize) -> Duration {
    let base = match attempt {
        0 | 1 => 45,
        2 => 90,
        3 => 180,
        _ => 300,
    };
    Duration::from_secs(base + deterministic_jitter_seconds(scope, attempt))
}

fn deterministic_jitter_seconds(scope: &RateLimitScope, attempt: usize) -> u64 {
    let mut hasher = DefaultHasher::new();
    scope.as_str().hash(&mut hasher);
    attempt.hash(&mut hasher);
    hasher.finish() % 11
}

fn normalize_parallelism_for_runtime(value: usize) -> usize {
    match value {
        1 | 3 | 6 | 10 => value,
        _ => 6,
    }
}

pub(crate) fn apply_temporary_parallelism_drop(
    configured: usize,
    consecutive_limits: usize,
) -> usize {
    if consecutive_limits == 0 {
        return normalize_parallelism_for_runtime(configured);
    }
    match (
        normalize_parallelism_for_runtime(configured),
        consecutive_limits,
    ) {
        (10, 1) => 6,
        (10, 2) => 3,
        (10, _) => 1,
        (6, 1) => 3,
        (6, _) => 1,
        (3, _) => 1,
        (value, _) => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_rate_limit_messages() {
        assert_eq!(
            classify_rate_limit(Some(429), r#"{"message":"TPM limit reached"}"#),
            Some(RateLimitKind::Tpm)
        );
        assert_eq!(
            classify_rate_limit(Some(429), r#"RPM limit reached"#),
            Some(RateLimitKind::Rpm)
        );
        assert_eq!(
            classify_rate_limit(Some(429), "Too Many Requests"),
            Some(RateLimitKind::Generic)
        );
        assert_eq!(classify_rate_limit(Some(403), "rate limit"), None);
    }

    #[test]
    fn parses_retry_after_seconds() {
        assert_eq!(parse_retry_after(Some("12")), Some(Duration::from_secs(12)));
        assert_eq!(parse_retry_after(Some("not-a-number")), None);
    }

    #[test]
    fn drops_parallelism_temporarily_after_consecutive_limits() {
        assert_eq!(apply_temporary_parallelism_drop(10, 0), 10);
        assert_eq!(apply_temporary_parallelism_drop(10, 1), 6);
        assert_eq!(apply_temporary_parallelism_drop(10, 2), 3);
        assert_eq!(apply_temporary_parallelism_drop(10, 3), 1);
        assert_eq!(apply_temporary_parallelism_drop(6, 1), 3);
        assert_eq!(apply_temporary_parallelism_drop(6, 2), 1);
        assert_eq!(apply_temporary_parallelism_drop(3, 1), 1);
    }
}

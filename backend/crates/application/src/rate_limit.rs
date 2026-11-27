//! Per-principal request throttling port.
//!
//! Throttling is an application concern (roadmap B8): the inbound adapter asks the limiter
//! before running any use case for a verified principal. The port is synchronous because
//! decisions are expected to be cheap and local (ADR 016); a distributed limiter would get its
//! own asynchronous port.

use einkaufio_domain::UserId;

/// Outcome of a throttling check.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RateLimitDecision {
    Allowed,
    /// The principal exceeded its budget and may retry after the given number of seconds.
    Limited {
        retry_after_seconds: u64,
    },
}

/// The limiter could not decide; callers must fail closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RateLimitError {
    Unavailable,
}

pub trait RateLimiter: Send + Sync {
    /// Consume one request from `principal`'s budget.
    fn check(&self, principal: &UserId) -> Result<RateLimitDecision, RateLimitError>;
}

//! In-process token-bucket rate limiter (ADR 016).
//!
//! Buckets live in the memory of one Lambda instance, so the limit is best effort per
//! instance; reserved Lambda concurrency bounds the total. The number of tracked principals
//! is capped so the map cannot grow without bound; when the cap is reached and no idle bucket
//! can be evicted, new principals are limited (fail closed) rather than tracked.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use einkaufio_application::{RateLimitDecision, RateLimitError, RateLimiter, UserId};

#[derive(Clone, Copy, Debug)]
struct Bucket {
    tokens: f64,
    updated: Instant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TokenBucketConfigError {
    ZeroCapacity,
    ZeroRefill,
    ZeroTrackedPrincipals,
}

#[derive(Debug)]
pub struct TokenBucketRateLimiter {
    capacity: u32,
    refill_per_second: u32,
    max_tracked: usize,
    buckets: Mutex<HashMap<UserId, Bucket>>,
}

impl TokenBucketRateLimiter {
    /// Default budget: bursts of 60 requests, refilling 10 per second.
    pub const DEFAULT_CAPACITY: u32 = 60;
    pub const DEFAULT_REFILL_PER_SECOND: u32 = 10;
    pub const DEFAULT_MAX_TRACKED: usize = 10_000;

    pub fn try_new(
        capacity: u32,
        refill_per_second: u32,
        max_tracked: usize,
    ) -> Result<Self, TokenBucketConfigError> {
        if capacity == 0 {
            return Err(TokenBucketConfigError::ZeroCapacity);
        }
        if refill_per_second == 0 {
            return Err(TokenBucketConfigError::ZeroRefill);
        }
        if max_tracked == 0 {
            return Err(TokenBucketConfigError::ZeroTrackedPrincipals);
        }
        Ok(Self {
            capacity,
            refill_per_second,
            max_tracked,
            buckets: Mutex::new(HashMap::new()),
        })
    }

    pub fn with_defaults() -> Self {
        Self {
            capacity: Self::DEFAULT_CAPACITY,
            refill_per_second: Self::DEFAULT_REFILL_PER_SECOND,
            max_tracked: Self::DEFAULT_MAX_TRACKED,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    fn refilled(&self, bucket: Bucket, now: Instant) -> f64 {
        let elapsed = now.saturating_duration_since(bucket.updated).as_secs_f64();
        (bucket.tokens + elapsed * f64::from(self.refill_per_second)).min(f64::from(self.capacity))
    }

    fn retry_after(&self, tokens: f64) -> u64 {
        let missing = (1.0 - tokens).max(0.0);
        let seconds = Duration::from_secs_f64(missing / f64::from(self.refill_per_second));
        seconds
            .as_secs()
            .saturating_add(u64::from(seconds.subsec_nanos() > 0))
            .max(1)
    }

    pub(crate) fn check_at(
        &self,
        principal: &UserId,
        now: Instant,
    ) -> Result<RateLimitDecision, RateLimitError> {
        let mut buckets = self
            .buckets
            .lock()
            .map_err(|_| RateLimitError::Unavailable)?;
        if !buckets.contains_key(principal) && buckets.len() >= self.max_tracked {
            let capacity = f64::from(self.capacity);
            let snapshot: Vec<(UserId, Bucket)> =
                buckets.iter().map(|(k, v)| (k.clone(), *v)).collect();
            for (key, bucket) in snapshot {
                if self.refilled(bucket, now) >= capacity {
                    buckets.remove(&key);
                }
            }
            if buckets.len() >= self.max_tracked {
                return Ok(RateLimitDecision::Limited {
                    retry_after_seconds: self.retry_after(0.0),
                });
            }
        }
        let bucket = buckets.entry(principal.clone()).or_insert(Bucket {
            tokens: f64::from(self.capacity),
            updated: now,
        });
        let tokens = self.refilled(*bucket, now);
        bucket.updated = now;
        if tokens >= 1.0 {
            bucket.tokens = tokens - 1.0;
            Ok(RateLimitDecision::Allowed)
        } else {
            bucket.tokens = tokens;
            Ok(RateLimitDecision::Limited {
                retry_after_seconds: self.retry_after(tokens),
            })
        }
    }
}

impl RateLimiter for TokenBucketRateLimiter {
    fn check(&self, principal: &UserId) -> Result<RateLimitDecision, RateLimitError> {
        self.check_at(principal, Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: &str) -> UserId {
        UserId::try_new(id).unwrap()
    }

    #[test]
    fn rejects_zero_configuration() {
        assert_eq!(
            TokenBucketRateLimiter::try_new(0, 1, 1).unwrap_err(),
            TokenBucketConfigError::ZeroCapacity
        );
        assert_eq!(
            TokenBucketRateLimiter::try_new(1, 0, 1).unwrap_err(),
            TokenBucketConfigError::ZeroRefill
        );
        assert_eq!(
            TokenBucketRateLimiter::try_new(1, 1, 0).unwrap_err(),
            TokenBucketConfigError::ZeroTrackedPrincipals
        );
    }

    #[test]
    fn allows_burst_then_limits_and_refills() {
        let limiter = TokenBucketRateLimiter::try_new(2, 1, 10).unwrap();
        let start = Instant::now();
        let alice = user("alice");
        assert_eq!(
            limiter.check_at(&alice, start),
            Ok(RateLimitDecision::Allowed)
        );
        assert_eq!(
            limiter.check_at(&alice, start),
            Ok(RateLimitDecision::Allowed)
        );
        assert_eq!(
            limiter.check_at(&alice, start),
            Ok(RateLimitDecision::Limited {
                retry_after_seconds: 1
            })
        );
        let later = start + Duration::from_secs(1);
        assert_eq!(
            limiter.check_at(&alice, later),
            Ok(RateLimitDecision::Allowed)
        );
    }

    #[test]
    fn budgets_are_per_principal() {
        let limiter = TokenBucketRateLimiter::try_new(1, 1, 10).unwrap();
        let now = Instant::now();
        assert_eq!(
            limiter.check_at(&user("a"), now),
            Ok(RateLimitDecision::Allowed)
        );
        assert!(matches!(
            limiter.check_at(&user("a"), now),
            Ok(RateLimitDecision::Limited { .. })
        ));
        assert_eq!(
            limiter.check_at(&user("b"), now),
            Ok(RateLimitDecision::Allowed)
        );
    }

    #[test]
    fn tracked_principals_are_bounded_and_idle_buckets_evicted() {
        let limiter = TokenBucketRateLimiter::try_new(1, 1, 1).unwrap();
        let now = Instant::now();
        assert_eq!(
            limiter.check_at(&user("a"), now),
            Ok(RateLimitDecision::Allowed)
        );
        assert!(matches!(
            limiter.check_at(&user("b"), now),
            Ok(RateLimitDecision::Limited { .. })
        ));
        let later = now + Duration::from_secs(5);
        assert_eq!(
            limiter.check_at(&user("b"), later),
            Ok(RateLimitDecision::Allowed)
        );
    }

    #[test]
    fn concurrent_checks_never_exceed_capacity() {
        let limiter = std::sync::Arc::new(TokenBucketRateLimiter::try_new(50, 1, 10).unwrap());
        let now = Instant::now();
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let limiter = limiter.clone();
                std::thread::spawn(move || {
                    (0..20)
                        .filter(|_| {
                            limiter.check_at(&user("a"), now) == Ok(RateLimitDecision::Allowed)
                        })
                        .count()
                })
            })
            .collect();
        let allowed: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();
        assert_eq!(allowed, 50);
    }
}

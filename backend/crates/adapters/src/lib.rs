//! Outbound adapters implementing application-owned ports.
//!
//! Test-only adapters and reusable contracts are available under `cfg(test)` or the
//! explicit `test-support` feature; they are not part of the production library API.

#[cfg(feature = "dynamodb")]
pub mod dynamodb;
#[cfg(any(test, feature = "test-support"))]
pub mod in_memory;
pub mod invitation_tokens;
#[cfg(any(test, feature = "test-support"))]
pub mod repository_contracts;
pub mod token_bucket;

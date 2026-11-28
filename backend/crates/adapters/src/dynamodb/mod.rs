//! DynamoDB single-table adapter for every repository port (roadmap A5/B1, ADR 003/015/017/019).
//!
//! Enabled by the `dynamodb` cargo feature. DynamoDB types, keys and expressions stay inside
//! this module; the ports only see domain values and port errors (ADR 009).
//!
//! # Concurrency model
//!
//! Every mutation is an optimistic attempt: consistent reads → a decision taken with the
//! domain model → one `TransactWriteItems` whose conditions assert that every record the
//! decision depended on is unchanged (aggregate versions, lifecycle state, membership and
//! ownership, counters). If DynamoDB cancels the transaction, nothing was written and the
//! next attempt re-reads the persisted records, so the outcome (`NotFound`, `Forbidden`,
//! `Conflict`, a limit, or a retry that commits) is always classified from stored state,
//! never from client input (ADR 015). Attempts are bounded; exhaustion fails closed with
//! `Unavailable`.
//!
//! Multi-record reads that must be mutually consistent use `TransactGetItems`, or a
//! version check before and after a partition query (list items and sync).

mod households;
mod keys;
mod lists;
mod profiles;
mod records;
mod store;

use std::fmt;

use aws_smithy_http_client::tls;

/// The AWS SDK for DynamoDB, re-exported so that the composition root builds the client with
/// exactly the SDK version this adapter was compiled against.
pub use aws_sdk_dynamodb as sdk;

/// A validated DynamoDB table name (3–255 characters of `[A-Za-z0-9_.-]`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableName(String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidTableName;

impl TableName {
    pub fn try_new(value: impl Into<String>) -> Result<Self, InvalidTableName> {
        let value = value.into();
        let valid_characters = value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_.-".contains(character));
        if (3..=255).contains(&value.len()) && valid_characters {
            Ok(Self(value))
        } else {
            Err(InvalidTableName)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Implements every repository port on one single-table DynamoDB design.
///
/// `Send + Sync`; cloning the SDK client is cheap, so one instance can back the whole router.
pub struct DynamoDbRepository {
    store: store::Store,
}

impl DynamoDbRepository {
    /// Wraps a configured client. The table must have the string key attributes `PK` (hash)
    /// and `SK` (range) and no other requirement (no index is used, ADR 003).
    pub fn new(client: sdk::Client, table: TableName) -> Self {
        Self {
            store: store::Store::new(client, table.0),
        }
    }
}

impl fmt::Debug for DynamoDbRepository {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DynamoDbRepository")
    }
}

/// HTTPS client for the SDK: rustls 0.23 with the `ring` provider and the platform's root
/// certificates. Pass it to `sdk::Config::builder().http_client(..)`, because the SDK's own
/// default client is disabled to keep aws-lc out of the dependency graph (ADR 019).
pub fn https_client() -> sdk::config::SharedHttpClient {
    aws_smithy_http_client::Builder::new()
        .tls_provider(tls::Provider::Rustls(
            tls::rustls_provider::CryptoMode::Ring,
        ))
        .build_https()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_names_are_validated() {
        assert!(TableName::try_new("einkaufio-dev.main_1").is_ok());
        assert_eq!(TableName::try_new("ab"), Err(InvalidTableName));
        assert_eq!(TableName::try_new("bad name"), Err(InvalidTableName));
        assert_eq!(TableName::try_new("x".repeat(256)), Err(InvalidTableName));
    }
}

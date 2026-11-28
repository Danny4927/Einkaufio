//! Thin, storage-specific layer over the DynamoDB client.
//!
//! Everything here speaks DynamoDB (attribute maps, condition expressions, transactions) and
//! nothing here knows the domain. Errors are reduced to [`StoreError`] without any request
//! detail: SDK errors can contain keys, and keys can contain invitation token hashes, which
//! must never reach logs (docs/security.md).

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use aws_sdk_dynamodb::Client;
use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError;
use aws_sdk_dynamodb::types::{
    AttributeValue, ConditionCheck, Delete, DeleteRequest, Get, KeysAndAttributes, Put,
    TransactGetItem, TransactWriteItem, Update, WriteRequest,
};
use ring::rand::{SecureRandom, SystemRandom};

use super::keys::{Key, PK, SK};

/// One stored record.
pub(super) type Item = HashMap<String, AttributeValue>;

/// Storage failure without details. Callers map it to their port's `Unavailable`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct StoreError;

/// Outcome of a conditional transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Commit {
    Committed,
    /// A condition failed or the transaction conflicted with a concurrent one. Nothing was
    /// written. The caller re-reads the records and classifies the outcome from persisted
    /// state (ADR 015/019); it never classifies from client input.
    Cancelled,
}

/// Maximum optimistic attempts (read → decide → conditional write) per port operation.
///
/// High enough for the documented contention (at most 10 members racing for one household),
/// bounded so that a persistent condition failure (a bug or corrupted data) fails closed.
pub(super) const MAX_ATTEMPTS: u32 = 40;
/// DynamoDB's per-transaction action limit.
pub(super) const MAX_TRANSACTION_ACTIONS: usize = 100;
const MAX_BATCH_GET: usize = 100;
const MAX_BATCH_WRITE: usize = 25;
const MAX_BATCH_ROUNDS: u32 = 10;

/// One elementary condition; a [`Condition`] joins them with `AND`.
#[derive(Clone, Debug)]
pub(super) enum Check {
    /// The record does not exist.
    Missing,
    /// The record exists.
    Present,
    Equals(&'static str, AttributeValue),
    /// `attribute_not_exists(attr) OR attr < value` (counters that start absent).
    MissingOrLessThan(&'static str, AttributeValue),
    GreaterThan(&'static str, AttributeValue),
}

#[derive(Clone, Debug, Default)]
pub(super) struct Condition(Vec<Check>);

impl Condition {
    pub(super) fn none() -> Self {
        Self::default()
    }
    pub(super) fn missing() -> Self {
        Self(vec![Check::Missing])
    }
    pub(super) fn present() -> Self {
        Self(vec![Check::Present])
    }
    pub(super) fn and(mut self, check: Check) -> Self {
        self.0.push(check);
        self
    }
    pub(super) fn equals(self, attribute: &'static str, value: AttributeValue) -> Self {
        self.and(Check::Equals(attribute, value))
    }
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// One action of a write transaction.
#[derive(Clone, Debug)]
pub(super) enum Action {
    Put {
        item: Item,
        condition: Condition,
    },
    Delete {
        key: Key,
        condition: Condition,
    },
    /// Verifies a record without writing it (access re-checks, ADR 015).
    Check {
        key: Key,
        condition: Condition,
    },
    /// `ADD attribute delta` on a counter record.
    Add {
        key: Key,
        attribute: &'static str,
        delta: i64,
        condition: Condition,
    },
}

#[derive(Default)]
struct Placeholders {
    names: HashMap<String, String>,
    values: HashMap<String, AttributeValue>,
}

impl Placeholders {
    fn name(&mut self, attribute: &str) -> String {
        let placeholder = format!("#n{}", self.names.len());
        self.names.insert(placeholder.clone(), attribute.to_owned());
        placeholder
    }

    fn value(&mut self, value: AttributeValue) -> String {
        let placeholder = format!(":v{}", self.values.len());
        self.values.insert(placeholder.clone(), value);
        placeholder
    }

    fn render(&mut self, condition: &Condition) -> Option<String> {
        if condition.is_empty() {
            return None;
        }
        let parts: Vec<String> = condition
            .0
            .iter()
            .map(|check| match check {
                Check::Missing => format!("attribute_not_exists({})", self.name(PK)),
                Check::Present => format!("attribute_exists({})", self.name(PK)),
                Check::Equals(attribute, value) => {
                    format!("{} = {}", self.name(attribute), self.value(value.clone()))
                }
                Check::MissingOrLessThan(attribute, value) => {
                    let name = self.name(attribute);
                    format!(
                        "(attribute_not_exists({name}) OR {name} < {})",
                        self.value(value.clone())
                    )
                }
                Check::GreaterThan(attribute, value) => {
                    format!("{} > {}", self.name(attribute), self.value(value.clone()))
                }
            })
            .collect();
        Some(parts.join(" AND "))
    }

    fn names(&self) -> Option<HashMap<String, String>> {
        (!self.names.is_empty()).then(|| self.names.clone())
    }

    fn values(&self) -> Option<HashMap<String, AttributeValue>> {
        (!self.values.is_empty()).then(|| self.values.clone())
    }
}

pub(super) fn string(value: impl Into<String>) -> AttributeValue {
    AttributeValue::S(value.into())
}

pub(super) fn number(value: impl Into<i128>) -> AttributeValue {
    AttributeValue::N(value.into().to_string())
}

fn key_map(key: &Key) -> Item {
    HashMap::from([
        (PK.to_owned(), string(key.pk.clone())),
        (SK.to_owned(), string(key.sk.clone())),
    ])
}

/// The sort key of a stored record, if well-formed.
pub(super) fn sort_key(item: &Item) -> Option<&str> {
    match item.get(SK) {
        Some(AttributeValue::S(value)) => Some(value),
        _ => None,
    }
}

fn record_key(item: &Item) -> Option<Key> {
    match (item.get(PK), item.get(SK)) {
        (Some(AttributeValue::S(pk)), Some(AttributeValue::S(sk))) => Some(Key {
            pk: pk.clone(),
            sk: sk.clone(),
        }),
        _ => None,
    }
}

fn transact_item(table: &str, action: Action) -> Result<TransactWriteItem, StoreError> {
    let mut placeholders = Placeholders::default();
    let item = match action {
        Action::Put { item, condition } => {
            let expression = placeholders.render(&condition);
            TransactWriteItem::builder().put(
                Put::builder()
                    .table_name(table)
                    .set_item(Some(item))
                    .set_condition_expression(expression)
                    .set_expression_attribute_names(placeholders.names())
                    .set_expression_attribute_values(placeholders.values())
                    .build()
                    .map_err(|_| StoreError)?,
            )
        }
        Action::Delete { key, condition } => {
            let expression = placeholders.render(&condition);
            TransactWriteItem::builder().delete(
                Delete::builder()
                    .table_name(table)
                    .set_key(Some(key_map(&key)))
                    .set_condition_expression(expression)
                    .set_expression_attribute_names(placeholders.names())
                    .set_expression_attribute_values(placeholders.values())
                    .build()
                    .map_err(|_| StoreError)?,
            )
        }
        Action::Check { key, condition } => {
            let expression = placeholders.render(&condition).ok_or(StoreError)?;
            TransactWriteItem::builder().condition_check(
                ConditionCheck::builder()
                    .table_name(table)
                    .set_key(Some(key_map(&key)))
                    .condition_expression(expression)
                    .set_expression_attribute_names(placeholders.names())
                    .set_expression_attribute_values(placeholders.values())
                    .build()
                    .map_err(|_| StoreError)?,
            )
        }
        Action::Add {
            key,
            attribute,
            delta,
            condition,
        } => {
            let expression = placeholders.render(&condition);
            let update = format!(
                "ADD {} {}",
                placeholders.name(attribute),
                placeholders.value(number(delta))
            );
            TransactWriteItem::builder().update(
                Update::builder()
                    .table_name(table)
                    .set_key(Some(key_map(&key)))
                    .update_expression(update)
                    .set_condition_expression(expression)
                    .set_expression_attribute_names(placeholders.names())
                    .set_expression_attribute_values(placeholders.values())
                    .build()
                    .map_err(|_| StoreError)?,
            )
        }
    };
    Ok(item.build())
}

/// Whether a cancelled transaction may succeed after a re-read.
///
/// Condition failures and conflicts with concurrent transactions are expected under
/// optimistic concurrency; throttling is transient. Every other reason (for example a
/// validation error) is a defect and fails closed.
fn retryable_cancellation(codes: &[Option<&str>]) -> bool {
    codes.iter().all(|code| {
        matches!(
            code,
            None | Some(
                "None"
                    | "ConditionalCheckFailed"
                    | "TransactionConflict"
                    | "ThrottlingError"
                    | "ProvisionedThroughputExceeded"
            )
        )
    })
}

/// DynamoDB access for one table.
pub(super) struct Store {
    client: Client,
    table: String,
    random: SystemRandom,
}

impl Store {
    pub(super) fn new(client: Client, table: String) -> Self {
        Self {
            client,
            table,
            random: SystemRandom::new(),
        }
    }

    /// Strongly consistent point read.
    pub(super) async fn get(&self, key: &Key) -> Result<Option<Item>, StoreError> {
        self.client
            .get_item()
            .table_name(&self.table)
            .set_key(Some(key_map(key)))
            .consistent_read(true)
            .send()
            .await
            .map(|output| output.item)
            .map_err(|_| StoreError)
    }

    /// Reads several records as one serializable snapshot (`TransactGetItems`). Results are
    /// returned in the order of `keys`; duplicate keys are read once.
    pub(super) async fn get_atomic(&self, keys: &[Key]) -> Result<Vec<Option<Item>>, StoreError> {
        let mut unique: Vec<&Key> = Vec::new();
        for key in keys {
            if !unique.contains(&key) {
                unique.push(key);
            }
        }
        if unique.is_empty() || unique.len() > MAX_TRANSACTION_ACTIONS {
            return Err(StoreError);
        }
        let gets = unique
            .iter()
            .map(|key| {
                Get::builder()
                    .table_name(&self.table)
                    .set_key(Some(key_map(key)))
                    .build()
                    .map(|get| TransactGetItem::builder().get(get).build())
                    .map_err(|_| StoreError)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let output = self
            .client
            .transact_get_items()
            .set_transact_items(Some(gets))
            .send()
            .await
            .map_err(|_| StoreError)?;
        let responses = output.responses.unwrap_or_default();
        if responses.len() != unique.len() {
            return Err(StoreError);
        }
        let by_key: HashMap<&Key, Option<Item>> = unique
            .into_iter()
            .zip(responses)
            .map(|(key, response)| (key, response.item))
            .collect();
        keys.iter()
            .map(|key| by_key.get(key).cloned().ok_or(StoreError))
            .collect()
    }

    /// Strongly consistent query of one partition, optionally restricted to a sort-key
    /// prefix. Follows every page; callers only query bounded partitions.
    pub(super) async fn query(
        &self,
        pk: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<Item>, StoreError> {
        let mut items = Vec::new();
        let mut start: Option<Item> = None;
        loop {
            let mut request = self
                .client
                .query()
                .table_name(&self.table)
                .consistent_read(true)
                .expression_attribute_names("#pk", PK)
                .expression_attribute_values(":pk", string(pk))
                .set_exclusive_start_key(start.take());
            request = match prefix {
                Some(prefix) => request
                    .key_condition_expression("#pk = :pk AND begins_with(#sk, :prefix)")
                    .expression_attribute_names("#sk", SK)
                    .expression_attribute_values(":prefix", string(prefix)),
                None => request.key_condition_expression("#pk = :pk"),
            };
            let output = request.send().await.map_err(|_| StoreError)?;
            items.extend(output.items.unwrap_or_default());
            match output.last_evaluated_key {
                Some(key) if !key.is_empty() => start = Some(key),
                _ => return Ok(items),
            }
        }
    }

    /// Strongly consistent batch read. Missing records are simply absent from the result.
    pub(super) async fn batch_get(&self, keys: &[Key]) -> Result<Vec<Item>, StoreError> {
        let mut seen = HashSet::new();
        let unique: Vec<&Key> = keys.iter().filter(|key| seen.insert(*key)).collect();
        let mut found = Vec::new();
        for chunk in unique.chunks(MAX_BATCH_GET) {
            let mut pending: Vec<Item> = chunk.iter().map(|key| key_map(key)).collect();
            let mut rounds = 0;
            while !pending.is_empty() {
                if rounds == MAX_BATCH_ROUNDS {
                    return Err(StoreError);
                }
                self.pause(rounds).await;
                rounds += 1;
                let request = KeysAndAttributes::builder()
                    .set_keys(Some(pending))
                    .consistent_read(true)
                    .build()
                    .map_err(|_| StoreError)?;
                let output = self
                    .client
                    .batch_get_item()
                    .request_items(&self.table, request)
                    .send()
                    .await
                    .map_err(|_| StoreError)?;
                if let Some(mut responses) = output.responses {
                    found.extend(responses.remove(&self.table).unwrap_or_default());
                }
                pending = output
                    .unprocessed_keys
                    .and_then(|mut unprocessed| unprocessed.remove(&self.table))
                    .map(|remaining| remaining.keys)
                    .unwrap_or_default();
            }
        }
        Ok(found)
    }

    /// Unconditionally deletes records in batches (bounded cleanup, ADR 017/019).
    /// Idempotent: deleting a missing record succeeds.
    pub(super) async fn batch_delete(&self, keys: &[Key]) -> Result<(), StoreError> {
        let mut seen = HashSet::new();
        let unique: Vec<&Key> = keys.iter().filter(|key| seen.insert(*key)).collect();
        for chunk in unique.chunks(MAX_BATCH_WRITE) {
            let mut pending = chunk
                .iter()
                .map(|key| {
                    DeleteRequest::builder()
                        .set_key(Some(key_map(key)))
                        .build()
                        .map(|delete| WriteRequest::builder().delete_request(delete).build())
                        .map_err(|_| StoreError)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut rounds = 0;
            while !pending.is_empty() {
                if rounds == MAX_BATCH_ROUNDS {
                    return Err(StoreError);
                }
                self.pause(rounds).await;
                rounds += 1;
                let output = self
                    .client
                    .batch_write_item()
                    .request_items(&self.table, pending)
                    .send()
                    .await
                    .map_err(|_| StoreError)?;
                pending = output
                    .unprocessed_items
                    .and_then(|mut unprocessed| unprocessed.remove(&self.table))
                    .unwrap_or_default();
            }
        }
        Ok(())
    }

    /// Commits the actions as one `TransactWriteItems` call (all or nothing).
    ///
    /// A fresh idempotency token makes SDK-level retries of the same call safe.
    pub(super) async fn commit(&self, actions: Vec<Action>) -> Result<Commit, StoreError> {
        if actions.is_empty() {
            return Ok(Commit::Committed);
        }
        if actions.len() > MAX_TRANSACTION_ACTIONS {
            return Err(StoreError);
        }
        let items = actions
            .into_iter()
            .map(|action| transact_item(&self.table, action))
            .collect::<Result<Vec<_>, _>>()?;
        let result = self
            .client
            .transact_write_items()
            .set_transact_items(Some(items))
            .client_request_token(self.token()?)
            .send()
            .await;
        match result {
            Ok(_) => Ok(Commit::Committed),
            Err(SdkError::ServiceError(error)) => match error.into_err() {
                TransactWriteItemsError::TransactionCanceledException(cancelled) => {
                    let codes: Vec<Option<&str>> = cancelled
                        .cancellation_reasons()
                        .iter()
                        .map(|reason| reason.code())
                        .collect();
                    if retryable_cancellation(&codes) {
                        Ok(Commit::Cancelled)
                    } else {
                        Err(StoreError)
                    }
                }
                // Throttling and a concurrent transaction on the same items are transient
                // and guarantee that nothing was written.
                TransactWriteItemsError::TransactionInProgressException(_)
                | TransactWriteItemsError::ProvisionedThroughputExceededException(_)
                | TransactWriteItemsError::RequestLimitExceeded(_)
                | TransactWriteItemsError::ThrottlingException(_) => Ok(Commit::Cancelled),
                _ => Err(StoreError),
            },
            Err(_) => Err(StoreError),
        }
    }

    /// Bounded, jittered exponential back-off before optimistic retry `attempt`.
    pub(super) async fn pause(&self, attempt: u32) {
        if attempt == 0 {
            return;
        }
        let ceiling_ms = 5_u64.saturating_mul(1 << attempt.min(5)).min(100);
        let mut bytes = [0_u8; 2];
        let jitter = match self.random.fill(&mut bytes) {
            Ok(()) => u64::from(u16::from_le_bytes(bytes)) % (ceiling_ms / 2 + 1),
            Err(_) => 0,
        };
        tokio::time::sleep(Duration::from_millis(ceiling_ms / 2 + jitter)).await;
    }

    fn token(&self) -> Result<String, StoreError> {
        let mut bytes = [0_u8; 16];
        self.random.fill(&mut bytes).map_err(|_| StoreError)?;
        Ok(super::keys::hex_encode(&bytes))
    }
}

/// Keys of the given records (used to sweep partitions).
pub(super) fn keys_of<'a>(
    items: impl IntoIterator<Item = &'a Item>,
) -> Result<Vec<Key>, StoreError> {
    items
        .into_iter()
        .map(|item| record_key(item).ok_or(StoreError))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditions_render_with_placeholders_only() {
        let mut placeholders = Placeholders::default();
        let condition = Condition::present()
            .equals("version", number(3_u64))
            .and(Check::MissingOrLessThan("count", number(5_u64)));
        let rendered = placeholders.render(&condition).unwrap();
        assert_eq!(
            rendered,
            "attribute_exists(#n0) AND #n1 = :v0 AND (attribute_not_exists(#n2) OR #n2 < :v1)"
        );
        assert_eq!(
            placeholders.names.get("#n1").map(String::as_str),
            Some("version")
        );
        assert_eq!(placeholders.values.get(":v1"), Some(&number(5_u64)));
        assert!(Placeholders::default().render(&Condition::none()).is_none());
    }

    #[test]
    fn only_optimistic_cancellations_are_retried() {
        assert!(retryable_cancellation(&[
            Some("None"),
            Some("ConditionalCheckFailed")
        ]));
        assert!(retryable_cancellation(&[None, Some("TransactionConflict")]));
        assert!(!retryable_cancellation(&[Some("ValidationError")]));
        assert!(!retryable_cancellation(&[
            Some("ConditionalCheckFailed"),
            Some("ItemCollectionSizeLimitExceeded")
        ]));
    }

    #[test]
    fn keys_of_rejects_malformed_records() {
        let well_formed = key_map(&Key {
            pk: "LIST#a".into(),
            sk: "META".into(),
        });
        assert_eq!(keys_of([&well_formed]).unwrap().len(), 1);
        assert_eq!(keys_of([&Item::new()]), Err(StoreError));
    }
}

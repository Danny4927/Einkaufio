//! Runs the shared repository contracts and race tests against DynamoDB (ADR 019).
//!
//! Requires DynamoDB Local (or a disposable DynamoDB endpoint) at `DYNAMODB_ENDPOINT`, for
//! example `http://localhost:8000`. The tests fail, rather than skip, when the variable is
//! missing, so that CI cannot report success without exercising the adapter
//! (`make test-dynamodb`, CI job `backend-dynamodb`). Every test creates its own table.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use einkaufio_adapters::dynamodb::sdk::config::{BehaviorVersion, Credentials, Region};
use einkaufio_adapters::dynamodb::sdk::types::{
    AttributeDefinition, BillingMode, KeySchemaElement, KeyType, ScalarAttributeType,
};
use einkaufio_adapters::dynamodb::{DynamoDbRepository, TableName, https_client, sdk};
use einkaufio_adapters::repository_contracts::{
    provision_household, run_account_data_contract, run_household_contract,
    run_invitation_contract, run_item_reorder_contract, run_membership_lifecycle_contract,
    run_pagination_contract, run_profile_contract, run_shared_access_contract,
    run_shared_query_contract, run_shopping_item_contract, run_shopping_list_contract,
    run_sync_contract,
};
use einkaufio_application::{
    HouseholdAccessRequest, HouseholdRepository, HouseholdRepositoryError, HouseholdSummary,
    InvitationRepository, ListAccessRequest, ProfileRepository, RepositoryError,
    ShoppingItemRepository, ShoppingListRepository,
};
use einkaufio_domain::{
    AggregateVersion, DisplayName, Household, HouseholdId, HouseholdOperation, HouseholdRole,
    Invitation, InvitationId, InvitationLifetime, InvitationMaxUses, InvitationTokenHash, ItemId,
    ItemName, ItemOrder, ListId, ListName, ListOperation, ShoppingItem, ShoppingList, Timestamp,
    UserId, UserProfile,
};

const NOW: u64 = 2_000;

fn endpoint() -> String {
    std::env::var("DYNAMODB_ENDPOINT").expect(
        "DYNAMODB_ENDPOINT must point to DynamoDB Local (see docs/local-development.md); \
         these tests never skip silently",
    )
}

fn client() -> sdk::Client {
    let config = sdk::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new("eu-central-1"))
        .endpoint_url(endpoint())
        .credentials_provider(Credentials::new("local", "local", None, None, "static"))
        .http_client(https_client())
        .build();
    sdk::Client::from_conf(config)
}

/// A fresh repository on its own new table.
async fn repository() -> DynamoDbRepository {
    fresh_table().await.0
}

/// A fresh repository plus the raw client and table name for storage-level assertions.
async fn fresh_table() -> (DynamoDbRepository, sdk::Client, String) {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let name = format!(
        "contract-{nanos}-{}",
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let client = client();
    let key = |name: &str, key_type| {
        KeySchemaElement::builder()
            .attribute_name(name)
            .key_type(key_type)
            .build()
            .unwrap()
    };
    let attribute = |name: &str| {
        AttributeDefinition::builder()
            .attribute_name(name)
            .attribute_type(ScalarAttributeType::S)
            .build()
            .unwrap()
    };
    client
        .create_table()
        .table_name(&name)
        .billing_mode(BillingMode::PayPerRequest)
        .key_schema(key("PK", KeyType::Hash))
        .key_schema(key("SK", KeyType::Range))
        .attribute_definitions(attribute("PK"))
        .attribute_definitions(attribute("SK"))
        .send()
        .await
        .expect("create contract table in DynamoDB Local");
    let repository = DynamoDbRepository::new(client.clone(), TableName::try_new(&name).unwrap());
    (repository, client, name)
}

/// Every (PK, SK) pair stored in the table.
async fn stored_keys(client: &sdk::Client, table: &str) -> Vec<(String, String)> {
    let mut keys = Vec::new();
    let mut start = None;
    loop {
        let output = client
            .scan()
            .table_name(table)
            .consistent_read(true)
            .set_exclusive_start_key(start.take())
            .send()
            .await
            .unwrap();
        for item in output.items.unwrap_or_default() {
            let text = |name: &str| item.get(name).unwrap().as_s().unwrap().clone();
            keys.push((text("PK"), text("SK")));
        }
        match output.last_evaluated_key {
            Some(key) if !key.is_empty() => start = Some(key),
            _ => return keys,
        }
    }
}

#[tokio::test]
async fn dynamodb_passes_list_contract() {
    let repository = repository().await;
    run_shopping_list_contract(&repository, &repository).await;
}

#[tokio::test]
async fn dynamodb_passes_item_contract() {
    let repository = repository().await;
    run_shopping_item_contract(&repository, &repository).await;
}

#[tokio::test]
async fn dynamodb_passes_shared_access_contract() {
    let repository = repository().await;
    run_shared_access_contract(&repository, &repository, &repository).await;
}

#[tokio::test]
async fn dynamodb_passes_reorder_contract() {
    let repository = repository().await;
    run_item_reorder_contract(&repository, &repository).await;
}

#[tokio::test]
async fn dynamodb_passes_pagination_contract() {
    let repository = repository().await;
    run_pagination_contract(&repository, &repository).await;
}

#[tokio::test]
async fn dynamodb_passes_sync_contract() {
    let repository = repository().await;
    run_sync_contract(&repository, &repository).await;
}

#[tokio::test]
async fn dynamodb_passes_shared_query_contract() {
    let repository = repository().await;
    run_shared_query_contract(&repository, &repository, &repository).await;
}

#[tokio::test]
async fn dynamodb_passes_profile_contract() {
    let repository = repository().await;
    run_profile_contract(&repository).await;
}

#[tokio::test]
async fn dynamodb_passes_household_contract() {
    let repository = repository().await;
    run_household_contract(&repository, &repository, &repository).await;
}

#[tokio::test]
async fn dynamodb_passes_membership_lifecycle_contract() {
    let repository = repository().await;
    run_membership_lifecycle_contract(&repository, &repository, &repository).await;
}

#[tokio::test]
async fn dynamodb_passes_invitation_contract() {
    let repository = repository().await;
    run_invitation_contract(&repository, &repository).await;
}

#[tokio::test]
async fn dynamodb_passes_account_data_contract() {
    let repository = repository().await;
    run_account_data_contract(&repository, &repository).await;
}

// ---------------------------------------------------------------------------------------
// Adapter-specific transaction and race tests (ADR 017 consequences, roadmap A5/B1/C4).
// ---------------------------------------------------------------------------------------

fn user(value: &str) -> UserId {
    UserId::try_new(value).unwrap()
}

fn list_id(value: &str) -> ListId {
    ListId::try_new(value).unwrap()
}

fn editor(principal: &str) -> ListAccessRequest {
    ListAccessRequest::for_operation(user(principal), ListOperation::EditItems)
}

fn item(index: usize) -> ShoppingItem {
    ShoppingItem::new(
        ItemId::try_new(format!("race-item-{index}")).unwrap(),
        ItemName::try_new("Milch").unwrap(),
        None,
        None,
        None,
        0,
        user("race-owner"),
    )
}

fn manage(principal: &str) -> HouseholdAccessRequest {
    HouseholdAccessRequest::for_operation(user(principal), HouseholdOperation::ManageInvitations)
}

fn hash(seed: u8) -> InvitationTokenHash {
    InvitationTokenHash::from_bytes([seed; 32])
}

async fn issue(repository: &DynamoDbRepository, id: &str, seed: u8, max_uses: u32) {
    let invitation = Invitation::new(
        InvitationId::try_new(id).unwrap(),
        HouseholdId::try_new("race").unwrap(),
        user("race-owner"),
        hash(seed),
        Timestamp::from_unix_seconds(1_000),
        InvitationLifetime::try_from_seconds(3_600).unwrap(),
        InvitationMaxUses::try_new(max_uses).unwrap(),
    )
    .unwrap();
    repository
        .create_invitation(
            &manage("race-owner"),
            invitation,
            Timestamp::from_unix_seconds(NOW),
        )
        .await
        .unwrap();
}

async fn accept_concurrently(
    repository: &Arc<DynamoDbRepository>,
    principals: Vec<String>,
    seeds: Vec<u8>,
) -> Vec<Result<HouseholdSummary, HouseholdRepositoryError>> {
    let tasks: Vec<_> = principals
        .into_iter()
        .zip(seeds.into_iter().cycle())
        .map(|(principal, seed)| {
            let repository = Arc::clone(repository);
            tokio::spawn(async move {
                repository
                    .accept_invitation(
                        &user(&principal),
                        &hash(seed),
                        Timestamp::from_unix_seconds(NOW),
                    )
                    .await
            })
        })
        .collect();
    let mut results = Vec::new();
    for task in tasks {
        results.push(task.await.unwrap());
    }
    results
}

async fn stored_invitations(repository: &DynamoDbRepository) -> Vec<Invitation> {
    repository
        .list_invitations(
            &manage("race-owner"),
            &HouseholdId::try_new("race").unwrap(),
        )
        .await
        .unwrap()
}

async fn member_count(repository: &DynamoDbRepository) -> usize {
    repository
        .get_household(
            &HouseholdAccessRequest::for_operation(user("race-owner"), HouseholdOperation::View),
            &HouseholdId::try_new("race").unwrap(),
        )
        .await
        .unwrap()
        .members
        .len()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_adds_with_the_same_expected_version_commit_exactly_once() {
    let repository = Arc::new(repository().await);
    repository
        .create_personal(ShoppingList::new_personal(
            list_id("race-list"),
            user("race-owner"),
            ListName::try_new("Race").unwrap(),
        ))
        .await
        .unwrap();
    let tasks: Vec<_> = (0..12)
        .map(|index| {
            let repository = Arc::clone(&repository);
            tokio::spawn(async move {
                repository
                    .add_item(
                        &editor("race-owner"),
                        &list_id("race-list"),
                        AggregateVersion::initial(),
                        item(index),
                    )
                    .await
            })
        })
        .collect();
    let mut committed = 0;
    for task in tasks {
        match task.await.unwrap() {
            Ok(version) => {
                committed += 1;
                assert_eq!(version.as_u64(), 2);
            }
            Err(error) => assert_eq!(error, RepositoryError::Conflict),
        }
    }
    assert_eq!(committed, 1);
    let items = repository
        .list_items(&editor("race-owner"), &list_id("race-list"))
        .await
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].position(), 0);
}

/// Concurrent item changes on different items without a list version (check/uncheck) all
/// commit: the adapter retries transaction conflicts on the shared list metadata, and every
/// change advances the list version exactly once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_checks_of_different_items_all_commit_with_distinct_versions() {
    let repository = Arc::new(repository().await);
    repository
        .create_personal(ShoppingList::new_personal(
            list_id("race-list"),
            user("race-owner"),
            ListName::try_new("Race").unwrap(),
        ))
        .await
        .unwrap();
    let mut version = AggregateVersion::initial();
    for index in 0..6 {
        version = repository
            .add_item(
                &editor("race-owner"),
                &list_id("race-list"),
                version,
                item(index),
            )
            .await
            .unwrap();
    }
    let tasks: Vec<_> = (0..6)
        .map(|index| {
            let repository = Arc::clone(&repository);
            tokio::spawn(async move {
                repository
                    .set_item_checked(
                        &editor("race-owner"),
                        &list_id("race-list"),
                        &ItemId::try_new(format!("race-item-{index}")).unwrap(),
                        1,
                        true,
                    )
                    .await
            })
        })
        .collect();
    let mut versions = Vec::new();
    for task in tasks {
        versions.push(task.await.unwrap().unwrap().1.as_u64());
    }
    versions.sort_unstable();
    assert_eq!(versions, (8..14).collect::<Vec<_>>());
}

/// A full list reordered in reverse moves all 100 items in one transaction.
#[tokio::test]
async fn reversing_a_full_list_is_one_atomic_reorder() {
    let repository = repository().await;
    repository
        .create_personal(ShoppingList::new_personal(
            list_id("full-list"),
            user("race-owner"),
            ListName::try_new("Voll").unwrap(),
        ))
        .await
        .unwrap();
    let mut version = AggregateVersion::initial();
    for index in 0..ShoppingList::MAX_ACTIVE_ITEMS {
        version = repository
            .add_item(
                &editor("race-owner"),
                &list_id("full-list"),
                version,
                item(index),
            )
            .await
            .unwrap();
    }
    let reversed = ItemOrder::try_new(
        (0..ShoppingList::MAX_ACTIVE_ITEMS)
            .rev()
            .map(|index| ItemId::try_new(format!("race-item-{index}")).unwrap())
            .collect(),
    )
    .unwrap();
    let (items, next) = repository
        .reorder_items(
            &editor("race-owner"),
            &list_id("full-list"),
            version,
            &reversed,
        )
        .await
        .unwrap();
    assert_eq!(next, version.checked_next().unwrap());
    assert_eq!(items[0].id().as_str(), "race-item-99");
    assert!(items.iter().all(|item| item.version() == 2));
    assert_eq!(
        repository
            .list_items(&editor("race-owner"), &list_id("full-list"))
            .await
            .unwrap(),
        items
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_acceptances_never_exceed_the_use_count() {
    let repository = Arc::new(repository().await);
    provision_household(repository.as_ref(), "race", "race-owner", &[]).await;
    issue(&repository, "race-invite", 1, 3).await;
    let results = accept_concurrently(
        &repository,
        (0..9).map(|index| format!("race-user-{index}")).collect(),
        vec![1],
    )
    .await;
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 3);
    assert!(results.iter().all(|result| matches!(
        result,
        Ok(_) | Err(HouseholdRepositoryError::InvitationUnavailable)
    )));
    assert_eq!(stored_invitations(&repository).await[0].used_count(), 3);
    assert_eq!(member_count(&repository).await, 4);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_acceptances_by_one_user_consume_one_use() {
    let repository = Arc::new(repository().await);
    provision_household(repository.as_ref(), "race", "race-owner", &[]).await;
    issue(&repository, "race-invite", 1, 10).await;
    let results = accept_concurrently(&repository, vec!["race-same".to_owned(); 8], vec![1]).await;
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert!(
        results
            .iter()
            .all(|result| matches!(result, Ok(_) | Err(HouseholdRepositoryError::AlreadyMember)))
    );
    assert_eq!(stored_invitations(&repository).await[0].used_count(), 1);
    assert_eq!(member_count(&repository).await, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_acceptances_respect_the_member_limit() {
    let repository = Arc::new(repository().await);
    provision_household(repository.as_ref(), "race", "race-owner", &[]).await;
    issue(&repository, "race-a", 1, 10).await;
    issue(&repository, "race-b", 2, 10).await;
    let results = accept_concurrently(
        &repository,
        (0..14).map(|index| format!("race-user-{index}")).collect(),
        vec![1, 2],
    )
    .await;
    let accepted = results.iter().filter(|result| result.is_ok()).count();
    assert_eq!(accepted, Household::MAX_MEMBERS - 1);
    assert!(results.iter().all(|result| matches!(
        result,
        Ok(_) | Err(HouseholdRepositoryError::MemberLimitReached)
    )));
    let used: u32 = stored_invitations(&repository)
        .await
        .iter()
        .map(Invitation::used_count)
        .sum();
    assert_eq!(used as usize, accepted);
    assert_eq!(member_count(&repository).await, Household::MAX_MEMBERS);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn revocation_racing_with_acceptance_stays_consistent() {
    let repository = Arc::new(repository().await);
    provision_household(repository.as_ref(), "race", "race-owner", &[]).await;
    issue(&repository, "race-invite", 1, 10).await;
    let revoker = {
        let repository = Arc::clone(&repository);
        tokio::spawn(async move {
            repository
                .revoke_invitation(
                    &manage("race-owner"),
                    &HouseholdId::try_new("race").unwrap(),
                    &InvitationId::try_new("race-invite").unwrap(),
                )
                .await
        })
    };
    let results = accept_concurrently(
        &repository,
        (0..6).map(|index| format!("race-user-{index}")).collect(),
        vec![1],
    )
    .await;
    revoker.await.unwrap().unwrap();
    let accepted = results.iter().filter(|result| result.is_ok()).count();
    let stored = stored_invitations(&repository).await;
    assert!(stored[0].is_revoked());
    assert_eq!(stored[0].used_count() as usize, accepted);
    assert_eq!(member_count(&repository).await, accepted + 1);
    assert_eq!(
        repository
            .accept_invitation(
                &user("race-late"),
                &hash(1),
                Timestamp::from_unix_seconds(NOW)
            )
            .await
            .map(|_| ()),
        Err(HouseholdRepositoryError::InvitationUnavailable)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_ownership_transfers_commit_exactly_once() {
    let repository = Arc::new(repository().await);
    let members: Vec<String> = (0..4).map(|index| format!("race-member-{index}")).collect();
    let member_refs: Vec<&str> = members.iter().map(String::as_str).collect();
    let household =
        provision_household(repository.as_ref(), "race", "race-owner", &member_refs).await;
    let tasks: Vec<_> = members
        .iter()
        .map(|member| {
            let repository = Arc::clone(&repository);
            let member = user(member);
            let expected = household.version();
            tokio::spawn(async move {
                repository
                    .transfer_ownership(
                        &HouseholdAccessRequest::for_operation(
                            user("race-owner"),
                            HouseholdOperation::TransferOwnership,
                        ),
                        &HouseholdId::try_new("race").unwrap(),
                        expected,
                        &member,
                    )
                    .await
            })
        })
        .collect();
    let mut committed = 0;
    for task in tasks {
        match task.await.unwrap() {
            Ok(_) => committed += 1,
            Err(error) => assert!(matches!(
                error,
                HouseholdRepositoryError::Conflict | HouseholdRepositoryError::Forbidden
            )),
        }
    }
    assert_eq!(committed, 1);
    let details = repository
        .get_household(
            &HouseholdAccessRequest::for_operation(user("race-member-0"), HouseholdOperation::View),
            &HouseholdId::try_new("race").unwrap(),
        )
        .await
        .unwrap();
    let owners = details
        .members
        .iter()
        .filter(|member| member.role == HouseholdRole::Owner)
        .count();
    assert_eq!(owners, 1);
}

/// Concurrent household creations by one user cannot exceed the per-user household limit:
/// the counter is enforced by a condition inside each creation transaction.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_household_creations_respect_the_per_user_limit() {
    let repository = Arc::new(repository().await);
    let tasks: Vec<_> = (0..8)
        .map(|index| {
            let repository = Arc::clone(&repository);
            tokio::spawn(async move {
                repository
                    .create_household(Household::new(
                        HouseholdId::try_new(format!("busy-{index}")).unwrap(),
                        user("busy"),
                        einkaufio_domain::HouseholdName::try_new("Viele").unwrap(),
                    ))
                    .await
            })
        })
        .collect();
    let mut created = 0;
    for task in tasks {
        match task.await.unwrap() {
            Ok(()) => created += 1,
            Err(error) => assert_eq!(error, HouseholdRepositoryError::HouseholdLimitReached),
        }
    }
    assert_eq!(created, Household::MAX_HOUSEHOLDS_PER_USER);
    assert_eq!(
        repository
            .list_households(&user("busy"))
            .await
            .unwrap()
            .len(),
        Household::MAX_HOUSEHOLDS_PER_USER
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_profile_updates_with_the_same_version_commit_exactly_once() {
    let repository = Arc::new(repository().await);
    repository
        .create_profile(UserProfile::new(
            user("race-owner"),
            DisplayName::try_new("Start").unwrap(),
        ))
        .await
        .unwrap();
    let tasks: Vec<_> = (0..8)
        .map(|index| {
            let repository = Arc::clone(&repository);
            tokio::spawn(async move {
                repository
                    .update_profile(
                        &user("race-owner"),
                        AggregateVersion::initial(),
                        DisplayName::try_new(format!("Name {index}")).unwrap(),
                    )
                    .await
            })
        })
        .collect();
    let mut committed = 0;
    for task in tasks {
        match task.await.unwrap() {
            Ok(profile) => {
                committed += 1;
                assert_eq!(profile.version().as_u64(), 2);
            }
            Err(error) => assert_eq!(error, RepositoryError::Conflict),
        }
    }
    assert_eq!(committed, 1);
}

/// Bounded deletion (ADR 017/019) leaves no orphaned records: deleting a personal list with
/// items and tombstones and a household with lists, items and invitations removes every
/// record except the users' (now zero) household counters.
#[tokio::test]
async fn deletions_sweep_every_child_record() {
    let (repository, client, table) = fresh_table().await;
    repository
        .create_personal(ShoppingList::new_personal(
            list_id("sweep-personal"),
            user("race-owner"),
            ListName::try_new("Privat").unwrap(),
        ))
        .await
        .unwrap();
    let mut version = AggregateVersion::initial();
    for index in 0..3 {
        version = repository
            .add_item(
                &editor("race-owner"),
                &list_id("sweep-personal"),
                version,
                item(index),
            )
            .await
            .unwrap();
    }
    version = repository
        .delete_item(
            &editor("race-owner"),
            &list_id("sweep-personal"),
            &ItemId::try_new("race-item-0").unwrap(),
            1,
        )
        .await
        .unwrap();
    repository
        .delete_list(
            &ListAccessRequest::for_operation(user("race-owner"), ListOperation::Delete),
            &list_id("sweep-personal"),
            version,
        )
        .await
        .unwrap();

    let household = provision_household(&repository, "race", "race-owner", &["race-member"]).await;
    for index in 0..3 {
        let list = ShoppingList::new_household(
            list_id(&format!("sweep-household-{index}")),
            HouseholdId::try_new("race").unwrap(),
            ListName::try_new("WG").unwrap(),
        );
        repository
            .create_household_list(
                &HouseholdAccessRequest::for_operation(
                    user("race-member"),
                    HouseholdOperation::CreateList,
                ),
                list.clone(),
            )
            .await
            .unwrap();
        repository
            .add_item(
                &editor("race-member"),
                list.id(),
                AggregateVersion::initial(),
                item(index),
            )
            .await
            .unwrap();
    }
    issue(&repository, "sweep-invite", 3, 5).await;
    repository
        .delete_household(
            &HouseholdAccessRequest::for_operation(user("race-owner"), HouseholdOperation::Delete),
            household.id(),
            household.version(),
        )
        .await
        .unwrap();

    let leftovers: Vec<_> = stored_keys(&client, &table)
        .await
        .into_iter()
        .filter(|(_, sk)| sk != "HOUSEHOLD_COUNT")
        .collect();
    assert!(leftovers.is_empty(), "orphaned records: {leftovers:?}");
    assert!(
        repository
            .list_households(&user("race-member"))
            .await
            .unwrap()
            .is_empty()
    );
}

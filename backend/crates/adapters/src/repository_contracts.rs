//! Reusable repository behavior contracts.
//!
//! Enable the `test-support` feature to reuse these scenarios from adapter integration tests.
//! Each contract provisions its own unique fixtures and expects a fresh repository pair.
//! Scenarios are async (ADR 014) and need no specific runtime; adapters drive them from their
//! own async tests. Access scoping follows ADR 015.

use std::collections::BTreeMap;

use einkaufio_application::{
    HouseholdAccessRequest, HouseholdRepository, HouseholdRepositoryError, InvitationRepository,
    ListAccessRequest, ListSync, PageCursor, PageRequest, PageSize, ProfileRepository,
    RepositoryError, ShoppingItemRepository, ShoppingListRepository,
};
use einkaufio_domain::{
    AggregateVersion, DisplayName, Household, HouseholdId, HouseholdName, HouseholdOperation,
    HouseholdPermission, Invitation, InvitationId, InvitationLifetime, InvitationMaxUses,
    InvitationTokenHash, ItemDetails, ItemId, ItemName, ItemNote, ItemOrder, ListId, ListName,
    ListOperation, ListPermission, ShoppingItem, ShoppingList, Timestamp, UserId, UserProfile,
};

fn user(value: &str) -> UserId {
    UserId::try_new(value).expect("valid test user")
}
fn list_id(value: &str) -> ListId {
    ListId::try_new(value).expect("valid test list id")
}
fn version(value: u64) -> AggregateVersion {
    AggregateVersion::try_new(value).expect("positive test version")
}
fn list(id: &str, owner: &str, name: &str) -> ShoppingList {
    ShoppingList::new_personal(list_id(id), user(owner), ListName::try_new(name).unwrap())
}
fn item(id: &str, owner: &str, supplied_position: u32) -> ShoppingItem {
    ShoppingItem::new(
        ItemId::try_new(id).unwrap(),
        ItemName::try_new("Milch").unwrap(),
        None,
        None,
        None,
        supplied_position,
        user(owner),
    )
}
fn reader(principal: &UserId) -> ListAccessRequest {
    ListAccessRequest::for_operation(principal.clone(), ListOperation::ReadItems)
}
fn editor(principal: &UserId) -> ListAccessRequest {
    ListAccessRequest::for_operation(principal.clone(), ListOperation::EditItems)
}
fn renamer(principal: &UserId) -> ListAccessRequest {
    ListAccessRequest::for_operation(principal.clone(), ListOperation::Rename)
}
fn deleter(principal: &UserId) -> ListAccessRequest {
    ListAccessRequest::for_operation(principal.clone(), ListOperation::Delete)
}
fn sorted(mut lists: Vec<ShoppingList>) -> Vec<ShoppingList> {
    lists.sort_by(|left, right| left.id().as_str().cmp(right.id().as_str()));
    lists
}

/// Household and invitation ports implemented by one adapter (they share memberships).
pub trait HouseholdStore: HouseholdRepository + InvitationRepository {}

impl<T: HouseholdRepository + InvitationRepository> HouseholdStore for T {}

pub(crate) fn household_id(value: &str) -> HouseholdId {
    HouseholdId::try_new(value).expect("valid test household id")
}
pub(crate) fn at(seconds: u64) -> Timestamp {
    Timestamp::from_unix_seconds(seconds)
}
fn household(id: &str, owner: &str, name: &str) -> Household {
    Household::new(
        household_id(id),
        user(owner),
        HouseholdName::try_new(name).unwrap(),
    )
}
fn household_list(id: &str, household: &str, name: &str) -> ShoppingList {
    ShoppingList::new_household(
        list_id(id),
        household_id(household),
        ListName::try_new(name).unwrap(),
    )
}
pub(crate) fn household_access(
    principal: &UserId,
    operation: HouseholdOperation,
) -> HouseholdAccessRequest {
    HouseholdAccessRequest::for_operation(principal.clone(), operation)
}
/// A deterministic stand-in for SHA-256(token): contracts never handle raw tokens.
pub(crate) fn token_hash(seed: &str) -> InvitationTokenHash {
    let mut bytes = [0_u8; 32];
    for (index, byte) in seed.bytes().cycle().take(32).enumerate() {
        bytes[index] = byte.wrapping_add(u8::try_from(index).unwrap_or(0));
    }
    InvitationTokenHash::from_bytes(bytes)
}
/// An invitation issued at t=1000 for one hour (expires at t=4600).
pub(crate) fn invitation(
    id: &str,
    household: &str,
    creator: &str,
    seed: &str,
    max_uses: u32,
) -> Invitation {
    Invitation::new(
        InvitationId::try_new(id).unwrap(),
        household_id(household),
        user(creator),
        token_hash(seed),
        at(1_000),
        InvitationLifetime::try_from_seconds(3_600).unwrap(),
        InvitationMaxUses::try_new(max_uses).unwrap(),
    )
    .unwrap()
}
pub(crate) const NOW: u64 = 2_000;
pub(crate) const EXPIRES: u64 = 4_600;

/// Creates a household owned by `owner` and makes each of `members` an active member through
/// the production path (owner invitation + authenticated acceptance).
pub async fn provision_household(
    households: &impl HouseholdStore,
    id: &str,
    owner: &str,
    members: &[&str],
) -> Household {
    households
        .create_household(household(id, owner, "Haushalt"))
        .await
        .expect("create household");
    if !members.is_empty() {
        let seed = format!("provision-{id}");
        households
            .create_invitation(
                &household_access(&user(owner), HouseholdOperation::ManageInvitations),
                invitation(
                    &format!("{id}-provision"),
                    id,
                    owner,
                    &seed,
                    u32::try_from(members.len()).unwrap(),
                ),
                at(NOW),
            )
            .await
            .expect("create provisioning invitation");
        for member in members {
            households
                .accept_invitation(&user(member), &token_hash(&seed), at(NOW))
                .await
                .expect("accept provisioning invitation");
        }
    }
    households
        .get_household(
            &household_access(&user(owner), HouseholdOperation::View),
            &household_id(id),
        )
        .await
        .expect("read provisioned household")
        .household
}

/// Provisions a household-owned list administered by `administrator` (household owner) with
/// `contributor` as active member. Uses production port operations only (ADR 015).
async fn provision_shared_list(
    households: &impl HouseholdStore,
    household: &str,
    list: &str,
    administrator: &UserId,
    contributor: &UserId,
) -> ShoppingList {
    provision_household(
        households,
        household,
        administrator.as_str(),
        &[contributor.as_str()],
    )
    .await;
    let shared = household_list(list, household, "Gemeinsam");
    households
        .create_household_list(
            &household_access(administrator, HouseholdOperation::CreateList),
            shared.clone(),
        )
        .await
        .expect("create household list");
    shared
}

/// Ends the contributor's access by leaving the household.
async fn revoke_contributor(
    households: &impl HouseholdStore,
    household: &str,
    contributor: &UserId,
) {
    households
        .leave_household(contributor, &household_id(household))
        .await
        .expect("leave household");
}

/// Runs list and cascade scenarios against a fresh pair of repositories.
pub async fn run_shopping_list_contract(
    lists: &impl ShoppingListRepository,
    items: &impl ShoppingItemRepository,
) {
    let owner = user("contract-owner");
    let other = user("contract-other");
    let created = list("contract-list", "contract-owner", "Einkauf");
    lists
        .create_personal(created.clone())
        .await
        .expect("create");
    assert_eq!(
        lists.create_personal(created.clone()).await,
        Err(RepositoryError::AlreadyExists)
    );
    assert_eq!(
        lists.list_accessible(&owner).await.unwrap(),
        vec![created.clone()]
    );
    assert!(lists.list_accessible(&other).await.unwrap().is_empty());

    // Access-based listing returns every owned list and never another principal's list.
    let second = list("contract-list-2", "contract-owner", "Zweite");
    let foreign = list("contract-foreign-list", "contract-other", "Fremd");
    lists
        .create_personal(second.clone())
        .await
        .expect("create second");
    lists
        .create_personal(foreign.clone())
        .await
        .expect("create foreign");
    assert_eq!(
        sorted(lists.list_accessible(&owner).await.unwrap()),
        vec![created.clone(), second.clone()]
    );
    assert_eq!(
        lists.list_accessible(&other).await.unwrap(),
        vec![foreign.clone()]
    );

    assert_eq!(
        lists
            .rename_list(
                &renamer(&other),
                created.id(),
                version(1),
                ListName::try_new("X").unwrap()
            )
            .await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        lists
            .rename_list(
                &renamer(&other),
                &list_id("absent"),
                version(1),
                ListName::try_new("X").unwrap()
            )
            .await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        lists
            .rename_list(
                &renamer(&owner),
                created.id(),
                version(2),
                ListName::try_new("Einkauf").unwrap()
            )
            .await,
        Err(RepositoryError::Conflict)
    );
    let renamed = lists
        .rename_list(
            &renamer(&owner),
            created.id(),
            version(1),
            ListName::try_new("Drogerie").unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(renamed.version(), 2);
    let no_op = lists
        .rename_list(
            &renamer(&owner),
            created.id(),
            version(2),
            ListName::try_new("Drogerie").unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(no_op.version(), 2);

    // A stale delete must not remove the aggregate.
    assert_eq!(
        lists
            .delete_list(&deleter(&owner), created.id(), version(1))
            .await,
        Err(RepositoryError::Conflict)
    );
    // Create an item and verify list deletion cascades to its items.
    let list_version = items
        .add_item(
            &editor(&owner),
            created.id(),
            version(2),
            item("cascade-item", "contract-owner", 77),
        )
        .await
        .unwrap();
    assert_eq!(list_version, version(3));
    assert_eq!(
        items.list_items(&reader(&other), created.id()).await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        items
            .list_items(&reader(&owner), created.id())
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        lists
            .delete_list(&deleter(&other), created.id(), version(3))
            .await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        lists
            .delete_list(&deleter(&owner), created.id(), version(2))
            .await,
        Err(RepositoryError::Conflict)
    );
    lists
        .delete_list(&deleter(&owner), created.id(), version(3))
        .await
        .unwrap();
    assert_eq!(
        items.list_items(&reader(&owner), created.id()).await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        lists.list_accessible(&owner).await.unwrap(),
        vec![second.clone()]
    );

    // Reusing the list and item IDs proves no orphaned item remains after cascade deletion.
    lists
        .create_personal(list("contract-list", "contract-owner", "Recreated"))
        .await
        .unwrap();
    assert_eq!(
        items
            .add_item(
                &editor(&owner),
                created.id(),
                version(1),
                item("cascade-item", "contract-owner", 0)
            )
            .await,
        Ok(version(2))
    );
}

/// Runs item scenarios against a fresh repository pair; this contract creates its own list.
pub async fn run_shopping_item_contract(
    lists: &impl ShoppingListRepository,
    items: &impl ShoppingItemRepository,
) {
    let owner = user("contract-item-owner");
    let other = user("contract-item-other");
    let item_list = list("contract-item-list", "contract-item-owner", "Items");
    lists
        .create_personal(item_list.clone())
        .await
        .expect("provision contract list");

    assert_eq!(
        items
            .add_item(
                &editor(&other),
                item_list.id(),
                version(1),
                item("unauthorized-item", "contract-item-owner", 0)
            )
            .await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        items
            .add_item(
                &editor(&other),
                &list_id("missing-parent-list"),
                version(1),
                item("unknown-parent", "contract-item-owner", 0)
            )
            .await,
        Err(RepositoryError::NotFound)
    );

    let list_version = items
        .add_item(
            &editor(&owner),
            item_list.id(),
            version(1),
            item("contract-item-1", "contract-item-owner", 999),
        )
        .await
        .expect("first item");
    assert_eq!(list_version, version(2));
    assert_eq!(
        items
            .add_item(
                &editor(&owner),
                item_list.id(),
                version(2),
                item("contract-item-1", "contract-item-owner", 0)
            )
            .await,
        Err(RepositoryError::AlreadyExists)
    );
    assert_eq!(
        items
            .add_item(
                &editor(&owner),
                item_list.id(),
                version(1),
                item("stale-item", "contract-item-owner", 0)
            )
            .await,
        Err(RepositoryError::Conflict)
    );

    let item_id = ItemId::try_new("contract-item-1").unwrap();
    assert_eq!(
        items
            .set_item_checked(&editor(&other), item_list.id(), &item_id, 1, true)
            .await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        items
            .set_item_checked(&editor(&owner), item_list.id(), &item_id, 99, true)
            .await,
        Err(RepositoryError::Conflict)
    );
    let (checked, list_version) = items
        .set_item_checked(&editor(&owner), item_list.id(), &item_id, 1, true)
        .await
        .unwrap();
    assert!(checked.checked());
    assert_eq!(checked.version(), 2);
    assert_eq!(list_version, version(3));

    let (unchanged, list_version) = items
        .set_item_checked(&editor(&owner), item_list.id(), &item_id, 2, true)
        .await
        .unwrap();
    assert_eq!(unchanged.version(), 2);
    assert_eq!(list_version, version(3));

    let (unchecked, list_version) = items
        .set_item_checked(&editor(&owner), item_list.id(), &item_id, 2, false)
        .await
        .unwrap();
    assert!(!unchecked.checked());
    assert_eq!(unchecked.version(), 3);
    assert_eq!(list_version, version(4));

    // Detail updates: authorize first, stale versions conflict even for no-ops, and only real
    // changes advance item and list versions while preserving check state and position.
    let edited = ItemDetails {
        name: ItemName::try_new("Contract item edited").unwrap(),
        quantity: None,
        unit: None,
        note: Some(ItemNote::try_new("contract note").unwrap()),
    };
    assert_eq!(
        items
            .update_item_details(&editor(&other), item_list.id(), &item_id, 3, edited.clone())
            .await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        items
            .update_item_details(
                &editor(&owner),
                item_list.id(),
                &ItemId::try_new("missing-item").unwrap(),
                1,
                edited.clone()
            )
            .await,
        Err(RepositoryError::NotFound)
    );
    let current_details = unchecked.details();
    assert_eq!(
        items
            .update_item_details(
                &editor(&owner),
                item_list.id(),
                &item_id,
                2,
                current_details.clone()
            )
            .await,
        Err(RepositoryError::Conflict)
    );
    let (same, list_version) = items
        .update_item_details(
            &editor(&owner),
            item_list.id(),
            &item_id,
            3,
            current_details,
        )
        .await
        .unwrap();
    assert_eq!(same.version(), 3);
    assert_eq!(list_version, version(4));
    let (updated, list_version) = items
        .update_item_details(&editor(&owner), item_list.id(), &item_id, 3, edited.clone())
        .await
        .unwrap();
    assert_eq!(updated.details(), edited);
    assert_eq!(updated.version(), 4);
    assert_eq!(updated.position(), unchecked.position());
    assert_eq!(updated.checked(), unchecked.checked());
    assert_eq!(list_version, version(5));
    assert_eq!(
        items
            .list_items(&reader(&owner), item_list.id())
            .await
            .unwrap(),
        vec![updated]
    );

    assert_eq!(
        items
            .delete_item(&editor(&owner), item_list.id(), &item_id, 3)
            .await,
        Err(RepositoryError::Conflict)
    );
    assert_eq!(
        items
            .delete_item(&editor(&other), item_list.id(), &item_id, 4)
            .await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        items
            .delete_item(&editor(&owner), item_list.id(), &item_id, 4)
            .await,
        Ok(version(6))
    );
    assert_eq!(
        items
            .delete_item(&editor(&owner), item_list.id(), &item_id, 4)
            .await,
        Err(RepositoryError::NotFound)
    );

    // Capacity and repository-assigned unique positions are part of the shared port contract.
    let capacity_list = list("contract-capacity-list", "contract-item-owner", "Capacity");
    lists.create_personal(capacity_list.clone()).await.unwrap();
    for index in 0..ShoppingList::MAX_ACTIVE_ITEMS {
        let expected = version(index as u64 + 1);
        let next = items
            .add_item(
                &editor(&owner),
                capacity_list.id(),
                expected,
                item(&format!("capacity-item-{index}"), "contract-item-owner", 42),
            )
            .await
            .unwrap();
        assert_eq!(next, version(index as u64 + 2));
    }
    assert_eq!(
        items
            .add_item(
                &editor(&owner),
                capacity_list.id(),
                version(101),
                item("capacity-overflow", "contract-item-owner", 0)
            )
            .await,
        Err(RepositoryError::ItemLimitReached)
    );
    let stored = items
        .list_items(&reader(&owner), capacity_list.id())
        .await
        .unwrap();
    assert_eq!(stored.len(), ShoppingList::MAX_ACTIVE_ITEMS);
    let positions: Vec<_> = stored.iter().map(ShoppingItem::position).collect();
    assert_eq!(
        positions,
        (0..ShoppingList::MAX_ACTIVE_ITEMS as u32).collect::<Vec<_>>()
    );
    assert_eq!(
        items.list_items(&reader(&other), capacity_list.id()).await,
        Err(RepositoryError::NotFound)
    );
}

/// Runs contributor-access scenarios: contributors use list content but cannot administer
/// the list, outsiders learn nothing, and revoked access takes effect on the next request.
pub async fn run_shared_access_contract(
    lists: &impl ShoppingListRepository,
    items: &impl ShoppingItemRepository,
    households: &impl HouseholdStore,
) {
    let administrator = user("shared-admin");
    let contributor = user("shared-contributor");
    let outsider = user("shared-outsider");
    let shared = provision_shared_list(
        households,
        "shared-household",
        "shared-list",
        &administrator,
        &contributor,
    )
    .await;

    assert_eq!(
        lists.list_accessible(&administrator).await.unwrap(),
        vec![shared.clone()]
    );
    assert_eq!(
        lists.list_accessible(&contributor).await.unwrap(),
        vec![shared.clone()]
    );
    assert!(lists.list_accessible(&outsider).await.unwrap().is_empty());

    // Contributors read and edit items; list and item versions behave as for the owner.
    let item_id = ItemId::try_new("shared-item").unwrap();
    assert_eq!(
        items
            .add_item(
                &editor(&contributor),
                shared.id(),
                version(1),
                item("shared-item", "shared-contributor", 0)
            )
            .await,
        Ok(version(2))
    );
    let (checked, list_version) = items
        .set_item_checked(&editor(&contributor), shared.id(), &item_id, 1, true)
        .await
        .unwrap();
    assert!(checked.checked());
    assert_eq!(list_version, version(3));
    let details = ItemDetails {
        name: ItemName::try_new("Hafermilch").unwrap(),
        quantity: None,
        unit: None,
        note: None,
    };
    let (updated, list_version) = items
        .update_item_details(&editor(&contributor), shared.id(), &item_id, 2, details)
        .await
        .unwrap();
    assert_eq!(updated.version(), 3);
    assert_eq!(list_version, version(4));
    assert_eq!(
        items
            .list_items(&reader(&administrator), shared.id())
            .await
            .unwrap(),
        vec![updated.clone()]
    );
    assert_eq!(
        items
            .list_items(&reader(&contributor), shared.id())
            .await
            .unwrap(),
        vec![updated.clone()]
    );

    // Contributors cannot administer the list. Authorization precedes the version check, so a
    // stale version still yields Forbidden, and a refused operation changes nothing.
    for expected in [version(4), version(1)] {
        assert_eq!(
            lists
                .rename_list(
                    &renamer(&contributor),
                    shared.id(),
                    expected,
                    ListName::try_new("Übernommen").unwrap()
                )
                .await,
            Err(RepositoryError::Forbidden)
        );
        assert_eq!(
            lists
                .delete_list(&deleter(&contributor), shared.id(), expected)
                .await,
            Err(RepositoryError::Forbidden)
        );
    }
    // Adapters enforce the requested permission generically, including on item operations.
    assert_eq!(
        items
            .list_items(
                &ListAccessRequest::new(contributor.clone(), ListPermission::Administer),
                shared.id()
            )
            .await,
        Err(RepositoryError::Forbidden)
    );

    // Outsiders cannot distinguish the shared list from a missing one, not even through
    // Forbidden on administrative operations.
    assert_eq!(
        lists
            .rename_list(
                &renamer(&outsider),
                shared.id(),
                version(4),
                ListName::try_new("X").unwrap()
            )
            .await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        lists
            .delete_list(&deleter(&outsider), shared.id(), version(4))
            .await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        items.list_items(&reader(&outsider), shared.id()).await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        items
            .add_item(
                &editor(&outsider),
                shared.id(),
                version(4),
                item("outsider-item", "shared-outsider", 0)
            )
            .await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        items
            .delete_item(&editor(&outsider), shared.id(), &item_id, 3)
            .await,
        Err(RepositoryError::NotFound)
    );
    let unchanged = lists.list_accessible(&administrator).await.unwrap();
    assert_eq!(unchanged.len(), 1);
    assert_eq!(unchanged[0].name().as_str(), "Gemeinsam");
    assert_eq!(unchanged[0].version(), 4);

    let renamed = lists
        .rename_list(
            &renamer(&administrator),
            shared.id(),
            version(4),
            ListName::try_new("Wocheneinkauf").unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(renamed.version(), 5);

    // Persisted access is re-checked on every request: revocation takes effect immediately and
    // the former contributor is treated exactly like an outsider.
    revoke_contributor(households, "shared-household", &contributor).await;
    assert!(
        lists
            .list_accessible(&contributor)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        items.list_items(&reader(&contributor), shared.id()).await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        items
            .delete_item(&editor(&contributor), shared.id(), &item_id, 3)
            .await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        lists
            .delete_list(&deleter(&contributor), shared.id(), version(5))
            .await,
        Err(RepositoryError::NotFound)
    );

    lists
        .delete_list(&deleter(&administrator), shared.id(), version(5))
        .await
        .unwrap();
    assert!(
        lists
            .list_accessible(&administrator)
            .await
            .unwrap()
            .is_empty()
    );
}

fn item_id(value: &str) -> ItemId {
    ItemId::try_new(value).expect("valid test item id")
}
fn order(ids: &[&str]) -> ItemOrder {
    ItemOrder::try_new(ids.iter().map(|id| item_id(id)).collect()).expect("valid order")
}
fn ids(items: &[ShoppingItem]) -> Vec<&str> {
    items.iter().map(|item| item.id().as_str()).collect()
}
fn page(size: u32, after: Option<PageCursor>) -> PageRequest {
    PageRequest {
        size: PageSize::try_new(size).expect("valid page size"),
        after,
    }
}

/// Adds `count` items `{prefix}-0..` to an owned list and returns the resulting list version.
async fn add_items(
    items: &impl ShoppingItemRepository,
    owner: &UserId,
    list: &ListId,
    prefix: &str,
    count: usize,
    mut current: AggregateVersion,
) -> AggregateVersion {
    for index in 0..count {
        current = items
            .add_item(
                &editor(owner),
                list,
                current,
                item(&format!("{prefix}-{index}"), owner.as_str(), 0),
            )
            .await
            .expect("add item");
    }
    current
}

/// Runs reorder scenarios (roadmap B2): version check before no-op detection, complete
/// orders only, unique contiguous positions, item and list versions advance once.
pub async fn run_item_reorder_contract(
    lists: &impl ShoppingListRepository,
    items: &impl ShoppingItemRepository,
) {
    let owner = user("reorder-owner");
    let other = user("reorder-other");
    let target = list("reorder-list", "reorder-owner", "Sortieren");
    lists.create_personal(target.clone()).await.expect("create");
    let v4 = add_items(items, &owner, target.id(), "r", 3, version(1)).await;
    assert_eq!(v4, version(4));

    // Authorization precedes everything: outsiders cannot even learn about conflicts.
    assert_eq!(
        items
            .reorder_items(&editor(&other), target.id(), version(1), &order(&["x"]))
            .await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        items
            .reorder_items(&editor(&owner), &list_id("absent"), v4, &order(&[]))
            .await,
        Err(RepositoryError::NotFound)
    );
    // A stale version conflicts even for a no-op order.
    assert_eq!(
        items
            .reorder_items(
                &editor(&owner),
                target.id(),
                version(3),
                &order(&["r-0", "r-1", "r-2"])
            )
            .await,
        Err(RepositoryError::Conflict)
    );
    // Incomplete, unknown or extra ids mean the client's view is stale.
    for stale in [
        order(&["r-0", "r-1"]),
        order(&["r-0", "r-1", "r-9"]),
        order(&["r-0", "r-1", "r-2", "r-9"]),
    ] {
        assert_eq!(
            items
                .reorder_items(&editor(&owner), target.id(), v4, &stale)
                .await,
            Err(RepositoryError::Conflict)
        );
    }
    // A no-op order with the current version changes nothing.
    let (unchanged, same) = items
        .reorder_items(
            &editor(&owner),
            target.id(),
            v4,
            &order(&["r-0", "r-1", "r-2"]),
        )
        .await
        .expect("no-op reorder");
    assert_eq!(same, v4);
    assert!(unchanged.iter().all(|item| item.version() == 1));

    let (reordered, v5) = items
        .reorder_items(
            &editor(&owner),
            target.id(),
            v4,
            &order(&["r-2", "r-0", "r-1"]),
        )
        .await
        .expect("reorder");
    assert_eq!(v5, version(5));
    assert_eq!(ids(&reordered), vec!["r-2", "r-0", "r-1"]);
    assert_eq!(
        reordered
            .iter()
            .map(ShoppingItem::position)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert!(reordered.iter().all(|item| item.version() == 2));
    let stored = items
        .list_items(&reader(&owner), target.id())
        .await
        .unwrap();
    assert_eq!(stored, reordered);

    // Items moved by the reorder carry new versions: stale item writes conflict.
    assert_eq!(
        items
            .set_item_checked(&editor(&owner), target.id(), &item_id("r-2"), 1, true)
            .await,
        Err(RepositoryError::Conflict)
    );
    // New items still get a position not used by any existing item.
    let v6 = add_items(items, &owner, target.id(), "late", 1, v5).await;
    let after_add = items
        .list_items(&reader(&owner), target.id())
        .await
        .unwrap();
    let mut positions: Vec<_> = after_add.iter().map(ShoppingItem::position).collect();
    positions.dedup();
    assert_eq!(positions.len(), 4, "positions stay unique");
    assert_eq!(ids(&after_add)[3], "late-0");

    // Only moved items advance their version.
    let (partially, _) = items
        .reorder_items(
            &editor(&owner),
            target.id(),
            v6,
            &order(&["r-2", "r-0", "late-0", "r-1"]),
        )
        .await
        .expect("partial reorder");
    let versions: Vec<_> = partially.iter().map(ShoppingItem::version).collect();
    assert_eq!(versions, vec![2, 2, 2, 3]);
}

/// Runs cursor-pagination scenarios (roadmap B3): stable order, bounded pages, no duplicates
/// or gaps, opaque cursors bound to their query, authorization before cursor validation.
pub async fn run_pagination_contract(
    lists: &impl ShoppingListRepository,
    items: &impl ShoppingItemRepository,
) {
    let owner = user("page-owner");
    let other = user("page-other");
    for index in 0..5 {
        lists
            .create_personal(list(&format!("page-list-{index}"), "page-owner", "Seite"))
            .await
            .expect("create list");
    }
    lists
        .create_personal(list("page-foreign", "page-other", "Fremd"))
        .await
        .expect("create foreign");

    let mut seen = Vec::new();
    let mut cursor = None;
    let mut pages = 0;
    loop {
        let result = lists
            .list_accessible_page(&owner, &page(2, cursor))
            .await
            .expect("page lists");
        assert!(result.items.len() <= 2);
        seen.extend(
            result
                .items
                .iter()
                .map(|list| list.id().as_str().to_owned()),
        );
        pages += 1;
        match result.next {
            Some(next) => cursor = Some(next),
            None => break,
        }
        assert!(pages < 10, "pagination terminates");
    }
    let mut expected: Vec<_> = (0..5).map(|index| format!("page-list-{index}")).collect();
    let mut sorted_seen = seen.clone();
    sorted_seen.sort();
    sorted_seen.dedup();
    assert_eq!(sorted_seen.len(), seen.len(), "no duplicates across pages");
    expected.sort();
    assert_eq!(sorted_seen, expected, "only and all accessible lists");
    let foreign_page = lists
        .list_accessible_page(&other, &page(100, None))
        .await
        .unwrap();
    assert_eq!(ids_of_lists(&foreign_page.items), vec!["page-foreign"]);
    assert_eq!(foreign_page.next, None);
    assert_eq!(
        lists
            .list_accessible_page(
                &owner,
                &page(2, Some(PageCursor::try_new("not-a-cursor").unwrap()))
            )
            .await,
        Err(RepositoryError::InvalidCursor)
    );

    let target = list_id("page-list-0");
    add_items(items, &owner, &target, "p", 5, version(1)).await;
    let first = items
        .list_items_page(&reader(&owner), &target, &page(3, None))
        .await
        .expect("first item page");
    assert_eq!(ids(&first.items), vec!["p-0", "p-1", "p-2"]);
    let next = first.next.clone().expect("more items");
    let second = items
        .list_items_page(&reader(&owner), &target, &page(3, Some(next.clone())))
        .await
        .expect("second item page");
    assert_eq!(ids(&second.items), vec!["p-3", "p-4"]);
    assert_eq!(second.next, None);
    let exact = items
        .list_items_page(&reader(&owner), &target, &page(5, None))
        .await
        .unwrap();
    assert_eq!(exact.items.len(), 5);
    assert_eq!(exact.next, None, "no cursor without further entries");
    let empty = items
        .list_items_page(&reader(&owner), &list_id("page-list-1"), &page(5, None))
        .await
        .unwrap();
    assert!(empty.items.is_empty());
    assert_eq!(empty.next, None);

    // Cursors are bound to their list and query kind.
    assert_eq!(
        items
            .list_items_page(
                &reader(&owner),
                &list_id("page-list-1"),
                &page(3, Some(next.clone()))
            )
            .await,
        Err(RepositoryError::InvalidCursor)
    );
    assert_eq!(
        lists
            .list_accessible_page(&owner, &page(3, Some(next.clone())))
            .await,
        Err(RepositoryError::InvalidCursor)
    );
    // Outsiders get NotFound, even with garbage cursors: no existence probe via cursors.
    assert_eq!(
        items
            .list_items_page(
                &reader(&other),
                &target,
                &page(3, Some(PageCursor::try_new("zz").unwrap()))
            )
            .await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        items
            .list_items_page(&reader(&owner), &list_id("absent"), &page(3, None))
            .await,
        Err(RepositoryError::NotFound)
    );
}

fn ids_of_lists(lists: &[ShoppingList]) -> Vec<&str> {
    lists.iter().map(|list| list.id().as_str()).collect()
}

/// Client-side replica used to check that sync responses converge to the server state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Replica {
    version: u64,
    name: String,
    items: BTreeMap<String, ShoppingItem>,
}

impl Replica {
    fn apply(&mut self, sync: ListSync) {
        match sync {
            ListSync::Snapshot { list, items } => {
                self.version = list.version();
                self.name = list.name().as_str().to_owned();
                self.items = items
                    .into_iter()
                    .map(|item| (item.id().as_str().to_owned(), item))
                    .collect();
            }
            ListSync::Changes {
                list,
                since,
                changed,
                removed,
            } => {
                assert_eq!(since.as_u64(), self.version, "changes relative to replica");
                self.version = list.version();
                self.name = list.name().as_str().to_owned();
                for item_id in removed {
                    self.items.remove(item_id.as_str());
                }
                for item in changed {
                    self.items.insert(item.id().as_str().to_owned(), item);
                }
            }
        }
    }
}

async fn server_state(
    lists: &impl ShoppingListRepository,
    items: &impl ShoppingItemRepository,
    owner: &UserId,
    target: &ListId,
) -> Replica {
    let list = lists
        .list_accessible(owner)
        .await
        .unwrap()
        .into_iter()
        .find(|list| list.id() == target)
        .expect("list accessible");
    Replica {
        version: list.version(),
        name: list.name().as_str().to_owned(),
        items: items
            .list_items(&reader(owner), target)
            .await
            .unwrap()
            .into_iter()
            .map(|item| (item.id().as_str().to_owned(), item))
            .collect(),
    }
}

/// Runs version-based sync scenarios (roadmap B5): snapshots without a baseline, deltas that
/// converge to server state, tombstones for deletions, authorization first.
pub async fn run_sync_contract(
    lists: &impl ShoppingListRepository,
    items: &impl ShoppingItemRepository,
) {
    let owner = user("sync-owner");
    let other = user("sync-other");
    let target = list("sync-list", "sync-owner", "Sync");
    lists.create_personal(target.clone()).await.expect("create");
    let v3 = add_items(items, &owner, target.id(), "s", 2, version(1)).await;

    let mut replica = Replica::default();
    let snapshot = items
        .sync_list(&reader(&owner), target.id(), None)
        .await
        .expect("snapshot");
    assert!(matches!(snapshot, ListSync::Snapshot { .. }));
    replica.apply(snapshot);
    assert_eq!(
        replica,
        server_state(lists, items, &owner, target.id()).await
    );
    assert_eq!(replica.version, v3.as_u64());

    // Up to date: no item changes.
    match items
        .sync_list(&reader(&owner), target.id(), Some(v3))
        .await
        .unwrap()
    {
        ListSync::Changes {
            changed, removed, ..
        } => assert!(changed.is_empty() && removed.is_empty()),
        ListSync::Snapshot { .. } => {}
    }

    // Mutate: check, edit, delete, add, reorder, rename.
    let (_, v4) = items
        .set_item_checked(&editor(&owner), target.id(), &item_id("s-0"), 1, true)
        .await
        .unwrap();
    let v5 = items
        .delete_item(&editor(&owner), target.id(), &item_id("s-1"), 1)
        .await
        .unwrap();
    let mut at_v5 = Replica::default();
    at_v5.apply(
        items
            .sync_list(&reader(&owner), target.id(), None)
            .await
            .unwrap(),
    );
    assert_eq!(at_v5.version, v5.as_u64());
    let v6 = add_items(items, &owner, target.id(), "t", 1, v5).await;
    let (_, v7) = items
        .reorder_items(&editor(&owner), target.id(), v6, &order(&["t-0", "s-0"]))
        .await
        .unwrap();
    let renamed = lists
        .rename_list(
            &renamer(&owner),
            target.id(),
            v7,
            ListName::try_new("Sync neu").unwrap(),
        )
        .await
        .unwrap();
    assert!(v4 < renamed.aggregate_version());

    let delta = items
        .sync_list(&reader(&owner), target.id(), Some(v3))
        .await
        .unwrap();
    if let ListSync::Changes { removed, .. } = &delta {
        assert_eq!(removed, &vec![item_id("s-1")], "deleted items are reported");
    }
    replica.apply(delta);
    assert_eq!(
        replica,
        server_state(lists, items, &owner, target.id()).await
    );

    // A replica captured at an intermediate version converges as well.
    at_v5.apply(
        items
            .sync_list(&reader(&owner), target.id(), Some(v5))
            .await
            .unwrap(),
    );
    assert_eq!(at_v5, replica);
    let current = renamed.aggregate_version();

    // A baseline newer than the server forces a snapshot.
    assert!(matches!(
        items
            .sync_list(&reader(&owner), target.id(), Some(version(999)))
            .await
            .unwrap(),
        ListSync::Snapshot { .. }
    ));
    // Authorization first: outsiders and missing lists are indistinguishable.
    assert_eq!(
        items
            .sync_list(&reader(&other), target.id(), Some(v3))
            .await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        items
            .sync_list(&reader(&owner), &list_id("absent"), None)
            .await,
        Err(RepositoryError::NotFound)
    );
    // After deleting the list, sync no longer discloses it.
    lists
        .delete_list(&deleter(&owner), target.id(), current)
        .await
        .unwrap();
    assert_eq!(
        items.sync_list(&reader(&owner), target.id(), None).await,
        Err(RepositoryError::NotFound)
    );
}

/// Runs contributor/outsider scenarios for reorder, pagination and sync.
pub async fn run_shared_query_contract(
    lists: &impl ShoppingListRepository,
    items: &impl ShoppingItemRepository,
    households: &impl HouseholdStore,
) {
    let administrator = user("query-admin");
    let contributor = user("query-contributor");
    let outsider = user("query-outsider");
    let shared = provision_shared_list(
        households,
        "query-household",
        "query-shared",
        &administrator,
        &contributor,
    )
    .await;
    let v3 = add_items(items, &administrator, shared.id(), "q", 2, version(1)).await;

    let contributor_page = lists
        .list_accessible_page(&contributor, &page(10, None))
        .await
        .unwrap();
    assert_eq!(ids_of_lists(&contributor_page.items), vec!["query-shared"]);
    assert!(
        lists
            .list_accessible_page(&outsider, &page(10, None))
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(
        items
            .list_items_page(&reader(&contributor), shared.id(), &page(10, None))
            .await
            .unwrap()
            .items
            .len(),
        2
    );
    let (_, v4) = items
        .reorder_items(
            &editor(&contributor),
            shared.id(),
            v3,
            &order(&["q-1", "q-0"]),
        )
        .await
        .expect("contributors may reorder");
    assert!(matches!(
        items
            .sync_list(&reader(&contributor), shared.id(), Some(v3))
            .await
            .unwrap(),
        ListSync::Changes { .. } | ListSync::Snapshot { .. }
    ));
    // Insufficient permission is Forbidden for principals that can see the list ...
    let administer = ListAccessRequest::new(contributor.clone(), ListPermission::Administer);
    assert_eq!(
        items
            .reorder_items(&administer, shared.id(), v4, &order(&["q-0", "q-1"]))
            .await,
        Err(RepositoryError::Forbidden)
    );
    assert_eq!(
        items.sync_list(&administer, shared.id(), None).await,
        Err(RepositoryError::Forbidden)
    );
    assert_eq!(
        items
            .list_items_page(&administer, shared.id(), &page(1, None))
            .await,
        Err(RepositoryError::Forbidden)
    );
    // ... and NotFound for everyone else.
    assert_eq!(
        items
            .reorder_items(&editor(&outsider), shared.id(), v4, &order(&["q-0", "q-1"]))
            .await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        items.sync_list(&reader(&outsider), shared.id(), None).await,
        Err(RepositoryError::NotFound)
    );

    revoke_contributor(households, "query-household", &contributor).await;
    assert_eq!(
        items
            .sync_list(&reader(&contributor), shared.id(), Some(v4))
            .await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        items
            .list_items_page(&reader(&contributor), shared.id(), &page(1, None))
            .await,
        Err(RepositoryError::NotFound)
    );
    assert!(
        lists
            .list_accessible_page(&contributor, &page(10, None))
            .await
            .unwrap()
            .items
            .is_empty()
    );
}

/// Runs profile scenarios (roadmap B7): one profile per principal, optimistic concurrency
/// before no-op detection, no cross-principal access.
pub async fn run_profile_contract(profiles: &impl ProfileRepository) {
    let anna = user("profile-anna");
    let ben = user("profile-ben");
    let name = |value: &str| DisplayName::try_new(value).unwrap();
    assert_eq!(
        profiles.get_profile(&anna).await,
        Err(RepositoryError::NotFound)
    );
    let created = UserProfile::new(anna.clone(), name("Anna"));
    profiles
        .create_profile(created.clone())
        .await
        .expect("create");
    assert_eq!(
        profiles
            .create_profile(UserProfile::new(anna.clone(), name("Doppelt")))
            .await,
        Err(RepositoryError::AlreadyExists)
    );
    assert_eq!(profiles.get_profile(&anna).await, Ok(created.clone()));
    assert_eq!(
        profiles.get_profile(&ben).await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        profiles.update_profile(&ben, version(1), name("Ben")).await,
        Err(RepositoryError::NotFound)
    );

    // Stale versions conflict even for a no-op.
    let noop = profiles
        .update_profile(&anna, version(1), name("Anna"))
        .await
        .expect("no-op update");
    assert_eq!(noop.version(), version(1));
    assert_eq!(
        profiles
            .update_profile(&anna, version(2), name("Anna"))
            .await,
        Err(RepositoryError::Conflict)
    );
    let updated = profiles
        .update_profile(&anna, version(1), name("Anna B."))
        .await
        .expect("update");
    assert_eq!(updated.version(), version(2));
    assert_eq!(updated.display_name().as_str(), "Anna B.");
    assert_eq!(
        profiles
            .update_profile(&anna, version(1), name("Alt"))
            .await,
        Err(RepositoryError::Conflict)
    );
    assert_eq!(profiles.get_profile(&anna).await, Ok(updated));

    // Another principal's profile is independent.
    profiles
        .create_profile(UserProfile::new(ben.clone(), name("Ben")))
        .await
        .expect("create ben");
    assert_eq!(
        profiles
            .get_profile(&anna)
            .await
            .unwrap()
            .display_name()
            .as_str(),
        "Anna B."
    );
}

fn admin(principal: &UserId, operation: HouseholdOperation) -> HouseholdAccessRequest {
    household_access(principal, operation)
}

/// Runs household scenarios (roadmap C2): creation and listing, owner/member/outsider
/// authorization, optimistic concurrency, household-owned lists and the per-user household
/// and per-household list limits.
pub async fn run_household_contract(
    lists: &impl ShoppingListRepository,
    items: &impl ShoppingItemRepository,
    households: &impl HouseholdStore,
) {
    let owner = user("hh-owner");
    let member = user("hh-member");
    let outsider = user("hh-outsider");
    let created = provision_household(households, "hh-1", "hh-owner", &["hh-member"]).await;
    // One accepted membership advanced the household version.
    assert_eq!(created.version(), version(2));
    assert_eq!(
        households
            .create_household(household("hh-1", "hh-outsider", "Kopie"))
            .await,
        Err(HouseholdRepositoryError::AlreadyExists)
    );

    let owned = households.list_households(&owner).await.unwrap();
    assert_eq!(owned.len(), 1);
    assert_eq!(owned[0].household, created);
    assert_eq!(owned[0].role, einkaufio_domain::HouseholdRole::Owner);
    let joined = households.list_households(&member).await.unwrap();
    assert_eq!(joined[0].role, einkaufio_domain::HouseholdRole::Member);
    assert!(
        households
            .list_households(&outsider)
            .await
            .unwrap()
            .is_empty()
    );

    let details = households
        .get_household(&admin(&member, HouseholdOperation::View), created.id())
        .await
        .unwrap();
    let roster: Vec<_> = details
        .members
        .iter()
        .map(|entry| (entry.user_id.as_str(), entry.role))
        .collect();
    assert_eq!(
        roster,
        vec![
            ("hh-member", einkaufio_domain::HouseholdRole::Member),
            ("hh-owner", einkaufio_domain::HouseholdRole::Owner),
        ]
    );
    // Outsiders cannot tell an existing household from a missing one.
    for id in [created.id().clone(), household_id("hh-missing")] {
        assert_eq!(
            households
                .get_household(&admin(&outsider, HouseholdOperation::View), &id)
                .await,
            Err(HouseholdRepositoryError::NotFound)
        );
        assert_eq!(
            households
                .rename_household(
                    &admin(&outsider, HouseholdOperation::Rename),
                    &id,
                    version(2),
                    HouseholdName::try_new("X").unwrap()
                )
                .await,
            Err(HouseholdRepositoryError::NotFound)
        );
    }

    // Members cannot administer, even with a stale version (authorization first). Adapters
    // enforce the requested permission generically.
    for expected in [version(2), version(1)] {
        assert_eq!(
            households
                .rename_household(
                    &admin(&member, HouseholdOperation::Rename),
                    created.id(),
                    expected,
                    HouseholdName::try_new("Übernommen").unwrap()
                )
                .await,
            Err(HouseholdRepositoryError::Forbidden)
        );
    }
    assert_eq!(
        households
            .get_household(
                &HouseholdAccessRequest::new(member.clone(), HouseholdPermission::Administer),
                created.id()
            )
            .await,
        Err(HouseholdRepositoryError::Forbidden)
    );
    // Stale versions conflict before no-op detection.
    assert_eq!(
        households
            .rename_household(
                &admin(&owner, HouseholdOperation::Rename),
                created.id(),
                version(1),
                HouseholdName::try_new("Haushalt").unwrap()
            )
            .await,
        Err(HouseholdRepositoryError::Conflict)
    );
    let renamed = households
        .rename_household(
            &admin(&owner, HouseholdOperation::Rename),
            created.id(),
            version(2),
            HouseholdName::try_new("Familie").unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(renamed.version(), version(3));
    assert_eq!(renamed.name().as_str(), "Familie");

    // Household lists: members may create them; outsiders cannot; personal lists are refused.
    let by_member = household_list("hh-list-member", "hh-1", "Drogerie");
    households
        .create_household_list(
            &admin(&member, HouseholdOperation::CreateList),
            by_member.clone(),
        )
        .await
        .unwrap();
    assert_eq!(
        households
            .create_household_list(
                &admin(&member, HouseholdOperation::CreateList),
                by_member.clone()
            )
            .await,
        Err(HouseholdRepositoryError::AlreadyExists)
    );
    assert_eq!(
        households
            .create_household_list(
                &admin(&outsider, HouseholdOperation::CreateList),
                household_list("hh-list-outsider", "hh-1", "X")
            )
            .await,
        Err(HouseholdRepositoryError::NotFound)
    );
    assert_eq!(
        households
            .create_household_list(
                &admin(&owner, HouseholdOperation::CreateList),
                list("hh-list-personal", "hh-owner", "Privat")
            )
            .await,
        Err(HouseholdRepositoryError::InvalidRequest)
    );
    let personal = list("hh-personal", "hh-member", "Privat");
    lists.create_personal(personal.clone()).await.unwrap();

    // Household lists are visible to every member, personal lists only to their owner.
    assert_eq!(
        sorted(lists.list_accessible(&member).await.unwrap()),
        vec![by_member.clone(), personal.clone()]
    );
    assert_eq!(
        lists.list_accessible(&owner).await.unwrap(),
        vec![by_member.clone()]
    );
    assert!(lists.list_accessible(&outsider).await.unwrap().is_empty());
    // The household owner administers household lists; members contribute.
    assert_eq!(
        items
            .add_item(
                &editor(&owner),
                by_member.id(),
                version(1),
                item("hh-item", "hh-owner", 0)
            )
            .await,
        Ok(version(2))
    );
    assert_eq!(
        lists
            .rename_list(
                &renamer(&member),
                by_member.id(),
                version(2),
                ListName::try_new("X").unwrap()
            )
            .await,
        Err(RepositoryError::Forbidden)
    );
    assert_eq!(
        lists
            .rename_list(
                &renamer(&owner),
                by_member.id(),
                version(2),
                ListName::try_new("Drogeriemarkt").unwrap()
            )
            .await
            .unwrap()
            .version(),
        3
    );
    // Household ownership never grants access to a member's personal list.
    assert_eq!(
        items.list_items(&reader(&owner), personal.id()).await,
        Err(RepositoryError::NotFound)
    );

    // Per-household list limit (20), including lists created by members.
    for index in 1..Household::MAX_LISTS {
        households
            .create_household_list(
                &admin(&owner, HouseholdOperation::CreateList),
                household_list(&format!("hh-list-{index}"), "hh-1", "Liste"),
            )
            .await
            .unwrap();
    }
    assert_eq!(
        households
            .create_household_list(
                &admin(&member, HouseholdOperation::CreateList),
                household_list("hh-list-over", "hh-1", "Zu viel")
            )
            .await,
        Err(HouseholdRepositoryError::ListLimitReached)
    );
    // Deleting a list frees a slot.
    lists
        .delete_list(&deleter(&owner), by_member.id(), version(3))
        .await
        .unwrap();
    households
        .create_household_list(
            &admin(&member, HouseholdOperation::CreateList),
            household_list("hh-list-over", "hh-1", "Wieder Platz"),
        )
        .await
        .unwrap();

    // Per-user household limit counts owned and joined households.
    for index in 1..Household::MAX_HOUSEHOLDS_PER_USER {
        households
            .create_household(household(
                &format!("hh-extra-{index}"),
                "hh-member",
                "Weitere",
            ))
            .await
            .unwrap();
    }
    assert_eq!(
        households
            .create_household(household("hh-extra-over", "hh-member", "Zu viel"))
            .await,
        Err(HouseholdRepositoryError::HouseholdLimitReached)
    );
    assert_eq!(
        households.list_households(&member).await.unwrap().len(),
        Household::MAX_HOUSEHOLDS_PER_USER
    );
}

/// Runs membership lifecycle scenarios (roadmap C3): leaving, removal, owner transfer before
/// owner departure, and household deletion cascading to lists, items and invitations.
pub async fn run_membership_lifecycle_contract(
    lists: &impl ShoppingListRepository,
    items: &impl ShoppingItemRepository,
    households: &impl HouseholdStore,
) {
    let owner = user("life-owner");
    let member = user("life-member");
    let second = user("life-second");
    let outsider = user("life-outsider");
    let home = provision_household(
        households,
        "life-1",
        "life-owner",
        &["life-member", "life-second"],
    )
    .await;
    assert_eq!(home.version(), version(3));
    let shared = household_list("life-list", "life-1", "Gemeinsam");
    households
        .create_household_list(
            &admin(&owner, HouseholdOperation::CreateList),
            shared.clone(),
        )
        .await
        .unwrap();

    // The owner cannot leave or be removed before transferring ownership.
    assert_eq!(
        households.leave_household(&owner, home.id()).await,
        Err(HouseholdRepositoryError::OwnerMustTransferOwnership)
    );
    assert_eq!(
        households
            .remove_member(
                &admin(&owner, HouseholdOperation::RemoveMember),
                home.id(),
                version(3),
                &owner
            )
            .await,
        Err(HouseholdRepositoryError::OwnerMustTransferOwnership)
    );
    assert_eq!(
        households.leave_household(&outsider, home.id()).await,
        Err(HouseholdRepositoryError::NotFound)
    );

    // Members cannot transfer or remove; outsiders learn nothing.
    assert_eq!(
        households
            .transfer_ownership(
                &admin(&member, HouseholdOperation::TransferOwnership),
                home.id(),
                version(1),
                &member
            )
            .await,
        Err(HouseholdRepositoryError::Forbidden)
    );
    assert_eq!(
        households
            .remove_member(
                &admin(&member, HouseholdOperation::RemoveMember),
                home.id(),
                version(3),
                &second
            )
            .await,
        Err(HouseholdRepositoryError::Forbidden)
    );
    assert_eq!(
        households
            .transfer_ownership(
                &admin(&outsider, HouseholdOperation::TransferOwnership),
                home.id(),
                version(3),
                &outsider
            )
            .await,
        Err(HouseholdRepositoryError::NotFound)
    );
    // Transfer targets must be active members; stale versions conflict first.
    assert_eq!(
        households
            .transfer_ownership(
                &admin(&owner, HouseholdOperation::TransferOwnership),
                home.id(),
                version(3),
                &outsider
            )
            .await,
        Err(HouseholdRepositoryError::NotFound)
    );
    assert_eq!(
        households
            .transfer_ownership(
                &admin(&owner, HouseholdOperation::TransferOwnership),
                home.id(),
                version(2),
                &member
            )
            .await,
        Err(HouseholdRepositoryError::Conflict)
    );

    let transferred = households
        .transfer_ownership(
            &admin(&owner, HouseholdOperation::TransferOwnership),
            home.id(),
            version(3),
            &member,
        )
        .await
        .unwrap();
    assert_eq!(transferred.owner_id(), &member);
    assert_eq!(transferred.version(), version(4));
    // List administration follows household ownership immediately.
    assert_eq!(
        lists
            .rename_list(
                &renamer(&owner),
                shared.id(),
                version(1),
                ListName::try_new("Alt").unwrap()
            )
            .await,
        Err(RepositoryError::Forbidden)
    );
    assert_eq!(
        lists
            .rename_list(
                &renamer(&member),
                shared.id(),
                version(1),
                ListName::try_new("Neu").unwrap()
            )
            .await
            .unwrap()
            .version(),
        2
    );
    assert_eq!(
        households
            .rename_household(
                &admin(&owner, HouseholdOperation::Rename),
                home.id(),
                version(4),
                HouseholdName::try_new("X").unwrap()
            )
            .await,
        Err(HouseholdRepositoryError::Forbidden)
    );

    // The former owner is now a member and may leave; access ends with the same write.
    let after_leave = households.leave_household(&owner, home.id()).await.unwrap();
    assert_eq!(after_leave.version(), version(5));
    assert!(lists.list_accessible(&owner).await.unwrap().is_empty());
    assert_eq!(
        items.list_items(&reader(&owner), shared.id()).await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        households
            .get_household(&admin(&owner, HouseholdOperation::View), home.id())
            .await,
        Err(HouseholdRepositoryError::NotFound)
    );
    assert_eq!(
        households.leave_household(&owner, home.id()).await,
        Err(HouseholdRepositoryError::NotFound)
    );

    // The new owner removes a member; removing a non-member is NotFound.
    assert_eq!(
        households
            .remove_member(
                &admin(&member, HouseholdOperation::RemoveMember),
                home.id(),
                version(4),
                &second
            )
            .await,
        Err(HouseholdRepositoryError::Conflict)
    );
    let after_removal = households
        .remove_member(
            &admin(&member, HouseholdOperation::RemoveMember),
            home.id(),
            version(5),
            &second,
        )
        .await
        .unwrap();
    assert_eq!(after_removal.version(), version(6));
    assert!(lists.list_accessible(&second).await.unwrap().is_empty());
    assert_eq!(
        households
            .remove_member(
                &admin(&member, HouseholdOperation::RemoveMember),
                home.id(),
                version(6),
                &second
            )
            .await,
        Err(HouseholdRepositoryError::NotFound)
    );

    // Deletion: members are Forbidden, stale versions conflict, outsiders get NotFound.
    let full = provision_household(
        households,
        "life-full",
        "life-member",
        &["life-second", "life-third"],
    )
    .await;
    assert_eq!(
        households
            .delete_household(
                &admin(&second, HouseholdOperation::Delete),
                full.id(),
                full.version()
            )
            .await,
        Err(HouseholdRepositoryError::Forbidden)
    );
    assert_eq!(
        households
            .delete_household(
                &admin(&outsider, HouseholdOperation::Delete),
                full.id(),
                full.version()
            )
            .await,
        Err(HouseholdRepositoryError::NotFound)
    );
    assert_eq!(
        households
            .delete_household(
                &admin(&member, HouseholdOperation::Delete),
                full.id(),
                version(1)
            )
            .await,
        Err(HouseholdRepositoryError::Conflict)
    );

    // Fill the household to its list limit, with items, plus a pending invitation, then
    // delete it: everything becomes inaccessible at once and the limit slots are freed.
    let mut full_lists = Vec::new();
    for index in 0..Household::MAX_LISTS {
        let list = household_list(&format!("life-full-{index}"), "life-full", "Liste");
        households
            .create_household_list(
                &admin(&second, HouseholdOperation::CreateList),
                list.clone(),
            )
            .await
            .unwrap();
        items
            .add_item(
                &editor(&second),
                list.id(),
                version(1),
                item(&format!("life-full-item-{index}"), "life-second", 0),
            )
            .await
            .unwrap();
        full_lists.push(list);
    }
    households
        .create_invitation(
            &admin(&member, HouseholdOperation::ManageInvitations),
            invitation(
                "life-pending",
                "life-full",
                "life-member",
                "life-pending",
                5,
            ),
            at(NOW),
        )
        .await
        .unwrap();
    households
        .delete_household(
            &admin(&member, HouseholdOperation::Delete),
            full.id(),
            full.version(),
        )
        .await
        .unwrap();

    for principal in [&member, &second, &user("life-third")] {
        let remaining = lists.list_accessible(principal).await.unwrap();
        assert!(
            remaining
                .iter()
                .all(|list| list.household_id() != Some(full.id())),
            "deleted household lists must disappear"
        );
        assert!(
            households
                .list_households(principal)
                .await
                .unwrap()
                .iter()
                .all(|summary| summary.household.id() != full.id())
        );
    }
    for list in &full_lists {
        assert_eq!(
            items.list_items(&reader(&second), list.id()).await,
            Err(RepositoryError::NotFound)
        );
        assert_eq!(
            lists
                .delete_list(&deleter(&member), list.id(), version(2))
                .await,
            Err(RepositoryError::NotFound)
        );
    }
    assert_eq!(
        households
            .get_household(&admin(&member, HouseholdOperation::View), full.id())
            .await,
        Err(HouseholdRepositoryError::NotFound)
    );
    assert_eq!(
        households
            .accept_invitation(&outsider, &token_hash("life-pending"), at(NOW))
            .await,
        Err(HouseholdRepositoryError::InvitationUnavailable)
    );
    assert_eq!(
        households
            .delete_household(
                &admin(&member, HouseholdOperation::Delete),
                full.id(),
                full.version()
            )
            .await,
        Err(HouseholdRepositoryError::NotFound)
    );
    // The id can be reused and the members' household slots were released.
    households
        .create_household(household("life-full", "life-third", "Neu"))
        .await
        .unwrap();
    assert!(
        lists
            .list_accessible(&user("life-third"))
            .await
            .unwrap()
            .is_empty()
    );
}

/// Runs invitation scenarios (roadmap C4): owner-only management, the valid-invitation
/// limit, expiry/revocation/use-count enforcement, authenticated acceptance and membership
/// limits. Contracts handle only token hashes, never raw tokens.
pub async fn run_invitation_contract(
    lists: &impl ShoppingListRepository,
    households: &impl HouseholdStore,
) {
    let owner = user("inv-owner");
    let member = user("inv-member");
    let outsider = user("inv-outsider");
    let home = provision_household(households, "inv-1", "inv-owner", &["inv-member"]).await;
    let manage = |principal: &UserId| admin(principal, HouseholdOperation::ManageInvitations);
    let shared = household_list("inv-list", "inv-1", "Gemeinsam");
    households
        .create_household_list(
            &admin(&owner, HouseholdOperation::CreateList),
            shared.clone(),
        )
        .await
        .unwrap();

    // Only the owner manages invitations; the creator must be the verified principal.
    assert_eq!(
        households
            .create_invitation(
                &manage(&member),
                invitation("inv-m", "inv-1", "inv-member", "inv-m", 1),
                at(NOW)
            )
            .await,
        Err(HouseholdRepositoryError::Forbidden)
    );
    assert_eq!(
        households
            .create_invitation(
                &manage(&outsider),
                invitation("inv-o", "inv-1", "inv-outsider", "inv-o", 1),
                at(NOW)
            )
            .await,
        Err(HouseholdRepositoryError::NotFound)
    );
    assert_eq!(
        households
            .create_invitation(
                &manage(&owner),
                invitation("inv-spoof", "inv-1", "inv-member", "inv-spoof", 1),
                at(NOW)
            )
            .await,
        Err(HouseholdRepositoryError::InvalidRequest)
    );
    assert_eq!(
        households
            .list_invitations(&manage(&member), home.id())
            .await,
        Err(HouseholdRepositoryError::Forbidden)
    );

    // Two uses: two different users join, the third attempt finds it used up.
    households
        .create_invitation(
            &manage(&owner),
            invitation("inv-a", "inv-1", "inv-owner", "inv-a", 2),
            at(NOW),
        )
        .await
        .unwrap();
    assert_eq!(
        households
            .create_invitation(
                &manage(&owner),
                invitation("inv-a", "inv-1", "inv-owner", "inv-a-2", 2),
                at(NOW)
            )
            .await,
        Err(HouseholdRepositoryError::AlreadyExists)
    );
    // Existing members are refused without consuming a use.
    assert_eq!(
        households
            .accept_invitation(&member, &token_hash("inv-a"), at(NOW))
            .await,
        Err(HouseholdRepositoryError::AlreadyMember)
    );
    let joined = households
        .accept_invitation(&user("inv-joiner-1"), &token_hash("inv-a"), at(NOW))
        .await
        .unwrap();
    assert_eq!(joined.role, einkaufio_domain::HouseholdRole::Member);
    assert_eq!(
        joined.household.version(),
        home.version().checked_next().unwrap()
    );
    // Acceptance grants access to household lists immediately.
    assert_eq!(
        lists.list_accessible(&user("inv-joiner-1")).await.unwrap(),
        vec![shared.clone()]
    );
    households
        .accept_invitation(&user("inv-joiner-2"), &token_hash("inv-a"), at(NOW))
        .await
        .unwrap();
    assert_eq!(
        households
            .accept_invitation(&user("inv-joiner-3"), &token_hash("inv-a"), at(NOW))
            .await,
        Err(HouseholdRepositoryError::InvitationUnavailable)
    );
    let stored = households
        .list_invitations(&manage(&owner), home.id())
        .await
        .unwrap();
    let used = stored
        .iter()
        .find(|candidate| candidate.id().as_str() == "inv-a")
        .unwrap();
    assert_eq!(used.used_count(), 2);
    assert_eq!(used.token_hash(), &token_hash("inv-a"));

    // Unknown, expired and revoked tokens are indistinguishable.
    households
        .create_invitation(
            &manage(&owner),
            invitation("inv-b", "inv-1", "inv-owner", "inv-b", 5),
            at(NOW),
        )
        .await
        .unwrap();
    for (seed, now) in [
        ("inv-unknown", NOW),
        ("inv-b", EXPIRES),
        ("inv-b", EXPIRES + 1),
    ] {
        assert_eq!(
            households
                .accept_invitation(&outsider, &token_hash(seed), at(now))
                .await,
            Err(HouseholdRepositoryError::InvitationUnavailable)
        );
    }
    assert_eq!(
        households
            .revoke_invitation(
                &manage(&member),
                home.id(),
                &InvitationId::try_new("inv-b").unwrap()
            )
            .await,
        Err(HouseholdRepositoryError::Forbidden)
    );
    let revoked = households
        .revoke_invitation(
            &manage(&owner),
            home.id(),
            &InvitationId::try_new("inv-b").unwrap(),
        )
        .await
        .unwrap();
    assert!(revoked.is_revoked());
    assert_eq!(
        households
            .accept_invitation(&outsider, &token_hash("inv-b"), at(NOW))
            .await,
        Err(HouseholdRepositoryError::InvitationUnavailable)
    );
    assert!(lists.list_accessible(&outsider).await.unwrap().is_empty());

    // Invitations of another household cannot be revoked through this one.
    provision_household(households, "inv-2", "inv-outsider", &[]).await;
    households
        .create_invitation(
            &admin(&outsider, HouseholdOperation::ManageInvitations),
            invitation("inv-foreign", "inv-2", "inv-outsider", "inv-foreign", 1),
            at(NOW),
        )
        .await
        .unwrap();
    assert_eq!(
        households
            .revoke_invitation(
                &manage(&owner),
                home.id(),
                &InvitationId::try_new("inv-foreign").unwrap()
            )
            .await,
        Err(HouseholdRepositoryError::NotFound)
    );

    // At most five simultaneously valid invitations; used-up, revoked and expired ones do
    // not count. inv-a (used up) and inv-b (revoked) are not valid.
    for index in 0..Invitation::MAX_VALID_PER_HOUSEHOLD {
        households
            .create_invitation(
                &manage(&owner),
                invitation(
                    &format!("inv-c{index}"),
                    "inv-1",
                    "inv-owner",
                    &format!("inv-c{index}"),
                    1,
                ),
                at(NOW),
            )
            .await
            .unwrap();
    }
    assert_eq!(
        households
            .create_invitation(
                &manage(&owner),
                invitation("inv-over", "inv-1", "inv-owner", "inv-over", 1),
                at(NOW)
            )
            .await,
        Err(HouseholdRepositoryError::InvitationLimitReached)
    );
    households
        .create_invitation(
            &manage(&owner),
            invitation("inv-later", "inv-1", "inv-owner", "inv-later", 1),
            at(EXPIRES),
        )
        .await
        .expect("expired invitations no longer count");

    // Member limit (10 including the owner): the household has owner, member, two joiners.
    let mut joined_count = 4;
    for index in 0..Invitation::MAX_VALID_PER_HOUSEHOLD {
        households
            .accept_invitation(
                &user(&format!("inv-fill-{index}")),
                &token_hash(&format!("inv-c{index}")),
                at(NOW),
            )
            .await
            .unwrap();
        joined_count += 1;
    }
    assert_eq!(joined_count, 9);
    households
        .revoke_invitation(
            &manage(&owner),
            home.id(),
            &InvitationId::try_new("inv-later").unwrap(),
        )
        .await
        .unwrap();
    households
        .create_invitation(
            &manage(&owner),
            invitation("inv-last", "inv-1", "inv-owner", "inv-last", 3),
            at(NOW),
        )
        .await
        .unwrap();
    households
        .accept_invitation(&user("inv-tenth"), &token_hash("inv-last"), at(NOW))
        .await
        .unwrap();
    assert_eq!(
        households
            .accept_invitation(&user("inv-eleventh"), &token_hash("inv-last"), at(NOW))
            .await,
        Err(HouseholdRepositoryError::MemberLimitReached)
    );
    assert_eq!(
        households
            .get_household(&admin(&owner, HouseholdOperation::View), home.id())
            .await
            .unwrap()
            .members
            .len(),
        Household::MAX_MEMBERS
    );

    // Per-user household limit applies to acceptance too.
    let busy = user("inv-busy");
    for index in 0..Household::MAX_HOUSEHOLDS_PER_USER {
        households
            .create_household(household(&format!("inv-busy-{index}"), "inv-busy", "Voll"))
            .await
            .unwrap();
    }
    households
        .create_invitation(
            &admin(&outsider, HouseholdOperation::ManageInvitations),
            invitation("inv-busy", "inv-2", "inv-outsider", "inv-busy", 1),
            at(NOW),
        )
        .await
        .unwrap();
    assert_eq!(
        households
            .accept_invitation(&busy, &token_hash("inv-busy"), at(NOW))
            .await,
        Err(HouseholdRepositoryError::HouseholdLimitReached)
    );
    // The refused acceptance did not consume the use.
    households
        .accept_invitation(&user("inv-free"), &token_hash("inv-busy"), at(NOW))
        .await
        .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::in_memory::InMemoryRepository;

    #[tokio::test]
    async fn in_memory_adapter_passes_list_contract() {
        let repository = InMemoryRepository::new();
        run_shopping_list_contract(&repository, &repository).await;
    }

    #[tokio::test]
    async fn in_memory_adapter_passes_item_contract() {
        let repository = InMemoryRepository::new();
        run_shopping_item_contract(&repository, &repository).await;
    }

    #[tokio::test]
    async fn in_memory_adapter_passes_shared_access_contract() {
        let repository = InMemoryRepository::new();
        run_shared_access_contract(&repository, &repository, &repository).await;
    }

    #[tokio::test]
    async fn in_memory_adapter_passes_household_contract() {
        let repository = InMemoryRepository::new();
        run_household_contract(&repository, &repository, &repository).await;
    }

    #[tokio::test]
    async fn in_memory_adapter_passes_membership_lifecycle_contract() {
        let repository = InMemoryRepository::new();
        run_membership_lifecycle_contract(&repository, &repository, &repository).await;
    }

    #[tokio::test]
    async fn in_memory_adapter_passes_invitation_contract() {
        let repository = InMemoryRepository::new();
        run_invitation_contract(&repository, &repository).await;
    }

    #[tokio::test]
    async fn default_household_and_invitation_operations_are_fail_closed() {
        let owner = user("owner");
        let id = household_id("household");
        let access = household_access(&owner, HouseholdOperation::View);
        let unavailable = Err::<(), _>(HouseholdRepositoryError::Unavailable);
        assert_eq!(
            Unimplemented
                .create_household(household("household", "owner", "X"))
                .await,
            unavailable
        );
        assert!(Unimplemented.list_households(&owner).await.is_err());
        assert!(Unimplemented.get_household(&access, &id).await.is_err());
        assert!(
            Unimplemented
                .rename_household(
                    &access,
                    &id,
                    version(1),
                    HouseholdName::try_new("X").unwrap()
                )
                .await
                .is_err()
        );
        assert_eq!(
            Unimplemented
                .create_household_list(&access, household_list("l", "household", "X"))
                .await,
            unavailable
        );
        assert!(Unimplemented.leave_household(&owner, &id).await.is_err());
        assert!(
            Unimplemented
                .remove_member(&access, &id, version(1), &owner)
                .await
                .is_err()
        );
        assert!(
            Unimplemented
                .transfer_ownership(&access, &id, version(1), &owner)
                .await
                .is_err()
        );
        assert_eq!(
            Unimplemented
                .delete_household(&access, &id, version(1))
                .await,
            unavailable
        );
        assert_eq!(
            Unimplemented
                .create_invitation(
                    &access,
                    invitation("i", "household", "owner", "i", 1),
                    at(NOW)
                )
                .await,
            unavailable
        );
        assert!(Unimplemented.list_invitations(&access, &id).await.is_err());
        assert!(
            Unimplemented
                .revoke_invitation(&access, &id, &InvitationId::try_new("i").unwrap())
                .await
                .is_err()
        );
        assert_eq!(
            Unimplemented
                .accept_invitation(&owner, &token_hash("i"), at(NOW))
                .await
                .map(|_| ()),
            unavailable
        );
    }

    impl HouseholdRepository for Unimplemented {}
    impl InvitationRepository for Unimplemented {}

    #[tokio::test]
    async fn in_memory_adapter_passes_reorder_contract() {
        let repository = InMemoryRepository::new();
        run_item_reorder_contract(&repository, &repository).await;
    }

    #[tokio::test]
    async fn in_memory_adapter_passes_pagination_contract() {
        let repository = InMemoryRepository::new();
        run_pagination_contract(&repository, &repository).await;
    }

    #[tokio::test]
    async fn in_memory_adapter_passes_sync_contract() {
        let repository = InMemoryRepository::new();
        run_sync_contract(&repository, &repository).await;
    }

    #[tokio::test]
    async fn in_memory_adapter_passes_shared_query_contract() {
        let repository = InMemoryRepository::new();
        run_shared_query_contract(&repository, &repository, &repository).await;
    }

    #[tokio::test]
    async fn in_memory_adapter_passes_profile_contract() {
        let repository = InMemoryRepository::new();
        run_profile_contract(&repository).await;
    }

    #[tokio::test]
    async fn default_query_and_reorder_operations_are_fail_closed() {
        let owner = user("owner");
        assert_eq!(
            Unimplemented
                .list_accessible_page(&owner, &page(1, None))
                .await,
            Err(RepositoryError::Unavailable)
        );
        assert_eq!(
            Unimplemented
                .list_items_page(&reader(&owner), &list_id("list"), &page(1, None))
                .await,
            Err(RepositoryError::Unavailable)
        );
        assert_eq!(
            Unimplemented
                .reorder_items(&editor(&owner), &list_id("list"), version(1), &order(&[]))
                .await,
            Err(RepositoryError::Unavailable)
        );
        assert_eq!(
            Unimplemented
                .sync_list(&reader(&owner), &list_id("list"), None)
                .await,
            Err(RepositoryError::Unavailable)
        );
    }

    struct Unimplemented;

    impl ShoppingListRepository for Unimplemented {
        async fn create_personal(&self, _: ShoppingList) -> Result<(), RepositoryError> {
            Err(RepositoryError::Unavailable)
        }
    }

    impl ShoppingItemRepository for Unimplemented {
        async fn add_item(
            &self,
            _: &ListAccessRequest,
            _: &ListId,
            _: AggregateVersion,
            _: ShoppingItem,
        ) -> Result<AggregateVersion, RepositoryError> {
            Err(RepositoryError::Unavailable)
        }
        async fn set_item_checked(
            &self,
            _: &ListAccessRequest,
            _: &ListId,
            _: &ItemId,
            _: u64,
            _: bool,
        ) -> Result<(ShoppingItem, AggregateVersion), RepositoryError> {
            Err(RepositoryError::Unavailable)
        }
        async fn delete_item(
            &self,
            _: &ListAccessRequest,
            _: &ListId,
            _: &ItemId,
            _: u64,
        ) -> Result<AggregateVersion, RepositoryError> {
            Err(RepositoryError::Unavailable)
        }
    }

    #[tokio::test]
    async fn default_list_operations_are_fail_closed() {
        let owner = user("owner");
        assert_eq!(
            Unimplemented.list_accessible(&owner).await,
            Err(RepositoryError::Unavailable)
        );
        assert_eq!(
            Unimplemented
                .rename_list(
                    &renamer(&owner),
                    &list_id("list"),
                    version(1),
                    ListName::try_new("X").unwrap()
                )
                .await,
            Err(RepositoryError::Unavailable)
        );
        assert_eq!(
            Unimplemented
                .delete_list(&deleter(&owner), &list_id("list"), version(1))
                .await,
            Err(RepositoryError::Unavailable)
        );
    }

    #[tokio::test]
    async fn default_item_read_and_detail_update_are_fail_closed() {
        let owner = user("owner");
        assert_eq!(
            Unimplemented
                .list_items(&reader(&owner), &list_id("list"))
                .await,
            Err(RepositoryError::Unavailable)
        );
        let details = item("fail-closed", "owner", 0).details();
        assert_eq!(
            Unimplemented
                .update_item_details(
                    &editor(&owner),
                    &list_id("list"),
                    &ItemId::try_new("fail-closed").unwrap(),
                    1,
                    details
                )
                .await,
            Err(RepositoryError::Unavailable)
        );
    }
}

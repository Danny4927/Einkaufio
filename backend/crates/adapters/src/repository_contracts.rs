//! Reusable repository behavior contracts.
//!
//! Enable the `test-support` feature to reuse these scenarios from adapter integration tests.
//! Each contract provisions its own unique fixtures and expects a fresh repository pair.
//! Scenarios are async (ADR 014) and need no specific runtime; adapters drive them from their
//! own async tests. Access scoping follows ADR 015.

use std::future::Future;

use std::collections::BTreeMap;

use einkaufio_application::{
    ListAccessRequest, ListSync, PageCursor, PageRequest, PageSize, ProfileRepository,
    RepositoryError, ShoppingItemRepository, ShoppingListRepository,
};
use einkaufio_domain::{
    AggregateVersion, DisplayName, ItemDetails, ItemId, ItemName, ItemNote, ItemOrder, ListId,
    ListName, ListOperation, ListPermission, ShoppingItem, ShoppingList, UserId, UserProfile,
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

/// Provisions lists on which a principal holds contributor (non-ownership) access.
///
/// Until households exist the in-memory adapter grants contributor access directly. From
/// roadmap C2 a persistent adapter implements this through the production access path:
/// create a household administered by `list.owner_id()`, add `contributor` as an active member
/// and create `list` as a household-owned list. Implementations must not add an access path
/// that production code cannot reach.
pub trait SharedListFixture: Send + Sync {
    /// Persists `list`, administered by `list.owner_id()`, with `contributor` as contributor.
    fn provision_shared_list(
        &self,
        list: ShoppingList,
        contributor: &UserId,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;

    /// Removes the contributor's access (in C2: ends the household membership).
    fn revoke_contributor(
        &self,
        list_id: &ListId,
        contributor: &UserId,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;
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
    fixture: &impl SharedListFixture,
) {
    let administrator = user("shared-admin");
    let contributor = user("shared-contributor");
    let outsider = user("shared-outsider");
    let shared = list("shared-list", "shared-admin", "Gemeinsam");
    fixture
        .provision_shared_list(shared.clone(), &contributor)
        .await
        .expect("provision shared list");

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
    fixture
        .revoke_contributor(shared.id(), &contributor)
        .await
        .expect("revoke contributor");
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
    fixture: &impl SharedListFixture,
) {
    let administrator = user("query-admin");
    let contributor = user("query-contributor");
    let outsider = user("query-outsider");
    let shared = list("query-shared", "query-admin", "Gemeinsam");
    fixture
        .provision_shared_list(shared.clone(), &contributor)
        .await
        .expect("provision");
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

    fixture
        .revoke_contributor(shared.id(), &contributor)
        .await
        .expect("revoke");
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

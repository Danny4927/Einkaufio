//! Reusable repository behavior contracts.
//!
//! Enable the `test-support` feature to reuse these scenarios from adapter integration tests.
//! Each contract provisions its own unique fixtures and expects a fresh repository pair.
//! Scenarios are async (ADR 014) and need no specific runtime; adapters drive them from their
//! own async tests. Access scoping follows ADR 015.

use std::future::Future;

use einkaufio_application::{
    ListAccessRequest, RepositoryError, ShoppingItemRepository, ShoppingListRepository,
};
use einkaufio_domain::{
    AggregateVersion, ItemDetails, ItemId, ItemName, ItemNote, ListId, ListName, ListOperation,
    ListPermission, ShoppingItem, ShoppingList, UserId,
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

//! Reusable repository behavior contracts.
//!
//! Enable the `test-support` feature to reuse these scenarios from adapter integration tests.
//! Each contract provisions its own unique fixtures and expects a fresh repository pair.

use einkaufio_application::{RepositoryError, ShoppingItemRepository, ShoppingListRepository};
use einkaufio_domain::{
    AggregateVersion, ItemId, ItemName, ListId, ListName, ShoppingItem, ShoppingList, UserId,
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

/// Runs list and cascade scenarios against a fresh pair of repositories.
pub fn run_shopping_list_contract(
    lists: &impl ShoppingListRepository,
    items: &impl ShoppingItemRepository,
) {
    let owner = user("contract-owner");
    let other = user("contract-other");
    let created = list("contract-list", "contract-owner", "Einkauf");
    lists.create_personal(created.clone()).expect("create");
    assert_eq!(
        lists.create_personal(created.clone()),
        Err(RepositoryError::AlreadyExists)
    );
    assert_eq!(
        lists.list_personal_for_owner(&owner).unwrap(),
        vec![created.clone()]
    );
    assert!(lists.list_personal_for_owner(&other).unwrap().is_empty());

    assert_eq!(
        lists.rename_personal(
            &other,
            created.id(),
            version(1),
            ListName::try_new("X").unwrap()
        ),
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        lists.rename_personal(
            &other,
            &list_id("absent"),
            version(1),
            ListName::try_new("X").unwrap()
        ),
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        lists.rename_personal(
            &owner,
            created.id(),
            version(2),
            ListName::try_new("Einkauf").unwrap()
        ),
        Err(RepositoryError::Conflict)
    );
    let renamed = lists
        .rename_personal(
            &owner,
            created.id(),
            version(1),
            ListName::try_new("Drogerie").unwrap(),
        )
        .unwrap();
    assert_eq!(renamed.version(), 2);
    let no_op = lists
        .rename_personal(
            &owner,
            created.id(),
            version(2),
            ListName::try_new("Drogerie").unwrap(),
        )
        .unwrap();
    assert_eq!(no_op.version(), 2);

    // A stale delete must not remove the aggregate.
    assert_eq!(
        lists.delete_personal(&owner, created.id(), version(1)),
        Err(RepositoryError::Conflict)
    );
    // Create an item and verify list deletion cascades to its items.
    let list_version = items
        .add_item(
            &owner,
            created.id(),
            version(2),
            item("cascade-item", "contract-owner", 77),
        )
        .unwrap();
    assert_eq!(list_version, version(3));
    assert_eq!(
        items.list_items(&other, created.id()),
        Err(RepositoryError::NotFound)
    );
    assert_eq!(items.list_items(&owner, created.id()).unwrap().len(), 1);
    assert_eq!(
        lists.delete_personal(&other, created.id(), version(3)),
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        lists.delete_personal(&owner, created.id(), version(2)),
        Err(RepositoryError::Conflict)
    );
    lists
        .delete_personal(&owner, created.id(), version(3))
        .unwrap();
    assert_eq!(
        items.list_items(&owner, created.id()),
        Err(RepositoryError::NotFound)
    );

    // Reusing the list and item IDs proves no orphaned item remains after cascade deletion.
    lists
        .create_personal(list("contract-list", "contract-owner", "Recreated"))
        .unwrap();
    assert_eq!(
        items.add_item(
            &owner,
            created.id(),
            version(1),
            item("cascade-item", "contract-owner", 0)
        ),
        Ok(version(2))
    );
}

/// Runs item scenarios against a fresh repository pair; this contract creates its own list.
pub fn run_shopping_item_contract(
    lists: &impl ShoppingListRepository,
    items: &impl ShoppingItemRepository,
) {
    let owner = user("contract-item-owner");
    let other = user("contract-item-other");
    let item_list = list("contract-item-list", "contract-item-owner", "Items");
    lists
        .create_personal(item_list.clone())
        .expect("provision contract list");

    assert_eq!(
        items.add_item(
            &other,
            item_list.id(),
            version(1),
            item("unauthorized-item", "contract-item-owner", 0)
        ),
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        items.add_item(
            &other,
            &list_id("missing-parent-list"),
            version(1),
            item("unknown-parent", "contract-item-owner", 0)
        ),
        Err(RepositoryError::NotFound)
    );

    let list_version = items
        .add_item(
            &owner,
            item_list.id(),
            version(1),
            item("contract-item-1", "contract-item-owner", 999),
        )
        .expect("first item");
    assert_eq!(list_version, version(2));
    assert_eq!(
        items.add_item(
            &owner,
            item_list.id(),
            version(2),
            item("contract-item-1", "contract-item-owner", 0)
        ),
        Err(RepositoryError::AlreadyExists)
    );
    assert_eq!(
        items.add_item(
            &owner,
            item_list.id(),
            version(1),
            item("stale-item", "contract-item-owner", 0)
        ),
        Err(RepositoryError::Conflict)
    );

    let item_id = ItemId::try_new("contract-item-1").unwrap();
    assert_eq!(
        items.set_item_checked(&other, item_list.id(), &item_id, 1, true),
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        items.set_item_checked(&owner, item_list.id(), &item_id, 99, true),
        Err(RepositoryError::Conflict)
    );
    let (checked, list_version) = items
        .set_item_checked(&owner, item_list.id(), &item_id, 1, true)
        .unwrap();
    assert!(checked.checked());
    assert_eq!(checked.version(), 2);
    assert_eq!(list_version, version(3));

    let (unchanged, list_version) = items
        .set_item_checked(&owner, item_list.id(), &item_id, 2, true)
        .unwrap();
    assert_eq!(unchanged.version(), 2);
    assert_eq!(list_version, version(3));

    let (unchecked, list_version) = items
        .set_item_checked(&owner, item_list.id(), &item_id, 2, false)
        .unwrap();
    assert!(!unchecked.checked());
    assert_eq!(unchecked.version(), 3);
    assert_eq!(list_version, version(4));

    assert_eq!(
        items.delete_item(&owner, item_list.id(), &item_id, 2),
        Err(RepositoryError::Conflict)
    );
    assert_eq!(
        items.delete_item(&other, item_list.id(), &item_id, 3),
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        items.delete_item(&owner, item_list.id(), &item_id, 3),
        Ok(version(5))
    );
    assert_eq!(
        items.delete_item(&owner, item_list.id(), &item_id, 3),
        Err(RepositoryError::NotFound)
    );

    // Capacity and repository-assigned unique positions are part of the shared port contract.
    let capacity_list = list("contract-capacity-list", "contract-item-owner", "Capacity");
    lists.create_personal(capacity_list.clone()).unwrap();
    for index in 0..ShoppingList::MAX_ACTIVE_ITEMS {
        let expected = version(index as u64 + 1);
        let next = items
            .add_item(
                &owner,
                capacity_list.id(),
                expected,
                item(&format!("capacity-item-{index}"), "contract-item-owner", 42),
            )
            .unwrap();
        assert_eq!(next, version(index as u64 + 2));
    }
    assert_eq!(
        items.add_item(
            &owner,
            capacity_list.id(),
            version(101),
            item("capacity-overflow", "contract-item-owner", 0)
        ),
        Err(RepositoryError::ItemLimitReached)
    );
    let stored = items.list_items(&owner, capacity_list.id()).unwrap();
    assert_eq!(stored.len(), ShoppingList::MAX_ACTIVE_ITEMS);
    let positions: Vec<_> = stored.iter().map(ShoppingItem::position).collect();
    assert_eq!(
        positions,
        (0..ShoppingList::MAX_ACTIVE_ITEMS as u32).collect::<Vec<_>>()
    );
    assert_eq!(
        items.list_items(&other, capacity_list.id()),
        Err(RepositoryError::NotFound)
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::in_memory::InMemoryRepository;

    #[test]
    fn in_memory_adapter_passes_list_contract() {
        let repository = InMemoryRepository::new();
        run_shopping_list_contract(&repository, &repository);
    }

    #[test]
    fn in_memory_adapter_passes_item_contract() {
        let repository = InMemoryRepository::new();
        run_shopping_item_contract(&repository, &repository);
    }

    #[test]
    fn default_list_read_is_fail_closed() {
        struct Unimplemented;
        impl ShoppingListRepository for Unimplemented {
            fn create_personal(&self, _: ShoppingList) -> Result<(), RepositoryError> {
                Err(RepositoryError::Unavailable)
            }
        }
        assert_eq!(
            Unimplemented.list_personal_for_owner(&user("owner")),
            Err(RepositoryError::Unavailable)
        );
    }
}

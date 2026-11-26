//! Reusable repository behavior contracts.
//!
//! Run these scenarios against a fresh instance of every repository adapter. A production
//! adapter must additionally test its storage-specific conditional writes and transaction
//! behavior (for DynamoDB, preferably with DynamoDB Local plus targeted AWS integration).

use einkaufio_application::{RepositoryError, ShoppingItemRepository, ShoppingListRepository};
use einkaufio_domain::{
    AggregateVersion, ItemId, ItemName, ListId, ListName, ShoppingItem, ShoppingList, UserId,
};

use crate::in_memory::InMemoryRepository;

fn user(value: &str) -> UserId {
    UserId::try_new(value).expect("valid user")
}
fn list_id(value: &str) -> ListId {
    ListId::try_new(value).expect("valid list id")
}
fn version(value: u64) -> AggregateVersion {
    AggregateVersion::try_new(value).expect("positive version")
}
fn list(id: &str, owner: &str, name: &str) -> ShoppingList {
    ShoppingList::new_personal(list_id(id), user(owner), ListName::try_new(name).unwrap())
}
fn item(id: &str, owner: &str) -> ShoppingItem {
    ShoppingItem::new(
        ItemId::try_new(id).unwrap(),
        ItemName::try_new("Milch").unwrap(),
        None,
        None,
        None,
        0,
        user(owner),
    )
}

/// Shared list lifecycle scenarios. Requires a fresh, empty repository instance.
pub fn run_shopping_list_contract(repository: &impl ShoppingListRepository) {
    let owner = user("contract-owner");
    let other = user("contract-other");
    let created = list("contract-list", "contract-owner", "Einkauf");
    repository.create_personal(created.clone()).expect("create");

    assert_eq!(
        repository.list_personal_for_owner(&owner).unwrap(),
        vec![created.clone()]
    );
    assert!(repository.list_personal_for_owner(&other).unwrap().is_empty());

    // Unauthorized existing and absent resources are indistinguishable.
    assert_eq!(
        repository.rename_personal(&other, created.id(), version(1), ListName::try_new("X").unwrap()),
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        repository.rename_personal(&other, &list_id("absent"), version(1), ListName::try_new("X").unwrap()),
        Err(RepositoryError::NotFound)
    );
    // Stale expected version must conflict even when the requested name is unchanged.
    assert_eq!(
        repository.rename_personal(&owner, created.id(), version(2), ListName::try_new("Einkauf").unwrap()),
        Err(RepositoryError::Conflict)
    );

    let renamed = repository.rename_personal(
        &owner, created.id(), version(1), ListName::try_new("Drogerie").unwrap()
    ).unwrap();
    assert_eq!(renamed.name().as_str(), "Drogerie");
    assert_eq!(renamed.version(), 2);

    let no_op = repository.rename_personal(
        &owner, created.id(), version(2), ListName::try_new("Drogerie").unwrap()
    ).unwrap();
    assert_eq!(no_op.version(), 2);

    assert_eq!(
        repository.delete_personal(&other, created.id(), version(2)),
        Err(RepositoryError::NotFound)
    );
    repository.delete_personal(&owner, created.id(), version(2)).unwrap();
    assert_eq!(
        repository.delete_personal(&owner, created.id(), version(2)),
        Err(RepositoryError::NotFound)
    );
    assert!(repository.list_personal_for_owner(&owner).unwrap().is_empty());
}

/// Shared item lifecycle scenarios. Requires a fresh repository with the matching list
/// already provisioned at version 1.
pub fn run_shopping_item_contract(repository: &impl ShoppingItemRepository) {
    let owner = user("contract-item-owner");
    let other = user("contract-item-other");
    let list = list_id("contract-item-list");

    // Existing and absent parent lists must be indistinguishable for an unauthorized actor.
    assert_eq!(
        repository.add_item(&other, list.as_str(), 1, item("unauthorized-item", "contract-item-owner")),
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        repository.add_item(&other, "missing-parent-list", 1, item("unknown-parent-item", "contract-item-owner")),
        Err(RepositoryError::NotFound)
    );
    let list_version = repository.add_item(
        &owner, list.as_str(), 1, item("contract-item-1", "contract-item-owner")
    ).expect("first item");
    assert_eq!(list_version, 2);

    assert_eq!(
        repository.add_item(&owner, list.as_str(), 1, item("stale-item", "contract-item-owner")),
        Err(RepositoryError::Conflict)
    );
    let (checked, list_version) = repository.set_item_checked(
        &owner, list.as_str(), &ItemId::try_new("contract-item-1").unwrap(), 1, true
    ).unwrap();
    assert!(checked.checked());
    assert_eq!(checked.version(), 2);
    assert_eq!(list_version, 3);

    // Idempotent check must not advance either version.
    let (unchanged, list_version) = repository.set_item_checked(
        &owner, list.as_str(), &ItemId::try_new("contract-item-1").unwrap(), 2, true
    ).unwrap();
    assert_eq!(unchanged.version(), 2);
    assert_eq!(list_version, 3);

    assert_eq!(
        repository.delete_item(
            &other, list.as_str(), &ItemId::try_new("contract-item-1").unwrap(), 2
        ),
        Err(RepositoryError::NotFound)
    );
    let next = repository.delete_item(
        &owner, list.as_str(), &ItemId::try_new("contract-item-1").unwrap(), 2
    ).unwrap();
    assert_eq!(next, 4);
    assert_eq!(
        repository.delete_item(
            &owner, list.as_str(), &ItemId::try_new("contract-item-1").unwrap(), 2
        ),
        Err(RepositoryError::NotFound)
    );
    

}

/// The reference adapter is executed against the exact same exported scenarios production
/// adapters will invoke.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_adapter_passes_list_contract() {
        run_shopping_list_contract(&InMemoryRepository::new());
    }

    #[test]
    fn in_memory_adapter_passes_item_contract() {
        let repository = InMemoryRepository::new();
        repository.create_personal(list("contract-item-list", "contract-item-owner", "Items")).unwrap();
        run_shopping_item_contract(&repository);
    }

    #[test]
    fn in_memory_adapter_assigns_unique_positions_and_enforces_capacity() {
        let repository = InMemoryRepository::new();
        let owner = user("capacity-owner");
        repository
            .create_personal(list("capacity-list", "capacity-owner", "Capacity"))
            .unwrap();

        for index in 0..ShoppingList::MAX_ACTIVE_ITEMS {
            let expected_version = index as u64 + 1;
            let next_version = repository
                .add_item(
                    &owner,
                    "capacity-list",
                    expected_version,
                    item(&format!("capacity-item-{index}"), "capacity-owner"),
                )
                .unwrap();
            assert_eq!(next_version, expected_version + 1);
        }

        let list_id = list_id("capacity-list");
        let items = repository.items_for_list(&list_id);
        assert_eq!(items.len(), ShoppingList::MAX_ACTIVE_ITEMS);
        for (expected_position, item) in items.iter().enumerate() {
            assert_eq!(item.position(), expected_position as u32);
        }

        assert_eq!(
            repository.add_item(
                &owner,
                "capacity-list",
                101,
                item("capacity-overflow", "capacity-owner"),
            ),
            Err(RepositoryError::ItemLimitReached)
        );
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

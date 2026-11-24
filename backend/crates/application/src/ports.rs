//! Outbound ports owned by the application layer.

use einkaufio_domain::{ItemId, ShoppingItem, ShoppingList};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryError {
    AlreadyExists,
    NotFound,
    Conflict,
    Unavailable,
}

/// Atomically persists a new personal list and the owner's list reference.
pub trait ShoppingListRepository {
    fn create_personal(&self, list: ShoppingList) -> Result<(), RepositoryError>;
}

/// Item mutations and parent-list version changes must be atomic in every adapter.
pub trait ShoppingItemRepository {
    fn add_item(
        &self,
        list_id: &str,
        expected_list_version: u64,
        item: ShoppingItem,
    ) -> Result<u64, RepositoryError>;

    fn set_item_checked(
        &self,
        list_id: &str,
        item_id: &ItemId,
        expected_item_version: u64,
        checked: bool,
    ) -> Result<(ShoppingItem, u64), RepositoryError>;

    fn delete_item(
        &self,
        list_id: &str,
        item_id: &ItemId,
        expected_item_version: u64,
    ) -> Result<u64, RepositoryError>;
}

pub trait ListIdGenerator {
    fn generate(&self) -> String;
}

pub trait ItemIdGenerator {
    fn generate(&self) -> String;
}

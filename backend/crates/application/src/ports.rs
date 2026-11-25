//! Outbound ports owned by the application layer.

use einkaufio_domain::{ItemId, ShoppingItem, ShoppingList, UserId};

/// Persistence failures exposed by repository ports without storage-specific details.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryError {
    AlreadyExists,
    NotFound,
    Conflict,
    ItemLimitReached,
    Unavailable,
}

/// Persists and queries personal shopping lists.
///
/// Implementations must scope reads to the supplied owner. Never expose a global
/// list query to application use cases.
pub trait ShoppingListRepository {
    /// Persists a newly created personal shopping list.
    ///
    /// Implementations must atomically persist the list and the owner's list reference.
    fn create_personal(&self, list: ShoppingList) -> Result<(), RepositoryError>;

    /// Returns only personal lists owned by the authenticated user.
    fn list_personal_for_owner(
        &self,
        owner_id: &UserId,
    ) -> Result<Vec<ShoppingList>, RepositoryError>;
}

/// Mutates items owned by a list.
///
/// Every method receives the authenticated actor. Implementations MUST verify that the
/// actor is authorized to access the list before revealing whether the list or item exists.
/// Item changes and parent-list version changes must be atomic. Adding an item must also
/// atomically enforce the 100-active-item limit and assign a unique position; the adapter
/// must not trust a client-provided position.
pub trait ShoppingItemRepository {
    /// Adds an item and returns the new parent-list version.
    fn add_item(
        &self,
        actor_id: &UserId,
        list_id: &str,
        expected_list_version: u64,
        item: ShoppingItem,
    ) -> Result<u64, RepositoryError>;

    /// Changes the item and returns it together with the new parent-list version.
    fn set_item_checked(
        &self,
        actor_id: &UserId,
        list_id: &str,
        item_id: &ItemId,
        expected_item_version: u64,
        checked: bool,
    ) -> Result<(ShoppingItem, u64), RepositoryError>;

    /// Deletes the item and returns the new parent-list version.
    fn delete_item(
        &self,
        actor_id: &UserId,
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

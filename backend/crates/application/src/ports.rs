//! Outbound ports owned by the application layer.

use einkaufio_domain::{
    AggregateVersion, ItemId, ListId, ListName, ShoppingItem, ShoppingList, UserId,
};

/// Persistence failures exposed by repository ports without storage-specific details.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryError {
    AlreadyExists,
    NotFound,
    Conflict,
    ItemLimitReached,
    /// The aggregate cannot advance because its version reached the numeric limit.
    VersionExhausted,
    Unavailable,
}

/// Persists and queries personal shopping lists.
///
/// Implementations must scope reads and mutations to the supplied owner. Never expose a
/// global list query to application use cases. For mutations, adapters must authorize the
/// actor before disclosing whether a list exists.
pub trait ShoppingListRepository {
    /// Persists a newly created personal shopping list.
    ///
    /// Implementations must atomically persist the list and the owner's list reference.
    fn create_personal(&self, list: ShoppingList) -> Result<(), RepositoryError>;

    /// Returns only personal lists owned by the authenticated user.
    ///
    /// The fail-closed default keeps existing adapters safe until they implement the query.
    fn list_personal_for_owner(
        &self,
        _owner_id: &UserId,
    ) -> Result<Vec<ShoppingList>, RepositoryError> {
        Err(RepositoryError::Unavailable)
    }

    /// Renames an owned list and returns its new aggregate state.
    ///
    /// The adapter must authorize the actor before revealing list existence and compare
    /// expected_version atomically with the stored aggregate version BEFORE checking
    /// whether the requested name is unchanged. A stale expected version MUST return
    /// Conflict, even for a no-op rename. The adapter then applies the domain rename and
    /// persists the resulting state atomically. Map domain VersionExhausted to the
    /// explicit repository VersionExhausted error.
    fn rename_personal(
        &self,
        _actor_id: &UserId,
        _list_id: &ListId,
        _expected_version: AggregateVersion,
        _name: ListName,
    ) -> Result<ShoppingList, RepositoryError> {
        Err(RepositoryError::Unavailable)
    }

    /// Deletes an owned list and its owner index/reference atomically.
    ///
    /// The adapter must authorize the actor before revealing list existence and compare
    /// expected_version in the same transaction as deletion. A repeated delete returns
    /// NotFound (the API boundary may translate this to an idempotent response if desired).
    fn delete_personal(
        &self,
        _actor_id: &UserId,
        _list_id: &ListId,
        _expected_version: AggregateVersion,
    ) -> Result<(), RepositoryError> {
        Err(RepositoryError::Unavailable)
    }
}

/// Mutates items owned by a list.
///
/// Every method receives the authenticated actor. Implementations MUST verify that the
/// actor is authorized to access the list before revealing whether the list or item exists.
/// Item changes and parent-list version changes must be atomic. Adding an item must also
/// atomically enforce the 100-active-item limit and assign a unique position; the adapter
/// must not trust a client-provided position. The order of validation errors is deliberately
/// unspecified: callers must rely on the documented error category only when a single
/// precondition is violated.
pub trait ShoppingItemRepository {
    /// Adds an item and returns the new parent-list version.
    fn add_item(
        &self,
        actor_id: &UserId,
        list_id: &ListId,
        expected_list_version: AggregateVersion,
        item: ShoppingItem,
    ) -> Result<AggregateVersion, RepositoryError>;

    /// Returns the ordered items for an authorized list.
    ///
    /// Implementations must authorize before revealing list existence. The result is ordered
    /// by the repository-assigned position, which must be unique within the list.
    fn list_items(
        &self,
        _actor_id: &UserId,
        _list_id: &ListId,
    ) -> Result<Vec<ShoppingItem>, RepositoryError> {
        Err(RepositoryError::Unavailable)
    }

    /// Changes the item and returns it together with the new parent-list version.
    fn set_item_checked(
        &self,
        actor_id: &UserId,
        list_id: &ListId,
        item_id: &ItemId,
        expected_item_version: u64,
        checked: bool,
    ) -> Result<(ShoppingItem, AggregateVersion), RepositoryError>;

    /// Deletes the item and returns the new parent-list version.
    fn delete_item(
        &self,
        actor_id: &UserId,
        list_id: &ListId,
        item_id: &ItemId,
        expected_item_version: u64,
    ) -> Result<AggregateVersion, RepositoryError>;
}

pub trait ListIdGenerator {
    fn generate(&self) -> String;
}

pub trait ItemIdGenerator {
    fn generate(&self) -> String;
}

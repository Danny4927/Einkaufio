//! Outbound ports owned by the application layer.

use einkaufio_domain::ShoppingList;

/// Persistence failures exposed by the repository port, without storage-specific details.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryError {
    AlreadyExists,
    Unavailable,
}

/// Persists a newly created personal shopping list.
///
/// Implementations must atomically persist the list and the owner's list reference.
pub trait ShoppingListRepository {
    fn create_personal(&self, list: ShoppingList) -> Result<(), RepositoryError>;
}

/// Generates opaque list identifiers. The application validates the generated value.
pub trait ListIdGenerator {
    fn generate(&self) -> String;
}

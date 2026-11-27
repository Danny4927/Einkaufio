//! Pure domain model for Einkaufio.
//!
//! This crate must remain independent of frameworks, persistence technology,
//! AWS SDKs, transport DTOs and serialization libraries.

pub mod access;
pub mod identifiers;
pub mod shopping_item;
pub mod shopping_list;

pub use access::{ListAccess, ListOperation, ListPermission};
pub use identifiers::{IdentifierError, ItemId, ListId, UserId};
pub use shopping_item::{
    ItemDetails, ItemMutationError, ItemName, ItemNote, ItemQuantity, ItemUnit, ItemValueError,
    ShoppingItem,
};
pub use shopping_list::{
    AggregateVersion, AggregateVersionError, ListMutationError, ListName, ListNameError,
    ShoppingList,
};

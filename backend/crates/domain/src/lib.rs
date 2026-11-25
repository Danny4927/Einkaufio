//! Pure domain model for Einkaufio.
//!
//! This crate must remain independent of frameworks, persistence technology,
//! AWS SDKs, transport DTOs and serialization libraries.

pub mod identifiers;
pub mod shopping_item;
pub mod shopping_list;

pub use identifiers::{IdentifierError, ItemId, ListId, UserId};
pub use shopping_item::{ItemName, ItemNote, ItemQuantity, ItemUnit, ItemValueError, ShoppingItem};
pub use shopping_list::{ListName, ListMutationError, ListNameError, ShoppingList};

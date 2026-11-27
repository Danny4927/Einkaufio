//! Application use cases and ports owned by the application layer.
//!
//! This crate may depend on the domain crate, but never on infrastructure
//! adapters, HTTP frameworks, AWS SDKs or database clients.

pub mod create_personal_list;
pub mod delete_shopping_list;
pub mod list_accessible_lists;
pub mod ports;
pub mod rename_shopping_list;
pub mod shopping_items;
#[cfg(test)]
pub(crate) mod test_support;

pub use create_personal_list::{
    CreatePersonalList, CreatePersonalListCommand, CreatePersonalListError,
};
pub use delete_shopping_list::{
    DeleteShoppingList, DeleteShoppingListCommand, DeleteShoppingListError,
};
pub use list_accessible_lists::{
    ListAccessibleLists, ListAccessibleListsError, ListAccessibleListsQuery,
};
pub use ports::{
    ItemIdGenerator, ListAccessRequest, ListIdGenerator, RepositoryError, ShoppingItemRepository,
    ShoppingListRepository,
};
pub use rename_shopping_list::{
    RenameShoppingList, RenameShoppingListCommand, RenameShoppingListError,
};
pub use shopping_items::{
    AddShoppingItem, AddShoppingItemCommand, DeleteShoppingItem, DeleteShoppingItemCommand,
    SetShoppingItemChecked, SetShoppingItemCheckedCommand, ShoppingItemError, UpdateShoppingItem,
    UpdateShoppingItemCommand,
};

/// Domain types that appear in list-port signatures, re-exported for inbound adapters.
pub use einkaufio_domain::{ListId, ListName, ShoppingList, UserId};

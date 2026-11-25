//! Application use cases and ports owned by the application layer.
//!
//! This crate may depend on the domain crate, but never on infrastructure
//! adapters, HTTP frameworks, AWS SDKs or database clients.

pub mod create_personal_list;
pub mod list_personal_lists;
pub mod ports;
pub mod shopping_items;

pub use create_personal_list::{
    CreatePersonalList, CreatePersonalListCommand, CreatePersonalListError,
};
pub use list_personal_lists::{ListPersonalLists, ListPersonalListsError, ListPersonalListsQuery};
pub use ports::{
    ItemIdGenerator, ListIdGenerator, RepositoryError, ShoppingItemRepository,
    ShoppingListRepository,
};
pub use shopping_items::{
    AddShoppingItem, AddShoppingItemCommand, DeleteShoppingItem, DeleteShoppingItemCommand,
    SetShoppingItemChecked, SetShoppingItemCheckedCommand, ShoppingItemError,
};

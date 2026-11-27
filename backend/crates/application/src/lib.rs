//! Application use cases and ports owned by the application layer.
//!
//! This crate may depend on the domain crate, but never on infrastructure
//! adapters, HTTP frameworks, AWS SDKs or database clients.

pub mod create_personal_list;
pub mod delete_personal_list;
pub mod list_personal_lists;
pub mod ports;
pub mod rename_personal_list;
pub mod shopping_items;

pub use create_personal_list::{
    CreatePersonalList, CreatePersonalListCommand, CreatePersonalListError,
};
pub use delete_personal_list::{
    DeletePersonalList, DeletePersonalListCommand, DeletePersonalListError,
};
pub use list_personal_lists::{ListPersonalLists, ListPersonalListsError, ListPersonalListsQuery};
pub use ports::{
    ItemIdGenerator, ListIdGenerator, RepositoryError, ShoppingItemRepository,
    ShoppingListRepository,
};
pub use rename_personal_list::{
    RenamePersonalList, RenamePersonalListCommand, RenamePersonalListError,
};
pub use shopping_items::{
    AddShoppingItem, AddShoppingItemCommand, DeleteShoppingItem, DeleteShoppingItemCommand,
    SetShoppingItemChecked, SetShoppingItemCheckedCommand, ShoppingItemError, UpdateShoppingItem,
    UpdateShoppingItemCommand,
};

pub use einkaufio_domain::ShoppingList;

//! Application use cases and ports owned by the application boundary.
//!
//! This crate may depend on the domain crate, but never on infrastructure
//! adapters, HTTP frameworks, AWS SDKs or database clients.

pub mod create_personal_list;
pub mod ports;

pub use create_personal_list::{
    CreatePersonalList, CreatePersonalListCommand, CreatePersonalListError,
};
pub use ports::{ListIdGenerator, RepositoryError, ShoppingListRepository};

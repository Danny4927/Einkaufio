//! Pure domain model for Einkaufio.
//!
//! This crate must remain independent of frameworks, persistence technology,
//! AWS SDKs, transport DTOs and serialization libraries.

pub mod access;
pub mod household;
pub mod identifiers;
pub mod invitation;
pub mod profile;
pub mod shopping_item;
pub mod shopping_list;

pub use access::{ListAccess, ListOperation, ListPermission};
pub use household::{
    Household, HouseholdMember, HouseholdMutationError, HouseholdName, HouseholdNameError,
    HouseholdOperation, HouseholdPermission, HouseholdRole,
};
pub use identifiers::{HouseholdId, IdentifierError, InvitationId, ItemId, ListId, UserId};
pub use invitation::{
    Invitation, InvitationError, InvitationLifetime, InvitationLimitError, InvitationMaxUses,
    InvitationRestoreError, InvitationToken, InvitationTokenError, InvitationTokenHash,
    InvitationUseError, Timestamp,
};
pub use profile::{DisplayName, DisplayNameError, UserProfile};
pub use shopping_item::{
    ItemDetails, ItemMutationError, ItemName, ItemNote, ItemOrder, ItemOrderError, ItemQuantity,
    ItemUnit, ItemValueError, ShoppingItem,
};
pub use shopping_list::{
    AggregateVersion, AggregateVersionError, ListMutationError, ListName, ListNameError, ListOwner,
    ShoppingList,
};

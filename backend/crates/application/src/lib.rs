//! Application use cases and ports owned by the application layer.
//!
//! This crate may depend on the domain crate, but never on infrastructure
//! adapters, HTTP frameworks, AWS SDKs or database clients.

pub mod create_personal_list;
pub mod delete_shopping_list;
pub mod household_ports;
pub mod households;
pub mod invitations;
pub mod list_accessible_lists;
pub mod list_queries;
pub mod ports;
pub mod profiles;
pub mod rate_limit;
pub mod rename_shopping_list;
pub mod reorder_shopping_items;
pub mod shopping_items;
#[cfg(test)]
pub(crate) mod test_support;

pub use create_personal_list::{
    CreatePersonalList, CreatePersonalListCommand, CreatePersonalListError,
};
pub use delete_shopping_list::{
    DeleteShoppingList, DeleteShoppingListCommand, DeleteShoppingListError,
};
pub use household_ports::{
    Clock, HouseholdAccessRequest, HouseholdDetails, HouseholdIdGenerator, HouseholdRepository,
    HouseholdRepositoryError, HouseholdSummary, InvitationIdGenerator, InvitationRepository,
    InvitationTokenGenerator, InvitationTokenHasher, TokenGenerationError,
};
pub use households::{
    CreateHousehold, CreateHouseholdCommand, CreateHouseholdList, CreateHouseholdListCommand,
    DeleteHousehold, DeleteHouseholdCommand, GetHousehold, GetHouseholdQuery, HouseholdError,
    LeaveHousehold, LeaveHouseholdCommand, ListHouseholds, ListHouseholdsQuery,
    RemoveHouseholdMember, RemoveHouseholdMemberCommand, RenameHousehold, RenameHouseholdCommand,
    TransferHouseholdOwnership, TransferHouseholdOwnershipCommand,
};
pub use invitations::{
    AcceptHouseholdInvitation, AcceptHouseholdInvitationCommand, CreateHouseholdInvitation,
    CreateHouseholdInvitationCommand, CreatedInvitation, InvitationServices,
    ListHouseholdInvitations, ListHouseholdInvitationsQuery, RevokeHouseholdInvitation,
    RevokeHouseholdInvitationCommand,
};
pub use list_accessible_lists::{
    ListAccessibleLists, ListAccessibleListsError, ListAccessibleListsQuery,
};
pub use list_queries::{
    ListQueryError, PageAccessibleLists, PageParameters, PageShoppingItems, SyncShoppingList,
};
pub use ports::{
    ItemIdGenerator, ListAccessRequest, ListIdGenerator, ListSync, Page, PageCursor, PageRequest,
    PageSize, PageSizeError, ProfileRepository, RepositoryError, ShoppingItemRepository,
    ShoppingListRepository,
};
pub use profiles::{CreateProfile, GetProfile, ProfileError, UpdateProfile};
pub use rename_shopping_list::{
    RenameShoppingList, RenameShoppingListCommand, RenameShoppingListError,
};
pub use reorder_shopping_items::{ReorderShoppingItems, ReorderShoppingItemsCommand};
pub use shopping_items::{
    AddShoppingItem, AddShoppingItemCommand, DeleteShoppingItem, DeleteShoppingItemCommand,
    SetShoppingItemChecked, SetShoppingItemCheckedCommand, ShoppingItemError, UpdateShoppingItem,
    UpdateShoppingItemCommand,
};

/// Domain types returned by item, sync and profile use cases, re-exported for inbound adapters.
pub use einkaufio_domain::{
    AggregateVersion, DisplayName, ItemDetails, ItemId, ItemOrder, ShoppingItem, UserProfile,
};
/// Domain types of household and invitation ports, re-exported for inbound adapters.
pub use einkaufio_domain::{
    Household, HouseholdId, HouseholdMember, HouseholdName, HouseholdOperation, HouseholdRole,
    Invitation, InvitationId, InvitationToken, ListOwner, Timestamp,
};
/// Domain types that appear in list-port signatures, re-exported for inbound adapters.
pub use einkaufio_domain::{ListId, ListName, ShoppingList, UserId};
pub use rate_limit::{RateLimitDecision, RateLimitError, RateLimiter};

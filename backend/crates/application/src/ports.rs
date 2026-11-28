//! Outbound ports owned by the application layer.
//!
//! Repository ports are asynchronous (ADR 014): every method returns a `Send` future so that
//! use cases can run inside multi-threaded async runtimes (Axum on Lambda). Implementations
//! must be `Send + Sync` and must never block the runtime thread on I/O.
//!
//! List and item ports are access-scoped (ADR 015): operations on an existing list receive a
//! [`ListAccessRequest`] naming the verified principal and the permission the operation
//! requires. The adapter resolves the principal's effective [`ListAccess`] from persisted
//! ownership/membership records and enforces it in the same atomic operation as the read or
//! mutation.
//!
//! [`ListAccess`]: einkaufio_domain::ListAccess

use std::future::Future;

use einkaufio_domain::{
    AggregateVersion, DisplayName, ItemDetails, ItemId, ItemOrder, ListId, ListName, ListOperation,
    ListPermission, ShoppingItem, ShoppingList, UserId, UserProfile,
};

/// Persistence failures exposed by repository ports without storage-specific details.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryError {
    AlreadyExists,
    /// The resource does not exist **or** the principal holds no access to it. The two cases
    /// are deliberately indistinguishable to avoid existence leaks.
    NotFound,
    /// The principal holds access to the list but not the permission the operation requires.
    ///
    /// Only returned to principals that can already see the list, so it discloses nothing
    /// they could not read anyway. Authorization is checked before expected versions.
    Forbidden,
    Conflict,
    ItemLimitReached,
    /// The aggregate cannot advance because its version reached the numeric limit.
    VersionExhausted,
    Unavailable,
    /// A page cursor was not issued by this adapter for this query or is no longer valid.
    InvalidCursor,
}

/// Bounded page size for cursor-based queries (roadmap B3).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageSize(u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PageSizeError {
    Zero,
    TooLarge,
}

impl PageSize {
    pub const MAX: u32 = 100;
    pub const DEFAULT: Self = Self(50);

    pub fn try_new(value: u32) -> Result<Self, PageSizeError> {
        match value {
            0 => Err(PageSizeError::Zero),
            value if value > Self::MAX => Err(PageSizeError::TooLarge),
            value => Ok(Self(value)),
        }
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    /// The page size as a collection length.
    pub fn as_usize(self) -> usize {
        usize::try_from(self.0).unwrap_or(usize::MAX)
    }
}

/// An opaque continuation token. Only the adapter that issued it can interpret it; clients
/// must pass it back unchanged. Construction only bounds its size and characters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageCursor(String);

impl PageCursor {
    pub const MAX_LENGTH: usize = 512;

    pub fn try_new(value: impl Into<String>) -> Result<Self, RepositoryError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > Self::MAX_LENGTH
            || !value.chars().all(|character| character.is_ascii_graphic())
        {
            return Err(RepositoryError::InvalidCursor);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageRequest {
    pub size: PageSize,
    pub after: Option<PageCursor>,
}

/// One page of a stable order. `next` is present iff more entries may follow.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next: Option<PageCursor>,
}

/// Result of a version-based sync query (roadmap B5, ADR 004).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListSync {
    /// Full current state. Returned when the client has no version, a version the adapter
    /// cannot derive changes from (for example newer than the stored one or older than the
    /// retained change history), and always allowed in place of `Changes`.
    Snapshot {
        list: ShoppingList,
        items: Vec<ShoppingItem>,
    },
    /// Everything that changed after `since`: items created or modified (current state) and
    /// ids of items removed. Applying it to the client's state at `since` yields the current
    /// state. An empty change set means the client is up to date.
    Changes {
        list: ShoppingList,
        since: AggregateVersion,
        changed: Vec<ShoppingItem>,
        removed: Vec<ItemId>,
    },
}

/// The verified principal and the permission an operation requires on its target list.
///
/// Use cases build this from the application rule table ([`ListOperation`]); adapters only
/// enforce it. The principal must come from verified authentication context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListAccessRequest {
    principal: UserId,
    permission: ListPermission,
}

impl ListAccessRequest {
    pub fn new(principal: UserId, permission: ListPermission) -> Self {
        Self {
            principal,
            permission,
        }
    }

    /// Builds the request for an operation using the central authorization rule table.
    pub fn for_operation(principal: UserId, operation: ListOperation) -> Self {
        Self::new(principal, operation.required_permission())
    }

    pub fn principal(&self) -> &UserId {
        &self.principal
    }

    pub fn permission(&self) -> ListPermission {
        self.permission
    }
}

/// Persists and queries shopping lists.
///
/// Implementations must scope every read and mutation to lists the principal can access
/// (ADR 015). Never expose a global list query to application use cases. Adapters must resolve
/// access before disclosing whether a list exists: a principal without access receives
/// `NotFound`, a principal with insufficient access receives `Forbidden`.
pub trait ShoppingListRepository: Send + Sync {
    /// Persists a newly created personal shopping list owned by `list.owner_id()`.
    ///
    /// Implementations must atomically persist the list and the owner's list reference.
    fn create_personal(
        &self,
        list: ShoppingList,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;

    /// Returns every list the principal can currently access.
    ///
    /// That is personal lists the principal owns and, once households exist (roadmap C2),
    /// lists owned by households in which the principal is an active member. Access is
    /// resolved from persisted records on every call. Order is unspecified.
    ///
    /// The fail-closed default keeps existing adapters safe until they implement the query.
    fn list_accessible(
        &self,
        _principal: &UserId,
    ) -> impl Future<Output = Result<Vec<ShoppingList>, RepositoryError>> + Send {
        async { Err(RepositoryError::Unavailable) }
    }

    /// Returns one page of accessible lists in a stable adapter-defined order.
    ///
    /// Access is resolved on every call. Lists created or removed between pages may be
    /// missed or skipped but never duplicated. An unknown cursor yields `InvalidCursor`.
    fn list_accessible_page(
        &self,
        _principal: &UserId,
        _page: &PageRequest,
    ) -> impl Future<Output = Result<Page<ShoppingList>, RepositoryError>> + Send {
        async { Err(RepositoryError::Unavailable) }
    }

    /// Renames an accessible list and returns its new aggregate state.
    ///
    /// The adapter must authorize the request before revealing list existence and compare
    /// expected_version atomically with the stored aggregate version BEFORE checking
    /// whether the requested name is unchanged. A stale expected version MUST return
    /// Conflict, even for a no-op rename. The adapter then applies the domain rename and
    /// persists the resulting state atomically. Map domain VersionExhausted to the
    /// explicit repository VersionExhausted error.
    fn rename_list(
        &self,
        _access: &ListAccessRequest,
        _list_id: &ListId,
        _expected_version: AggregateVersion,
        _name: ListName,
    ) -> impl Future<Output = Result<ShoppingList, RepositoryError>> + Send {
        async { Err(RepositoryError::Unavailable) }
    }

    /// Deletes an accessible list, its items and its index/references atomically.
    ///
    /// The adapter must authorize the request before revealing list existence and compare
    /// expected_version in the same transaction as deletion. A repeated delete returns
    /// NotFound (the API boundary may translate this to an idempotent response if desired).
    fn delete_list(
        &self,
        _access: &ListAccessRequest,
        _list_id: &ListId,
        _expected_version: AggregateVersion,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send {
        async { Err(RepositoryError::Unavailable) }
    }
}

/// Reads and mutates items of a list.
///
/// Every method receives a [`ListAccessRequest`]. Implementations MUST verify that the
/// principal holds the required permission on the list before revealing whether the list or
/// item exists, and must re-check persisted access atomically with the mutation. Item changes
/// and parent-list version changes must be atomic. Adding an item must also atomically
/// enforce the 100-active-item limit and assign a unique position; the adapter must not
/// trust a client-provided position. The order of validation errors after authorization is
/// deliberately unspecified: callers must rely on the documented error category only when a
/// single precondition is violated.
pub trait ShoppingItemRepository: Send + Sync {
    /// Adds an item and returns the new parent-list version.
    fn add_item(
        &self,
        access: &ListAccessRequest,
        list_id: &ListId,
        expected_list_version: AggregateVersion,
        item: ShoppingItem,
    ) -> impl Future<Output = Result<AggregateVersion, RepositoryError>> + Send;

    /// Returns the ordered items for an accessible list.
    ///
    /// Implementations must authorize before revealing list existence. The result is ordered
    /// by the repository-assigned position, which must be unique within the list.
    fn list_items(
        &self,
        _access: &ListAccessRequest,
        _list_id: &ListId,
    ) -> impl Future<Output = Result<Vec<ShoppingItem>, RepositoryError>> + Send {
        async { Err(RepositoryError::Unavailable) }
    }

    /// Returns one page of the list's items ordered by position.
    ///
    /// Authorization comes first: a principal without access receives `NotFound` even for a
    /// malformed cursor. Cursors are bound to the list they were issued for.
    fn list_items_page(
        &self,
        _access: &ListAccessRequest,
        _list_id: &ListId,
        _page: &PageRequest,
    ) -> impl Future<Output = Result<Page<ShoppingItem>, RepositoryError>> + Send {
        async { Err(RepositoryError::Unavailable) }
    }

    /// Reorders all items of a list and returns them ordered, with the parent-list version.
    ///
    /// After authorization, expected_list_version is compared atomically BEFORE anything
    /// else: a stale version is `Conflict`, even for a no-op order. The order must name
    /// exactly the current items, otherwise `Conflict` (the client's view is stale). Items
    /// get positions `0..n`; moved items and the list version advance once, and only when
    /// the order actually changes.
    fn reorder_items(
        &self,
        _access: &ListAccessRequest,
        _list_id: &ListId,
        _expected_list_version: AggregateVersion,
        _order: &ItemOrder,
    ) -> impl Future<Output = Result<(Vec<ShoppingItem>, AggregateVersion), RepositoryError>> + Send
    {
        async { Err(RepositoryError::Unavailable) }
    }

    /// Returns the changes since `since` or a full snapshot (see [`ListSync`]).
    ///
    /// Authorization comes first. The list and items are read in one consistent view.
    fn sync_list(
        &self,
        _access: &ListAccessRequest,
        _list_id: &ListId,
        _since: Option<AggregateVersion>,
    ) -> impl Future<Output = Result<ListSync, RepositoryError>> + Send {
        async { Err(RepositoryError::Unavailable) }
    }

    /// Changes the item and returns it together with the new parent-list version.
    fn set_item_checked(
        &self,
        access: &ListAccessRequest,
        list_id: &ListId,
        item_id: &ItemId,
        expected_item_version: u64,
        checked: bool,
    ) -> impl Future<Output = Result<(ShoppingItem, AggregateVersion), RepositoryError>> + Send;

    /// Replaces the editable item details and returns the item with the parent-list version.
    ///
    /// The adapter must authorize before revealing list/item existence and compare
    /// expected_item_version atomically BEFORE detecting a no-op update: a stale version MUST
    /// return Conflict even when the details are unchanged. Item and list versions advance
    /// together, and only when the details actually change. Check state and position are
    /// preserved. The fail-closed default keeps existing adapters safe until implemented.
    fn update_item_details(
        &self,
        _access: &ListAccessRequest,
        _list_id: &ListId,
        _item_id: &ItemId,
        _expected_item_version: u64,
        _details: ItemDetails,
    ) -> impl Future<Output = Result<(ShoppingItem, AggregateVersion), RepositoryError>> + Send
    {
        async { Err(RepositoryError::Unavailable) }
    }

    /// Deletes the item and returns the new parent-list version.
    fn delete_item(
        &self,
        access: &ListAccessRequest,
        list_id: &ListId,
        item_id: &ItemId,
        expected_item_version: u64,
    ) -> impl Future<Output = Result<AggregateVersion, RepositoryError>> + Send;
}

/// Persists user profiles. Every operation is scoped to the verified principal: there is no
/// way to address another user's profile, so no cross-user existence can leak.
pub trait ProfileRepository: Send + Sync {
    /// Creates the principal's profile; `AlreadyExists` if one exists.
    fn create_profile(
        &self,
        profile: UserProfile,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;

    /// Returns the principal's profile; `NotFound` if none exists.
    fn get_profile(
        &self,
        principal: &UserId,
    ) -> impl Future<Output = Result<UserProfile, RepositoryError>> + Send;

    /// Changes the display name. expected_version is compared atomically BEFORE detecting a
    /// no-op: a stale version is `Conflict` even for an unchanged name.
    fn update_profile(
        &self,
        principal: &UserId,
        expected_version: AggregateVersion,
        display_name: DisplayName,
    ) -> impl Future<Output = Result<UserProfile, RepositoryError>> + Send;

    /// Deletes the principal's profile (account deletion, roadmap D2); `NotFound` if none
    /// exists. The fail-closed default keeps adapters safe until they implement it.
    fn delete_profile(
        &self,
        _principal: &UserId,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send {
        async { Err(RepositoryError::Unavailable) }
    }
}

/// Generates list identifiers. Synchronous: generation must not perform I/O.
pub trait ListIdGenerator: Send + Sync {
    fn generate(&self) -> String;
}

/// Generates item identifiers. Synchronous: generation must not perform I/O.
pub trait ItemIdGenerator: Send + Sync {
    fn generate(&self) -> String;
}

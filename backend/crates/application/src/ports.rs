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
    AggregateVersion, ItemDetails, ItemId, ListId, ListName, ListOperation, ListPermission,
    ShoppingItem, ShoppingList, UserId,
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

/// Generates list identifiers. Synchronous: generation must not perform I/O.
pub trait ListIdGenerator: Send + Sync {
    fn generate(&self) -> String;
}

/// Generates item identifiers. Synchronous: generation must not perform I/O.
pub trait ItemIdGenerator: Send + Sync {
    fn generate(&self) -> String;
}

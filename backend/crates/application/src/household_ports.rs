//! Outbound ports for households, membership lifecycle and invitations (roadmap C2-C4).
//!
//! Like the list ports (ADR 015), household operations on an existing household receive a
//! [`HouseholdAccessRequest`]. The adapter resolves the principal's persisted membership and
//! enforces the required permission in the same atomic operation as the read or mutation.
//! Error order: no membership (or no such household) → `NotFound`; membership too weak →
//! `Forbidden`; then expected-version checks (`Conflict`); then business rules and limits.
//!
//! Invitation secrets never cross these ports in raw form: use cases hash the token through
//! [`InvitationTokenHasher`] and adapters only ever see the [`InvitationTokenHash`] (ADR 017).

use std::future::Future;

use einkaufio_domain::{
    AggregateVersion, Household, HouseholdId, HouseholdMember, HouseholdName, HouseholdOperation,
    HouseholdPermission, HouseholdRole, Invitation, InvitationId, InvitationToken,
    InvitationTokenHash, ShoppingList, Timestamp, UserId,
};

/// Persistence failures of the household and invitation ports.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HouseholdRepositoryError {
    AlreadyExists,
    /// No such household/member/invitation **or** the principal is not an active member.
    /// The cases are indistinguishable to avoid existence leaks.
    NotFound,
    /// The principal is an active member but the operation requires the owner.
    Forbidden,
    Conflict,
    VersionExhausted,
    Unavailable,
    /// The call violated a documented port precondition (a programming error), for example a
    /// personal list passed to `create_household_list`.
    InvalidRequest,
    /// The principal already belongs to `Household::MAX_HOUSEHOLDS_PER_USER` households.
    HouseholdLimitReached,
    /// The household already has `Household::MAX_MEMBERS` active members.
    MemberLimitReached,
    /// The household already owns `Household::MAX_LISTS` lists.
    ListLimitReached,
    /// The household already has `Invitation::MAX_VALID_PER_HOUSEHOLD` valid invitations.
    InvitationLimitReached,
    /// The token is unknown, revoked, expired or used up. Deliberately one variant so that a
    /// token holder learns nothing about which case applies.
    InvitationUnavailable,
    /// The principal is already an active member; no invitation use was consumed.
    AlreadyMember,
    /// The owner cannot leave or be removed; ownership must be transferred first (ADR 005).
    OwnerMustTransferOwnership,
}

/// The verified principal and the permission an operation requires on its household.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HouseholdAccessRequest {
    principal: UserId,
    permission: HouseholdPermission,
}

impl HouseholdAccessRequest {
    pub fn new(principal: UserId, permission: HouseholdPermission) -> Self {
        Self {
            principal,
            permission,
        }
    }

    /// Builds the request from the household rule table.
    pub fn for_operation(principal: UserId, operation: HouseholdOperation) -> Self {
        Self::new(principal, operation.required_permission())
    }

    pub fn principal(&self) -> &UserId {
        &self.principal
    }

    pub fn permission(&self) -> HouseholdPermission {
        self.permission
    }
}

/// A household together with the principal's role in it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HouseholdSummary {
    pub household: Household,
    pub role: HouseholdRole,
}

/// A household with its active members, ordered by user id.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HouseholdDetails {
    pub household: Household,
    pub members: Vec<HouseholdMember>,
}

/// Persists households and their memberships.
///
/// Every method has a fail-closed default so that a partially implemented adapter never
/// grants access or reports success.
pub trait HouseholdRepository: Send + Sync {
    /// Atomically persists the household and its owner's membership (and the owner's
    /// household reference). Enforces `Household::MAX_HOUSEHOLDS_PER_USER` for the owner
    /// atomically with the write.
    fn create_household(
        &self,
        _household: Household,
    ) -> impl Future<Output = Result<(), HouseholdRepositoryError>> + Send {
        async { Err(HouseholdRepositoryError::Unavailable) }
    }

    /// Households in which the principal is an active member, ordered by household id.
    fn list_households(
        &self,
        _principal: &UserId,
    ) -> impl Future<Output = Result<Vec<HouseholdSummary>, HouseholdRepositoryError>> + Send {
        async { Err(HouseholdRepositoryError::Unavailable) }
    }

    /// The household and its active members.
    fn get_household(
        &self,
        _access: &HouseholdAccessRequest,
        _household_id: &HouseholdId,
    ) -> impl Future<Output = Result<HouseholdDetails, HouseholdRepositoryError>> + Send {
        async { Err(HouseholdRepositoryError::Unavailable) }
    }

    /// Renames the household. The expected version is compared before no-op detection.
    fn rename_household(
        &self,
        _access: &HouseholdAccessRequest,
        _household_id: &HouseholdId,
        _expected_version: AggregateVersion,
        _name: HouseholdName,
    ) -> impl Future<Output = Result<Household, HouseholdRepositoryError>> + Send {
        async { Err(HouseholdRepositoryError::Unavailable) }
    }

    /// Persists a household-owned list (`list.household_id()` must be `Some`, otherwise
    /// `InvalidRequest`). Enforces `Household::MAX_LISTS` atomically with the write.
    fn create_household_list(
        &self,
        _access: &HouseholdAccessRequest,
        _list: ShoppingList,
    ) -> impl Future<Output = Result<(), HouseholdRepositoryError>> + Send {
        async { Err(HouseholdRepositoryError::Unavailable) }
    }

    /// Ends the principal's own membership. The owner gets `OwnerMustTransferOwnership`.
    /// Access to the household's lists ends with the same atomic write.
    fn leave_household(
        &self,
        _principal: &UserId,
        _household_id: &HouseholdId,
    ) -> impl Future<Output = Result<Household, HouseholdRepositoryError>> + Send {
        async { Err(HouseholdRepositoryError::Unavailable) }
    }

    /// The owner removes another member. Removing the owner is `OwnerMustTransferOwnership`;
    /// a user who is not an active member is `NotFound`.
    fn remove_member(
        &self,
        _access: &HouseholdAccessRequest,
        _household_id: &HouseholdId,
        _expected_version: AggregateVersion,
        _member: &UserId,
    ) -> impl Future<Output = Result<Household, HouseholdRepositoryError>> + Send {
        async { Err(HouseholdRepositoryError::Unavailable) }
    }

    /// Makes an active member the owner; the previous owner stays a member. A target that is
    /// not an active member is `NotFound`. Transferring to oneself is a no-op.
    fn transfer_ownership(
        &self,
        _access: &HouseholdAccessRequest,
        _household_id: &HouseholdId,
        _expected_version: AggregateVersion,
        _new_owner: &UserId,
    ) -> impl Future<Output = Result<Household, HouseholdRepositoryError>> + Send {
        async { Err(HouseholdRepositoryError::Unavailable) }
    }

    /// Deletes the household (bounded cascade, ADR 017).
    ///
    /// When this returns `Ok`, the household, its memberships, its invitations and every
    /// household-owned list (with items) are inaccessible to everyone, and the members'
    /// household-limit slots are free. Adapters that cannot remove all children in one
    /// transaction must atomically mark the household as deleting together with the version
    /// check, treat it as nonexistent from then on, and remove the children in bounded,
    /// idempotent batches afterwards.
    fn delete_household(
        &self,
        _access: &HouseholdAccessRequest,
        _household_id: &HouseholdId,
        _expected_version: AggregateVersion,
    ) -> impl Future<Output = Result<(), HouseholdRepositoryError>> + Send {
        async { Err(HouseholdRepositoryError::Unavailable) }
    }
}

/// Persists invitations by token hash. Raw tokens never reach this port.
pub trait InvitationRepository: Send + Sync {
    /// Persists a new invitation for `invitation.household_id()`. Requires the household
    /// owner (`invitation.created_by()` must equal the principal, otherwise
    /// `InvalidRequest`). Enforces `Invitation::MAX_VALID_PER_HOUSEHOLD` valid invitations at
    /// `now` atomically with the write. A duplicate id or hash is `AlreadyExists`.
    fn create_invitation(
        &self,
        _access: &HouseholdAccessRequest,
        _invitation: Invitation,
        _now: Timestamp,
    ) -> impl Future<Output = Result<(), HouseholdRepositoryError>> + Send {
        async { Err(HouseholdRepositoryError::Unavailable) }
    }

    /// Every invitation of the household that has not been cleaned up, ordered by id.
    fn list_invitations(
        &self,
        _access: &HouseholdAccessRequest,
        _household_id: &HouseholdId,
    ) -> impl Future<Output = Result<Vec<Invitation>, HouseholdRepositoryError>> + Send {
        async { Err(HouseholdRepositoryError::Unavailable) }
    }

    /// Revokes an invitation of the household. Revoking twice is a no-op; an invitation of
    /// another household is `NotFound`.
    fn revoke_invitation(
        &self,
        _access: &HouseholdAccessRequest,
        _household_id: &HouseholdId,
        _invitation_id: &InvitationId,
    ) -> impl Future<Output = Result<Invitation, HouseholdRepositoryError>> + Send {
        async { Err(HouseholdRepositoryError::Unavailable) }
    }

    /// Accepts an invitation for the authenticated principal.
    ///
    /// In ONE atomic operation: the invitation must exist, not be revoked, not be expired at
    /// `now` and have uses left (otherwise `InvitationUnavailable`); the principal must not
    /// already be a member (`AlreadyMember`, no use consumed); the household must have fewer
    /// than `Household::MAX_MEMBERS` members (`MemberLimitReached`) and the principal fewer
    /// than `Household::MAX_HOUSEHOLDS_PER_USER` households (`HouseholdLimitReached`). Only
    /// then is the use count incremented and the membership created, together with the
    /// household version.
    fn accept_invitation(
        &self,
        _principal: &UserId,
        _token_hash: &InvitationTokenHash,
        _now: Timestamp,
    ) -> impl Future<Output = Result<HouseholdSummary, HouseholdRepositoryError>> + Send {
        async { Err(HouseholdRepositoryError::Unavailable) }
    }
}

/// Failure of the platform's secure random source. Callers must fail closed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenGenerationError;

/// Generates invitation tokens carrying at least 256 bits from a cryptographically secure
/// random source. Synchronous: generation must not perform network I/O.
pub trait InvitationTokenGenerator: Send + Sync {
    fn generate(&self) -> Result<InvitationToken, TokenGenerationError>;
}

/// Derives the persisted lookup hash (SHA-256, ADR 008/017) of an invitation token.
///
/// Implemented in the adapters crate because domain and application have no external
/// dependencies. Implementations must be deterministic and must not log their input.
pub trait InvitationTokenHasher: Send + Sync {
    fn hash(&self, token: &InvitationToken) -> InvitationTokenHash;
}

/// Current time for expiry decisions. The API never trusts client-supplied time.
pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

/// Generates household identifiers. Synchronous: generation must not perform I/O.
pub trait HouseholdIdGenerator: Send + Sync {
    fn generate(&self) -> String;
}

/// Generates invitation identifiers. They must be independent of the token.
pub trait InvitationIdGenerator: Send + Sync {
    fn generate(&self) -> String;
}

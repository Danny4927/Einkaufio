//! Household aggregate, roles and the household authorization rule table (roadmap C2/C3).
//!
//! A household has exactly one owner and up to [`Household::MAX_MEMBERS`] active members
//! (the owner included). Active membership grants access to every household-owned list
//! (ADR 005, ADR 015): the owner administers those lists, members contribute to them.

use crate::access::ListAccess;
use crate::identifiers::{HouseholdId, UserId};
use crate::shopping_list::AggregateVersion;

/// The role an active member holds in a household.
///
/// Ordered from weakest to strongest, like [`ListAccess`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum HouseholdRole {
    Member,
    Owner,
}

/// Permission a household operation requires.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum HouseholdPermission {
    /// Held by every active member.
    Participate,
    /// Held only by the owner.
    Administer,
}

/// Household operations that require authorization against an existing household.
///
/// Leaving is not listed: it is self-service for every member except the owner, who must
/// transfer ownership first (or delete the household).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum HouseholdOperation {
    /// Read the household and its member list.
    View,
    /// Create a household-owned list (subject to [`Household::MAX_LISTS`]).
    CreateList,
    Rename,
    /// Create, list and revoke invitations.
    ManageInvitations,
    RemoveMember,
    TransferOwnership,
    Delete,
}

impl HouseholdRole {
    /// Whether this role satisfies the required permission.
    pub const fn allows(self, permission: HouseholdPermission) -> bool {
        match (self, permission) {
            (Self::Owner, _) | (Self::Member, HouseholdPermission::Participate) => true,
            (Self::Member, HouseholdPermission::Administer) => false,
        }
    }

    /// Effective access this role grants on every list owned by the household (ADR 015).
    pub const fn list_access(self) -> ListAccess {
        match self {
            Self::Owner => ListAccess::Administrator,
            Self::Member => ListAccess::Contributor,
        }
    }
}

impl HouseholdOperation {
    /// The household authorization rule table.
    pub const fn required_permission(self) -> HouseholdPermission {
        match self {
            Self::View | Self::CreateList => HouseholdPermission::Participate,
            Self::Rename
            | Self::ManageInvitations
            | Self::RemoveMember
            | Self::TransferOwnership
            | Self::Delete => HouseholdPermission::Administer,
        }
    }
}

/// A validated household name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HouseholdName(String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HouseholdNameError {
    Empty,
    TooLong,
    ContainsControlCharacters,
}

impl HouseholdName {
    pub const MAX_LENGTH: usize = 80;

    pub fn try_new(value: impl Into<String>) -> Result<Self, HouseholdNameError> {
        let normalized = value.into().trim().to_owned();
        if normalized.is_empty() {
            return Err(HouseholdNameError::Empty);
        }
        if normalized.chars().count() > Self::MAX_LENGTH {
            return Err(HouseholdNameError::TooLong);
        }
        if normalized.chars().any(char::is_control) {
            return Err(HouseholdNameError::ContainsControlCharacters);
        }
        Ok(Self(normalized))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HouseholdMutationError {
    VersionExhausted,
}

/// Household aggregate root. Membership records are persisted alongside it; the owner is
/// always an active member.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Household {
    id: HouseholdId,
    name: HouseholdName,
    owner_id: UserId,
    version: AggregateVersion,
}

impl Household {
    /// Maximum active members, owner included (roadmap "Initial product limits").
    pub const MAX_MEMBERS: usize = 10;
    /// Maximum lists owned by one household.
    pub const MAX_LISTS: usize = 20;
    /// Maximum households one user can be an active member of (owned or joined). Bounds the
    /// accessible-list query (ADR 015, ADR 017).
    pub const MAX_HOUSEHOLDS_PER_USER: usize = 5;

    /// Creates a household owned by `owner_id` at its initial version.
    pub fn new(id: HouseholdId, owner_id: UserId, name: HouseholdName) -> Self {
        Self {
            id,
            name,
            owner_id,
            version: AggregateVersion::initial(),
        }
    }

    /// Reconstitutes a household from persisted state (persistence adapters only).
    ///
    /// Performs no business transition and does not advance the version.
    pub fn restore(
        id: HouseholdId,
        name: HouseholdName,
        owner_id: UserId,
        version: AggregateVersion,
    ) -> Self {
        Self {
            id,
            name,
            owner_id,
            version,
        }
    }

    pub fn id(&self) -> &HouseholdId {
        &self.id
    }

    pub fn name(&self) -> &HouseholdName {
        &self.name
    }

    pub fn owner_id(&self) -> &UserId {
        &self.owner_id
    }

    pub const fn version(&self) -> AggregateVersion {
        self.version
    }

    /// The role of an active member. Callers must only pass verified active members.
    pub fn role_of_member(&self, member: &UserId) -> HouseholdRole {
        if &self.owner_id == member {
            HouseholdRole::Owner
        } else {
            HouseholdRole::Member
        }
    }

    /// Renames the household; advances the version only when the name changes.
    ///
    /// Adapters compare the expected version before calling this, so a stale no-op rename is
    /// still a conflict.
    pub fn rename(&mut self, name: HouseholdName) -> Result<(), HouseholdMutationError> {
        if self.name == name {
            return Ok(());
        }
        self.version = self.next_version()?;
        self.name = name;
        Ok(())
    }

    /// Makes an active member the owner; the previous owner stays an active member.
    ///
    /// Transferring to the current owner is a no-op. Adapters must verify that `new_owner`
    /// is an active member in the same atomic operation.
    pub fn transfer_ownership(&mut self, new_owner: UserId) -> Result<(), HouseholdMutationError> {
        if self.owner_id == new_owner {
            return Ok(());
        }
        self.version = self.next_version()?;
        self.owner_id = new_owner;
        Ok(())
    }

    /// Advances the version for a visible membership change (join, leave, removal).
    pub fn record_membership_change(&mut self) -> Result<(), HouseholdMutationError> {
        self.version = self.next_version()?;
        Ok(())
    }

    fn next_version(&self) -> Result<AggregateVersion, HouseholdMutationError> {
        self.version
            .checked_next()
            .map_err(|_| HouseholdMutationError::VersionExhausted)
    }
}

/// An active member of a household as seen by other members.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HouseholdMember {
    pub user_id: UserId,
    pub role: HouseholdRole,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn household() -> Household {
        Household::new(
            HouseholdId::try_new("household-1").unwrap(),
            UserId::try_new("owner").unwrap(),
            HouseholdName::try_new("Familie").unwrap(),
        )
    }

    #[test]
    fn rule_table_reserves_administration_for_the_owner() {
        for operation in [HouseholdOperation::View, HouseholdOperation::CreateList] {
            assert_eq!(
                operation.required_permission(),
                HouseholdPermission::Participate
            );
            assert!(HouseholdRole::Member.allows(operation.required_permission()));
        }
        for operation in [
            HouseholdOperation::Rename,
            HouseholdOperation::ManageInvitations,
            HouseholdOperation::RemoveMember,
            HouseholdOperation::TransferOwnership,
            HouseholdOperation::Delete,
        ] {
            assert_eq!(
                operation.required_permission(),
                HouseholdPermission::Administer
            );
            assert!(!HouseholdRole::Member.allows(operation.required_permission()));
            assert!(HouseholdRole::Owner.allows(operation.required_permission()));
        }
    }

    #[test]
    fn household_roles_map_to_list_access() {
        assert_eq!(
            HouseholdRole::Owner.list_access(),
            ListAccess::Administrator
        );
        assert_eq!(HouseholdRole::Member.list_access(), ListAccess::Contributor);
    }

    #[test]
    fn household_name_is_validated() {
        assert_eq!(HouseholdName::try_new("  WG  ").unwrap().as_str(), "WG");
        assert_eq!(HouseholdName::try_new(" "), Err(HouseholdNameError::Empty));
        assert_eq!(
            HouseholdName::try_new("x".repeat(81)),
            Err(HouseholdNameError::TooLong)
        );
        assert_eq!(
            HouseholdName::try_new("a\u{7}b"),
            Err(HouseholdNameError::ContainsControlCharacters)
        );
    }

    #[test]
    fn rename_and_transfer_advance_the_version_only_on_change() {
        let mut household = household();
        household
            .rename(HouseholdName::try_new("Familie").unwrap())
            .unwrap();
        assert_eq!(household.version().as_u64(), 1);
        household
            .rename(HouseholdName::try_new("WG").unwrap())
            .unwrap();
        assert_eq!(household.version().as_u64(), 2);

        household
            .transfer_ownership(UserId::try_new("owner").unwrap())
            .unwrap();
        assert_eq!(household.version().as_u64(), 2);
        let member = UserId::try_new("member").unwrap();
        household.transfer_ownership(member.clone()).unwrap();
        assert_eq!(household.version().as_u64(), 3);
        assert_eq!(household.role_of_member(&member), HouseholdRole::Owner);
        assert_eq!(
            household.role_of_member(&UserId::try_new("owner").unwrap()),
            HouseholdRole::Member
        );
    }

    #[test]
    fn exhausted_version_leaves_the_household_unchanged() {
        let mut household = household();
        household.version = AggregateVersion::try_new(u64::MAX).unwrap();
        assert_eq!(
            household.rename(HouseholdName::try_new("Neu").unwrap()),
            Err(HouseholdMutationError::VersionExhausted)
        );
        assert_eq!(
            household.record_membership_change(),
            Err(HouseholdMutationError::VersionExhausted)
        );
        assert_eq!(household.name().as_str(), "Familie");
    }

    #[test]
    fn restore_reconstitutes_owner_and_version() {
        let restored = Household::restore(
            HouseholdId::try_new("household-1").unwrap(),
            HouseholdName::try_new("WG").unwrap(),
            UserId::try_new("new-owner").unwrap(),
            AggregateVersion::try_new(5).unwrap(),
        );
        assert_eq!(restored.version().as_u64(), 5);
        assert_eq!(
            restored.role_of_member(&UserId::try_new("new-owner").unwrap()),
            HouseholdRole::Owner
        );
        assert_eq!(restored.name().as_str(), "WG");
    }
}

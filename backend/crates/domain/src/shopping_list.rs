//! Shopping-list aggregate and its invariants.

use crate::access::ListAccess;
use crate::household::HouseholdRole;
use crate::identifiers::{HouseholdId, ListId, UserId};

/// A positive, monotonically increasing aggregate version.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AggregateVersion(u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AggregateVersionError {
    Zero,
}

impl AggregateVersion {
    pub fn try_new(value: u64) -> Result<Self, AggregateVersionError> {
        if value == 0 {
            return Err(AggregateVersionError::Zero);
        }
        Ok(Self(value))
    }

    pub const fn initial() -> Self {
        Self(1)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    pub fn checked_next(self) -> Result<Self, ListMutationError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(ListMutationError::VersionExhausted)
    }
}

/// A validated shopping-list name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListName(String);

/// Who owns a shopping list (ADR 005, ADR 015).
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ListOwner {
    /// A personal list; its owner administers it.
    Personal(UserId),
    /// A household-owned list; access follows active household membership.
    Household(HouseholdId),
}

/// A shopping list owned by one user or by one household.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShoppingList {
    id: ListId,
    owner: ListOwner,
    name: ListName,
    version: AggregateVersion,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListNameError {
    Empty,
    TooLong,
    ContainsControlCharacters,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListMutationError {
    VersionExhausted,
}

impl ListName {
    pub const MAX_LENGTH: usize = 120;

    pub fn try_new(value: impl Into<String>) -> Result<Self, ListNameError> {
        let normalized = value.into().trim().to_owned();
        if normalized.is_empty() {
            return Err(ListNameError::Empty);
        }
        if normalized.chars().count() > Self::MAX_LENGTH {
            return Err(ListNameError::TooLong);
        }
        if normalized.chars().any(char::is_control) {
            return Err(ListNameError::ContainsControlCharacters);
        }
        Ok(Self(normalized))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl ShoppingList {
    /// Maximum number of active items in one shopping list.
    pub const MAX_ACTIVE_ITEMS: usize = 100;

    /// Creates a personal list at its initial aggregate version.
    pub fn new_personal(id: ListId, owner_id: UserId, name: ListName) -> Self {
        Self {
            id,
            owner: ListOwner::Personal(owner_id),
            name,
            version: AggregateVersion::initial(),
        }
    }

    /// Creates a household-owned list at its initial aggregate version.
    pub fn new_household(id: ListId, household_id: HouseholdId, name: ListName) -> Self {
        Self {
            id,
            owner: ListOwner::Household(household_id),
            name,
            version: AggregateVersion::initial(),
        }
    }

    /// Advances the aggregate version for an item mutation in this list.
    ///
    /// The adapter must call this within the same atomic operation as the item write.
    pub fn record_item_mutation(&mut self) -> Result<(), ListMutationError> {
        self.version = self.version.checked_next()?;
        Ok(())
    }

    /// Renames this aggregate and advances its version when the name changes.
    ///
    /// Persistence adapters must compare the supplied expected version with the stored
    /// aggregate version before invoking this method, including for an unchanged name.
    /// That ordering ensures a stale no-op rename still returns a version conflict.
    pub fn rename(&mut self, name: ListName) -> Result<(), ListMutationError> {
        if self.name == name {
            return Ok(());
        }

        self.version = self.version.checked_next()?;
        self.name = name;
        Ok(())
    }

    pub fn id(&self) -> &ListId {
        &self.id
    }

    pub fn owner(&self) -> &ListOwner {
        &self.owner
    }

    /// The owner of a personal list; `None` for household-owned lists.
    pub fn personal_owner(&self) -> Option<&UserId> {
        match &self.owner {
            ListOwner::Personal(owner) => Some(owner),
            ListOwner::Household(_) => None,
        }
    }

    /// The owning household of a household list; `None` for personal lists.
    pub fn household_id(&self) -> Option<&HouseholdId> {
        match &self.owner {
            ListOwner::Personal(_) => None,
            ListOwner::Household(household_id) => Some(household_id),
        }
    }

    /// Resolves the principal's effective access (ADR 015).
    ///
    /// `household_role` is the principal's role in the household owning this list, read by
    /// the adapter from persisted membership records; it is ignored for personal lists, so a
    /// membership elsewhere can never grant access to a personal list.
    pub fn access_for(
        &self,
        principal: &UserId,
        household_role: Option<HouseholdRole>,
    ) -> Option<ListAccess> {
        match &self.owner {
            ListOwner::Personal(owner) => (owner == principal).then_some(ListAccess::Administrator),
            ListOwner::Household(_) => household_role.map(HouseholdRole::list_access),
        }
    }

    pub fn name(&self) -> &ListName {
        &self.name
    }

    /// Returns the aggregate version as its transport/storage representation.
    pub const fn version(&self) -> u64 {
        self.version.as_u64()
    }

    pub const fn aggregate_version(&self) -> AggregateVersion {
        self.version
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AggregateVersion, AggregateVersionError, ListMutationError, ListName, ListNameError,
        ShoppingList,
    };
    use crate::access::ListAccess;
    use crate::household::HouseholdRole;
    use crate::identifiers::{HouseholdId, ListId, UserId};

    fn list() -> ShoppingList {
        ShoppingList::new_personal(
            ListId::try_new("list-1").expect("valid list id"),
            UserId::try_new("user-1").expect("valid user id"),
            ListName::try_new("Einkauf").expect("valid list name"),
        )
    }

    #[test]
    fn aggregate_version_rejects_zero_and_accepts_positive_values() {
        assert_eq!(
            AggregateVersion::try_new(0),
            Err(AggregateVersionError::Zero)
        );
        assert_eq!(AggregateVersion::try_new(1).unwrap().as_u64(), 1);
    }

    #[test]
    fn list_name_is_trimmed() {
        let name = ListName::try_new("  Wocheneinkauf  ").expect("valid name");
        assert_eq!(name.as_str(), "Wocheneinkauf");
    }

    #[test]
    fn list_name_rejects_empty_and_overlong_values() {
        assert_eq!(ListName::try_new("  "), Err(ListNameError::Empty));
        assert_eq!(
            ListName::try_new("x".repeat(121)),
            Err(ListNameError::TooLong)
        );
    }

    #[test]
    fn list_name_rejects_control_characters() {
        assert_eq!(
            ListName::try_new("Wochen\neinkauf"),
            Err(ListNameError::ContainsControlCharacters)
        );
    }

    #[test]
    fn personal_list_starts_at_version_one() {
        let list = list();
        assert_eq!(list.version(), 1);
        assert_eq!(list.aggregate_version(), AggregateVersion::initial());
        assert_eq!(list.name().as_str(), "Einkauf");
        assert_eq!(list.personal_owner().unwrap().as_str(), "user-1");
        assert_eq!(list.household_id(), None);
    }

    #[test]
    fn personal_owner_administers_the_list_and_others_get_no_ownership_access() {
        let list = list();
        assert_eq!(
            list.access_for(&UserId::try_new("user-1").unwrap(), None),
            Some(ListAccess::Administrator)
        );
        assert_eq!(
            list.access_for(&UserId::try_new("user-2").unwrap(), None),
            None
        );
        // A household role never grants access to a personal list.
        assert_eq!(
            list.access_for(
                &UserId::try_new("user-2").unwrap(),
                Some(HouseholdRole::Owner)
            ),
            None
        );
    }

    #[test]
    fn household_list_access_follows_the_household_role() {
        let list = ShoppingList::new_household(
            ListId::try_new("list-2").unwrap(),
            HouseholdId::try_new("household-1").unwrap(),
            ListName::try_new("WG").unwrap(),
        );
        let user = UserId::try_new("user-1").unwrap();
        assert_eq!(list.personal_owner(), None);
        assert_eq!(list.household_id().unwrap().as_str(), "household-1");
        assert_eq!(list.version(), 1);
        assert_eq!(list.access_for(&user, None), None);
        assert_eq!(
            list.access_for(&user, Some(HouseholdRole::Member)),
            Some(ListAccess::Contributor)
        );
        assert_eq!(
            list.access_for(&user, Some(HouseholdRole::Owner)),
            Some(ListAccess::Administrator)
        );
    }

    #[test]
    fn rename_updates_name_and_increments_aggregate_version() {
        let mut list = list();

        list.rename(ListName::try_new("  Drogerie ").unwrap())
            .expect("rename should succeed");

        assert_eq!(list.name().as_str(), "Drogerie");
        assert_eq!(list.version(), 2);
    }

    #[test]
    fn renaming_to_same_name_is_idempotent() {
        let mut list = list();

        list.rename(ListName::try_new("Einkauf").unwrap())
            .expect("no-op rename should succeed");

        assert_eq!(list.version(), 1);
    }

    #[test]
    fn rename_does_not_mutate_when_version_is_exhausted() {
        let mut list = list();
        list.version = AggregateVersion::try_new(u64::MAX).unwrap();

        assert_eq!(
            list.rename(ListName::try_new("Neu").unwrap()),
            Err(ListMutationError::VersionExhausted)
        );
        assert_eq!(list.name().as_str(), "Einkauf");
        assert_eq!(list.version(), u64::MAX);
    }
}

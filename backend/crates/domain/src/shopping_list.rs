//! Shopping-list aggregate and its invariants.

use crate::identifiers::{ListId, UserId};

/// A validated shopping-list name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListName(String);

/// A personal shopping list owned by one user.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShoppingList {
    id: ListId,
    owner_id: UserId,
    name: ListName,
    version: u64,
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
            owner_id,
            name,
            version: 1,
        }
    }

    /// Renames this aggregate and advances its version when the name changes.
    ///
    /// The persistence adapter must still compare the caller's expected version and
    /// commit this state atomically with its stored representation.
    pub fn rename(&mut self, name: ListName) -> Result<(), ListMutationError> {
        if self.name == name {
            return Ok(());
        }

        self.version = self
            .version
            .checked_add(1)
            .ok_or(ListMutationError::VersionExhausted)?;
        self.name = name;
        Ok(())
    }

    pub fn id(&self) -> &ListId {
        &self.id
    }

    pub fn owner_id(&self) -> &UserId {
        &self.owner_id
    }

    pub fn name(&self) -> &ListName {
        &self.name
    }

    pub fn version(&self) -> u64 {
        self.version
    }
}

#[cfg(test)]
mod tests {
    use super::{ListMutationError, ListName, ListNameError, ShoppingList};
    use crate::identifiers::{ListId, UserId};

    fn list() -> ShoppingList {
        ShoppingList::new_personal(
            ListId::try_new("list-1").expect("valid list id"),
            UserId::try_new("user-1").expect("valid user id"),
            ListName::try_new("Einkauf").expect("valid list name"),
        )
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
        assert_eq!(list.name().as_str(), "Einkauf");
        assert_eq!(list.owner_id().as_str(), "user-1");
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
        list.version = u64::MAX;

        assert_eq!(
            list.rename(ListName::try_new("Neu").unwrap()),
            Err(ListMutationError::VersionExhausted)
        );
        assert_eq!(list.name().as_str(), "Einkauf");
        assert_eq!(list.version(), u64::MAX);
    }
}

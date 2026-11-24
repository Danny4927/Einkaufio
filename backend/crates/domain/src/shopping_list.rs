//! Shopping-list aggregate and its invariants.

use crate::identifiers::{ItemId, ListId, UserId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListName(String);

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
pub enum ShoppingListMutationError {
    ItemLimitReached,
    ItemNotFound,
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
    pub const MAX_ACTIVE_ITEMS: usize = 100;

    pub fn new_personal(id: ListId, owner_id: UserId, name: ListName) -> Self {
        Self {
            id,
            owner_id,
            name,
            version: 1,
        }
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

    /// A committed item mutation advances the aggregate version once.
    pub fn record_item_mutation(&mut self) {
        self.version += 1;
    }

    pub fn ensure_item_capacity(active_item_count: usize) -> Result<(), ShoppingListMutationError> {
        if active_item_count >= Self::MAX_ACTIVE_ITEMS {
            Err(ShoppingListMutationError::ItemLimitReached)
        } else {
            Ok(())
        }
    }

    pub fn ensure_item_exists(
        item_id: &ItemId,
        existing_item_ids: &[ItemId],
    ) -> Result<(), ShoppingListMutationError> {
        if existing_item_ids.iter().any(|id| id == item_id) {
            Ok(())
        } else {
            Err(ShoppingListMutationError::ItemNotFound)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ListName, ListNameError, ShoppingList, ShoppingListMutationError};
    use crate::identifiers::{ItemId, ListId, UserId};

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
        let list = ShoppingList::new_personal(
            ListId::try_new("list-1").unwrap(),
            UserId::try_new("user-1").unwrap(),
            ListName::try_new("Einkauf").unwrap(),
        );
        assert_eq!(list.version(), 1);
        assert_eq!(list.name().as_str(), "Einkauf");
        assert_eq!(list.owner_id().as_str(), "user-1");
    }

    #[test]
    fn item_mutation_advances_list_version() {
        let mut list = ShoppingList::new_personal(
            ListId::try_new("list-1").unwrap(),
            UserId::try_new("user-1").unwrap(),
            ListName::try_new("Einkauf").unwrap(),
        );
        list.record_item_mutation();
        assert_eq!(list.version(), 2);
    }

    #[test]
    fn enforces_initial_active_item_limit() {
        assert!(ShoppingList::ensure_item_capacity(99).is_ok());
        assert_eq!(
            ShoppingList::ensure_item_capacity(100),
            Err(ShoppingListMutationError::ItemLimitReached)
        );
    }

    #[test]
    fn rejects_missing_item_ids() {
        let id = ItemId::try_new("item-1").unwrap();
        assert_eq!(
            ShoppingList::ensure_item_exists(&id, &[]),
            Err(ShoppingListMutationError::ItemNotFound)
        );
        assert!(ShoppingList::ensure_item_exists(&id, &[id.clone()]).is_ok());
    }
}

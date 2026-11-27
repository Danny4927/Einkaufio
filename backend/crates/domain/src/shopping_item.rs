//! Shopping-item entity and value objects.

use crate::identifiers::{ItemId, UserId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemName(String);
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemQuantity(String);
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemUnit(String);
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemNote(String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ItemValueError {
    Empty,
    TooLong,
    ContainsControlCharacters,
}

/// Item mutation failures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ItemMutationError {
    /// The item version reached the numeric limit and cannot advance.
    VersionExhausted,
}

/// Validated, client-editable item attributes.
///
/// Check state, position, version and creator are deliberately excluded: they are changed only
/// through dedicated operations or assigned by the repository.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemDetails {
    pub name: ItemName,
    pub quantity: Option<ItemQuantity>,
    pub unit: Option<ItemUnit>,
    pub note: Option<ItemNote>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShoppingItem {
    id: ItemId,
    name: ItemName,
    quantity: Option<ItemQuantity>,
    unit: Option<ItemUnit>,
    note: Option<ItemNote>,
    checked: bool,
    position: u32,
    version: u64,
    created_by: UserId,
}

macro_rules! validated_text {
    ($type:ident, $max:expr) => {
        impl $type {
            pub const MAX_LENGTH: usize = $max;

            pub fn try_new(value: impl Into<String>) -> Result<Self, ItemValueError> {
                let normalized = value.into().trim().to_owned();
                if normalized.is_empty() {
                    return Err(ItemValueError::Empty);
                }
                if normalized.chars().count() > Self::MAX_LENGTH {
                    return Err(ItemValueError::TooLong);
                }
                if normalized.chars().any(char::is_control) {
                    return Err(ItemValueError::ContainsControlCharacters);
                }
                Ok(Self(normalized))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

validated_text!(ItemName, 120);
validated_text!(ItemQuantity, 40);
validated_text!(ItemUnit, 40);
validated_text!(ItemNote, 500);

impl ShoppingItem {
    pub fn new(
        id: ItemId,
        name: ItemName,
        quantity: Option<ItemQuantity>,
        unit: Option<ItemUnit>,
        note: Option<ItemNote>,
        position: u32,
        created_by: UserId,
    ) -> Self {
        Self {
            id,
            name,
            quantity,
            unit,
            note,
            checked: false,
            position,
            version: 1,
            created_by,
        }
    }

    pub fn id(&self) -> &ItemId {
        &self.id
    }

    pub fn name(&self) -> &ItemName {
        &self.name
    }

    pub fn quantity(&self) -> Option<&ItemQuantity> {
        self.quantity.as_ref()
    }

    pub fn unit(&self) -> Option<&ItemUnit> {
        self.unit.as_ref()
    }

    pub fn note(&self) -> Option<&ItemNote> {
        self.note.as_ref()
    }

    pub fn checked(&self) -> bool {
        self.checked
    }

    pub fn position(&self) -> u32 {
        self.position
    }

    /// Returns this item with a repository-assigned position.
    ///
    /// Position is persistence/list-order metadata and must not be trusted from clients.
    pub fn with_position(mut self, position: u32) -> Self {
        self.position = position;
        self
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn created_by(&self) -> &UserId {
        &self.created_by
    }

    /// Applies a check-state change and advances the item version only when needed.
    ///
    /// On version exhaustion the item is left unchanged.
    pub fn set_checked(&mut self, checked: bool) -> Result<bool, ItemMutationError> {
        if self.checked == checked {
            return Ok(false);
        }
        self.version = self.next_version()?;
        self.checked = checked;
        Ok(true)
    }

    fn next_version(&self) -> Result<u64, ItemMutationError> {
        self.version
            .checked_add(1)
            .ok_or(ItemMutationError::VersionExhausted)
    }

    /// Returns the current editable attributes.
    pub fn details(&self) -> ItemDetails {
        ItemDetails {
            name: self.name.clone(),
            quantity: self.quantity.clone(),
            unit: self.unit.clone(),
            note: self.note.clone(),
        }
    }

    /// Replaces the editable attributes and advances the item version only when they change.
    ///
    /// On version exhaustion the item is left unchanged.
    pub fn update_details(&mut self, details: ItemDetails) -> Result<bool, ItemMutationError> {
        if self.details() == details {
            return Ok(false);
        }
        self.version = self.next_version()?;
        self.name = details.name;
        self.quantity = details.quantity;
        self.unit = details.unit;
        self.note = details.note;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identifiers::{ItemId, UserId};

    fn item() -> ShoppingItem {
        ShoppingItem::new(
            ItemId::try_new("item-1").unwrap(),
            ItemName::try_new(" Milch ").unwrap(),
            Some(ItemQuantity::try_new("2").unwrap()),
            Some(ItemUnit::try_new(" Liter ").unwrap()),
            None,
            0,
            UserId::try_new("user-1").unwrap(),
        )
    }

    #[test]
    fn values_are_normalized_and_bounded() {
        assert_eq!(ItemName::try_new("  Milch ").unwrap().as_str(), "Milch");
        assert_eq!(
            ItemQuantity::try_new("x".repeat(41)),
            Err(ItemValueError::TooLong)
        );
        assert_eq!(ItemUnit::try_new(" "), Err(ItemValueError::Empty));
        assert_eq!(
            ItemNote::try_new("a\nb"),
            Err(ItemValueError::ContainsControlCharacters)
        );
    }

    #[test]
    fn new_item_starts_unchecked_at_version_one() {
        let item = item();
        assert!(!item.checked());
        assert_eq!(item.version(), 1);
        assert_eq!(item.name().as_str(), "Milch");
        assert_eq!(item.quantity().unwrap().as_str(), "2");
        assert_eq!(item.unit().unwrap().as_str(), "Liter");
    }

    #[test]
    fn checking_changes_state_and_increments_once() {
        let mut item = item();
        assert_eq!(item.set_checked(true), Ok(true));
        assert!(item.checked());
        assert_eq!(item.version(), 2);
        assert_eq!(item.set_checked(true), Ok(false));
        assert_eq!(item.version(), 2);
        assert_eq!(item.set_checked(false), Ok(true));
        assert_eq!(item.version(), 3);
    }

    #[test]
    fn updating_details_changes_attributes_and_increments_once() {
        let mut item = item();
        item.set_checked(true).unwrap();
        let details = ItemDetails {
            name: ItemName::try_new("Hafermilch").unwrap(),
            quantity: None,
            unit: Some(ItemUnit::try_new("Packung").unwrap()),
            note: Some(ItemNote::try_new("ungesüßt").unwrap()),
        };
        assert_eq!(item.update_details(details.clone()), Ok(true));
        assert_eq!(item.details(), details);
        assert_eq!(item.version(), 3);
        assert!(item.checked(), "check state is not part of the details");
        assert_eq!(item.position(), 0);
        assert_eq!(item.update_details(details), Ok(false));
        assert_eq!(item.version(), 3);
    }

    #[test]
    fn exhausted_version_fails_without_changing_the_item() {
        let mut item = item();
        item.version = u64::MAX;
        let before = item.clone();
        assert_eq!(
            item.set_checked(true),
            Err(ItemMutationError::VersionExhausted)
        );
        let details = ItemDetails {
            name: ItemName::try_new("Brot").unwrap(),
            ..item.details()
        };
        assert_eq!(
            item.update_details(details),
            Err(ItemMutationError::VersionExhausted)
        );
        assert_eq!(item, before);
        // No-op changes never need a new version, so they still succeed at the limit.
        assert_eq!(item.set_checked(false), Ok(false));
        assert_eq!(item.update_details(before.details()), Ok(false));
    }
}

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

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn created_by(&self) -> &UserId {
        &self.created_by
    }

    pub fn set_checked(&mut self, checked: bool) -> bool {
        if self.checked == checked {
            return false;
        }
        self.checked = checked;
        self.version += 1;
        true
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
        let i = item();
        assert!(!i.checked());
        assert_eq!(i.version(), 1);
        assert_eq!(i.name().as_str(), "Milch");
        assert_eq!(i.quantity().unwrap().as_str(), "2");
        assert_eq!(i.unit().unwrap().as_str(), "Liter");
    }

    #[test]
    fn checking_changes_state_and_increments_once() {
        let mut i = item();
        assert!(i.set_checked(true));
        assert!(i.checked());
        assert_eq!(i.version(), 2);
        assert!(!i.set_checked(true));
        assert_eq!(i.version(), 2);
        assert!(i.set_checked(false));
        assert_eq!(i.version(), 3);
    }
}

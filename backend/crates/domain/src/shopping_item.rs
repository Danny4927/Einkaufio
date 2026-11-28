//! Shopping-item entity and value objects.

use crate::identifiers::{ItemId, UserId};
use crate::shopping_list::AggregateVersion;

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

    /// Reconstitutes an item from persisted state (persistence adapters only).
    ///
    /// Performs no business transition. `version` is the persisted item version, which is
    /// positive by construction (`AggregateVersion`).
    pub fn restore(
        id: ItemId,
        details: ItemDetails,
        checked: bool,
        position: u32,
        version: AggregateVersion,
        created_by: UserId,
    ) -> Self {
        Self {
            id,
            name: details.name,
            quantity: details.quantity,
            unit: details.unit,
            note: details.note,
            checked,
            position,
            version: version.as_u64(),
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

    /// Moves the item to a repository-assigned position, advancing its version only when the
    /// position changes. On version exhaustion the item is left unchanged.
    pub fn move_to(&mut self, position: u32) -> Result<bool, ItemMutationError> {
        if self.position == position {
            return Ok(false);
        }
        self.version = self.next_version()?;
        self.position = position;
        Ok(true)
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

/// Errors of a requested item order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ItemOrderError {
    DuplicateItem,
    TooManyItems,
}

/// A requested complete order of a list's items: unique ids, at most
/// [`ItemOrder::MAX_ITEMS`] entries. Index `n` becomes position `n`.
///
/// Whether the order names exactly the list's current items can only be decided atomically
/// by the repository.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemOrder(Vec<ItemId>);

impl ItemOrder {
    /// Equal to the active-item limit of a list.
    pub const MAX_ITEMS: usize = 100;

    pub fn try_new(item_ids: Vec<ItemId>) -> Result<Self, ItemOrderError> {
        if item_ids.len() > Self::MAX_ITEMS {
            return Err(ItemOrderError::TooManyItems);
        }
        let mut seen = std::collections::HashSet::with_capacity(item_ids.len());
        if !item_ids.iter().all(|id| seen.insert(id)) {
            return Err(ItemOrderError::DuplicateItem);
        }
        Ok(Self(item_ids))
    }

    pub fn item_ids(&self) -> &[ItemId] {
        &self.0
    }

    /// The position assigned to `item_id`, if the order contains it.
    pub fn position_of(&self, item_id: &ItemId) -> Option<u32> {
        self.0
            .iter()
            .position(|candidate| candidate == item_id)
            .and_then(|index| u32::try_from(index).ok())
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
    fn moving_changes_position_and_increments_once() {
        let mut item = item();
        assert_eq!(item.move_to(3), Ok(true));
        assert_eq!(item.position(), 3);
        assert_eq!(item.version(), 2);
        assert_eq!(item.move_to(3), Ok(false));
        assert_eq!(item.version(), 2);
        item.version = u64::MAX;
        assert_eq!(item.move_to(1), Err(ItemMutationError::VersionExhausted));
        assert_eq!(item.position(), 3);
    }

    #[test]
    fn item_order_rejects_duplicates_and_oversize_orders() {
        let id = |value: &str| ItemId::try_new(value).unwrap();
        let order = ItemOrder::try_new(vec![id("b"), id("a")]).unwrap();
        assert_eq!(order.position_of(&id("a")), Some(1));
        assert_eq!(order.position_of(&id("c")), None);
        assert_eq!(
            ItemOrder::try_new(vec![id("a"), id("b"), id("a")]),
            Err(ItemOrderError::DuplicateItem)
        );
        let many = (0..=ItemOrder::MAX_ITEMS)
            .map(|index| id(&format!("item-{index}")))
            .collect();
        assert_eq!(ItemOrder::try_new(many), Err(ItemOrderError::TooManyItems));
        assert!(
            ItemOrder::try_new(Vec::new())
                .unwrap()
                .item_ids()
                .is_empty()
        );
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

    #[test]
    fn restore_reconstitutes_check_state_position_and_version() {
        let details = ItemDetails {
            name: ItemName::try_new("Milch").unwrap(),
            quantity: Some(ItemQuantity::try_new("2").unwrap()),
            unit: Some(ItemUnit::try_new("l").unwrap()),
            note: Some(ItemNote::try_new("Bio").unwrap()),
        };
        let mut restored = ShoppingItem::restore(
            ItemId::try_new("item-1").unwrap(),
            details.clone(),
            true,
            4,
            AggregateVersion::try_new(9).unwrap(),
            UserId::try_new("creator").unwrap(),
        );
        assert!(restored.checked());
        assert_eq!(restored.position(), 4);
        assert_eq!(restored.version(), 9);
        assert_eq!(restored.details(), details);
        assert_eq!(restored.created_by().as_str(), "creator");
        // Restored items continue their version history.
        assert_eq!(restored.set_checked(false), Ok(true));
        assert_eq!(restored.version(), 10);
    }
}

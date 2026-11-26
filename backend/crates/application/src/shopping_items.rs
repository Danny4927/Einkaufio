//! Application use cases for shopping-list item mutations.

use einkaufio_domain::{
    AggregateVersion, ItemId, ItemName, ItemNote, ItemQuantity, ItemUnit, ItemValueError, ListId, ShoppingItem,
    UserId,
};

use crate::ports::{ItemIdGenerator, RepositoryError, ShoppingItemRepository};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddShoppingItemCommand {
    /// Derived from verified authentication context, never accepted from the client body.
    pub actor_id: String,
    pub list_id: String,
    pub expected_list_version: u64,
    pub name: String,
    pub quantity: Option<String>,
    pub unit: Option<String>,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShoppingItemError {
    InvalidActor,
    InvalidListId,
    InvalidItemId,
    InvalidName(ItemValueError),
    InvalidQuantity(ItemValueError),
    InvalidUnit(ItemValueError),
    InvalidNote(ItemValueError),
    InvalidExpectedVersion,
    ItemLimitReached,
    VersionExhausted,
    NotFound,
    Conflict,
    PersistenceUnavailable,
}

pub struct AddShoppingItem<'a> {
    repository: &'a dyn ShoppingItemRepository,
    id_generator: &'a dyn ItemIdGenerator,
}

impl<'a> AddShoppingItem<'a> {
    pub fn new(
        repository: &'a dyn ShoppingItemRepository,
        id_generator: &'a dyn ItemIdGenerator,
    ) -> Self {
        Self {
            repository,
            id_generator,
        }
    }

    pub fn execute(
        &self,
        command: AddShoppingItemCommand,
    ) -> Result<(ShoppingItem, u64), ShoppingItemError> {
        let actor =
            UserId::try_new(command.actor_id).map_err(|_| ShoppingItemError::InvalidActor)?;
        let list_id =
            ListId::try_new(command.list_id).map_err(|_| ShoppingItemError::InvalidListId)?;
        if command.expected_list_version == 0 {
            return Err(ShoppingItemError::InvalidExpectedVersion);
        }
        let name = ItemName::try_new(command.name).map_err(ShoppingItemError::InvalidName)?;
        // Empty optional form fields are normalized to None before constructing value objects.
        let quantity = optional_value(command.quantity)
            .map(ItemQuantity::try_new)
            .transpose()
            .map_err(ShoppingItemError::InvalidQuantity)?;
        let unit = optional_value(command.unit)
            .map(ItemUnit::try_new)
            .transpose()
            .map_err(ShoppingItemError::InvalidUnit)?;
        let note = optional_value(command.note)
            .map(ItemNote::try_new)
            .transpose()
            .map_err(ShoppingItemError::InvalidNote)?;
        let item_id = ItemId::try_new(self.id_generator.generate())
            .map_err(|_| ShoppingItemError::InvalidItemId)?;
        // Position is deliberately not client-controlled. The adapter assigns the next unique
        // position in the same atomic operation that enforces the list capacity.
        let item = ShoppingItem::new(item_id, name, quantity, unit, note, 0, actor.clone());
        let next_list_version = self
            .repository
            .add_item(
                &actor,
                &list_id,
                AggregateVersion::try_new(command.expected_list_version)
                    .map_err(|_| ShoppingItemError::InvalidExpectedVersion)?,
                item.clone(),
            )
            .map_err(map_repository_error)?;
        Ok((item, next_list_version.as_u64()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetShoppingItemCheckedCommand {
    /// Derived from verified authentication context, never accepted from the client body.
    pub actor_id: String,
    pub list_id: String,
    pub item_id: String,
    pub expected_item_version: u64,
    pub checked: bool,
}

pub struct SetShoppingItemChecked<'a> {
    repository: &'a dyn ShoppingItemRepository,
}

impl<'a> SetShoppingItemChecked<'a> {
    pub fn new(repository: &'a dyn ShoppingItemRepository) -> Self {
        Self { repository }
    }

    /// Returns the persisted item and the new parent-list version.
    pub fn execute(
        &self,
        command: SetShoppingItemCheckedCommand,
    ) -> Result<(ShoppingItem, u64), ShoppingItemError> {
        let actor =
            UserId::try_new(command.actor_id).map_err(|_| ShoppingItemError::InvalidActor)?;
        let list_id =
            ListId::try_new(command.list_id).map_err(|_| ShoppingItemError::InvalidListId)?;
        let item_id =
            ItemId::try_new(command.item_id).map_err(|_| ShoppingItemError::InvalidItemId)?;
        if command.expected_item_version == 0 {
            return Err(ShoppingItemError::InvalidExpectedVersion);
        }
        self.repository
            .set_item_checked(
                &actor,
                &list_id,
                &item_id,
                command.expected_item_version,
                command.checked,
            )
            .map(|(item, version)| (item, version.as_u64()))
            .map_err(map_repository_error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeleteShoppingItemCommand {
    /// Derived from verified authentication context, never accepted from the client body.
    pub actor_id: String,
    pub list_id: String,
    pub item_id: String,
    pub expected_item_version: u64,
}

pub struct DeleteShoppingItem<'a> {
    repository: &'a dyn ShoppingItemRepository,
}

impl<'a> DeleteShoppingItem<'a> {
    pub fn new(repository: &'a dyn ShoppingItemRepository) -> Self {
        Self { repository }
    }

    /// Returns the new parent-list version (not an item version).
    pub fn execute(&self, command: DeleteShoppingItemCommand) -> Result<u64, ShoppingItemError> {
        let actor =
            UserId::try_new(command.actor_id).map_err(|_| ShoppingItemError::InvalidActor)?;
        let list_id =
            ListId::try_new(command.list_id).map_err(|_| ShoppingItemError::InvalidListId)?;
        let item_id =
            ItemId::try_new(command.item_id).map_err(|_| ShoppingItemError::InvalidItemId)?;
        if command.expected_item_version == 0 {
            return Err(ShoppingItemError::InvalidExpectedVersion);
        }
        self.repository
            .delete_item(
                &actor,
                &list_id,
                &item_id,
                command.expected_item_version,
            )
            .map(|version| version.as_u64())
            .map_err(map_repository_error)
    }
}

fn optional_value(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    })
}

fn map_repository_error(error: RepositoryError) -> ShoppingItemError {
    match error {
        RepositoryError::NotFound => ShoppingItemError::NotFound,
        RepositoryError::Conflict | RepositoryError::AlreadyExists => ShoppingItemError::Conflict,
        RepositoryError::ItemLimitReached => ShoppingItemError::ItemLimitReached,
        RepositoryError::VersionExhausted => ShoppingItemError::VersionExhausted,
        RepositoryError::Unavailable => ShoppingItemError::PersistenceUnavailable,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use einkaufio_domain::{ItemId, ItemName, ShoppingItem, UserId};

    use super::*;
    use crate::ports::RepositoryError;

    type CheckedCall = (UserId, String, ItemId, u64, bool);

    #[derive(Default)]
    struct FakeRepository {
        added: RefCell<Vec<(UserId, String, u64, ShoppingItem)>>,
        checked: RefCell<Vec<CheckedCall>>,
        deleted: RefCell<Vec<(UserId, String, ItemId, u64)>>,
        failure: Option<RepositoryError>,
        list_version_after_mutation: u64,
    }

    impl ShoppingItemRepository for FakeRepository {
        fn add_item(
            &self,
            actor_id: &UserId,
            list_id: &ListId,
            expected_version: AggregateVersion,
            item: ShoppingItem,
        ) -> Result<AggregateVersion, RepositoryError> {
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            self.added.borrow_mut().push((
                actor_id.clone(),
                list_id.as_str().to_owned(),
                expected_version.as_u64(),
                item,
            ));
            Ok(AggregateVersion::try_new(self.list_version_after_mutation.max(expected_version.as_u64() + 1)).unwrap())
        }

        fn set_item_checked(
            &self,
            actor_id: &UserId,
            list_id: &ListId,
            item_id: &ItemId,
            expected_version: u64,
            checked: bool,
        ) -> Result<(ShoppingItem, AggregateVersion), RepositoryError> {
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            self.checked.borrow_mut().push((
                actor_id.clone(),
                list_id.as_str().to_owned(),
                item_id.clone(),
                expected_version,
                checked,
            ));
            let mut item = ShoppingItem::new(
                item_id.clone(),
                ItemName::try_new("Milch").unwrap(),
                None,
                None,
                None,
                0,
                actor_id.clone(),
            );
            item.set_checked(checked);
            // Set an independently controlled value in the fake so the test can prove the
            // returned list version is not accidentally read from the item version.
            Ok((
                item,
                AggregateVersion::try_new(self.list_version_after_mutation.max(expected_version + 10)).unwrap(),
            ))
        }

        fn delete_item(
            &self,
            actor_id: &UserId,
            list_id: &ListId,
            item_id: &ItemId,
            expected_version: u64,
        ) -> Result<AggregateVersion, RepositoryError> {
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            self.deleted.borrow_mut().push((
                actor_id.clone(),
                list_id.as_str().to_owned(),
                item_id.clone(),
                expected_version,
            ));
            Ok(AggregateVersion::try_new(self.list_version_after_mutation.max(expected_version + 10)).unwrap())
        }
    }

    struct FixedId;

    impl ItemIdGenerator for FixedId {
        fn generate(&self) -> String {
            "item-123".into()
        }
    }

    fn add_command() -> AddShoppingItemCommand {
        AddShoppingItemCommand {
            actor_id: "user-1".into(),
            list_id: "list-1".into(),
            expected_list_version: 4,
            name: " Milch ".into(),
            quantity: Some("2".into()),
            unit: Some("Liter".into()),
            note: None,
        }
    }

    fn check_command() -> SetShoppingItemCheckedCommand {
        SetShoppingItemCheckedCommand {
            actor_id: "user-1".into(),
            list_id: "list-1".into(),
            item_id: "item-1".into(),
            expected_item_version: 1,
            checked: true,
        }
    }

    fn delete_command() -> DeleteShoppingItemCommand {
        DeleteShoppingItemCommand {
            actor_id: "user-1".into(),
            list_id: "list-1".into(),
            item_id: "item-1".into(),
            expected_item_version: 1,
        }
    }

    #[test]
    fn adds_item_and_returns_new_list_version() {
        let repository = FakeRepository {
            list_version_after_mutation: 5,
            ..Default::default()
        };
        let (item, list_version) = AddShoppingItem::new(&repository, &FixedId)
            .execute(add_command())
            .unwrap();

        assert_eq!(item.id().as_str(), "item-123");
        assert_eq!(item.name().as_str(), "Milch");
        assert_eq!(item.quantity().unwrap().as_str(), "2");
        assert_eq!(list_version, 5);
        let added = repository.added.borrow();
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].0.as_str(), "user-1");
        assert_eq!(added[0].3.position(), 0);
    }

    #[test]
    fn normalizes_empty_optional_form_fields_to_none() {
        let repository = FakeRepository::default();
        let command = AddShoppingItemCommand {
            quantity: Some("  ".into()),
            unit: Some("".into()),
            note: Some(" \t".into()),
            ..add_command()
        };
        let (item, _) = AddShoppingItem::new(&repository, &FixedId)
            .execute(command)
            .unwrap();
        assert!(item.quantity().is_none());
        assert!(item.unit().is_none());
        assert!(item.note().is_none());
    }

    #[test]
    fn validates_before_calling_repository() {
        let repository = FakeRepository::default();
        let invalid = AddShoppingItemCommand {
            name: " ".into(),
            ..add_command()
        };
        assert!(matches!(
            AddShoppingItem::new(&repository, &FixedId).execute(invalid),
            Err(ShoppingItemError::InvalidName(_))
        ));
        assert!(repository.added.borrow().is_empty());
    }

    #[test]
    fn rejects_invalid_actor_and_identifiers() {
        let repository = FakeRepository::default();
        assert_eq!(
            AddShoppingItem::new(&repository, &FixedId).execute(AddShoppingItemCommand {
                actor_id: " ".into(),
                ..add_command()
            }),
            Err(ShoppingItemError::InvalidActor)
        );
        assert_eq!(
            AddShoppingItem::new(&repository, &FixedId).execute(AddShoppingItemCommand {
                list_id: " ".into(),
                ..add_command()
            }),
            Err(ShoppingItemError::InvalidListId)
        );
        assert_eq!(
            SetShoppingItemChecked::new(&repository).execute(SetShoppingItemCheckedCommand {
                item_id: " ".into(),
                ..check_command()
            }),
            Err(ShoppingItemError::InvalidItemId)
        );
        assert!(repository.added.borrow().is_empty());
        assert!(repository.checked.borrow().is_empty());
    }

    #[test]
    fn rejects_invalid_actor_for_check_and_delete() {
        let repository = FakeRepository::default();
        assert_eq!(
            SetShoppingItemChecked::new(&repository).execute(SetShoppingItemCheckedCommand {
                actor_id: " ".into(),
                ..check_command()
            }),
            Err(ShoppingItemError::InvalidActor)
        );
        assert_eq!(
            DeleteShoppingItem::new(&repository).execute(DeleteShoppingItemCommand {
                actor_id: " ".into(),
                ..delete_command()
            }),
            Err(ShoppingItemError::InvalidActor)
        );
    }

    #[test]
    fn rejects_zero_expected_versions_for_check_and_delete() {
        let repository = FakeRepository::default();
        assert_eq!(
            SetShoppingItemChecked::new(&repository).execute(SetShoppingItemCheckedCommand {
                expected_item_version: 0,
                ..check_command()
            }),
            Err(ShoppingItemError::InvalidExpectedVersion)
        );
        assert_eq!(
            DeleteShoppingItem::new(&repository).execute(DeleteShoppingItemCommand {
                expected_item_version: 0,
                ..delete_command()
            }),
            Err(ShoppingItemError::InvalidExpectedVersion)
        );
        assert!(repository.checked.borrow().is_empty());
        assert!(repository.deleted.borrow().is_empty());
    }

    #[test]
    fn validates_each_optional_value_object() {
        let repository = FakeRepository::default();
        let quantity_too_long = AddShoppingItemCommand {
            quantity: Some("x".repeat(41)),
            ..add_command()
        };
        assert!(matches!(
            AddShoppingItem::new(&repository, &FixedId).execute(quantity_too_long),
            Err(ShoppingItemError::InvalidQuantity(_))
        ));
        let unit_with_control = AddShoppingItemCommand {
            unit: Some("box\nunit".into()),
            ..add_command()
        };
        assert!(matches!(
            AddShoppingItem::new(&repository, &FixedId).execute(unit_with_control),
            Err(ShoppingItemError::InvalidUnit(_))
        ));
        let note_too_long = AddShoppingItemCommand {
            note: Some("n".repeat(501)),
            ..add_command()
        };
        assert!(matches!(
            AddShoppingItem::new(&repository, &FixedId).execute(note_too_long),
            Err(ShoppingItemError::InvalidNote(_))
        ));
        assert!(repository.added.borrow().is_empty());
    }

    #[test]
    fn maps_item_limit_and_conflict_errors() {
        for (repository_error, expected) in [
            (
                RepositoryError::ItemLimitReached,
                ShoppingItemError::ItemLimitReached,
            ),
            (RepositoryError::AlreadyExists, ShoppingItemError::Conflict),
            (RepositoryError::Conflict, ShoppingItemError::Conflict),
            (
                RepositoryError::VersionExhausted,
                ShoppingItemError::VersionExhausted,
            ),
        ] {
            let repository = FakeRepository {
                failure: Some(repository_error),
                ..Default::default()
            };
            assert_eq!(
                AddShoppingItem::new(&repository, &FixedId).execute(add_command()),
                Err(expected)
            );
        }
    }

    #[test]
    fn use_case_returns_distinct_item_and_list_versions() {
        let repository = FakeRepository {
            list_version_after_mutation: 42,
            ..Default::default()
        };
        let (item, list_version) = SetShoppingItemChecked::new(&repository)
            .execute(check_command())
            .unwrap();

        assert_eq!(item.version(), 2);
        assert_eq!(list_version, 42);
        let checked = repository.checked.borrow();
        assert_eq!(checked[0].0.as_str(), "user-1");
        assert_eq!(checked[0].3, 1);
    }

    #[test]
    fn check_and_delete_forward_actor_to_repository() {
        let repository = FakeRepository {
            list_version_after_mutation: 30,
            ..Default::default()
        };
        SetShoppingItemChecked::new(&repository)
            .execute(check_command())
            .unwrap();
        DeleteShoppingItem::new(&repository)
            .execute(delete_command())
            .unwrap();

        assert_eq!(repository.checked.borrow()[0].0.as_str(), "user-1");
        assert_eq!(repository.deleted.borrow()[0].0.as_str(), "user-1");
        assert_eq!(repository.deleted.borrow()[0].3, 1);
    }

    #[test]
    fn maps_not_found_and_unavailable_errors() {
        let missing = FakeRepository {
            failure: Some(RepositoryError::NotFound),
            ..Default::default()
        };
        assert_eq!(
            DeleteShoppingItem::new(&missing).execute(delete_command()),
            Err(ShoppingItemError::NotFound)
        );

        let unavailable = FakeRepository {
            failure: Some(RepositoryError::Unavailable),
            ..Default::default()
        };
        assert_eq!(
            DeleteShoppingItem::new(&unavailable).execute(delete_command()),
            Err(ShoppingItemError::PersistenceUnavailable)
        );
    }
}

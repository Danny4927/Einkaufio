//! Application use cases for shopping-list item mutations.

use einkaufio_domain::{
    AggregateVersion, ItemDetails, ItemId, ItemName, ItemNote, ItemOrderError, ItemQuantity,
    ItemUnit, ItemValueError, ListId, ListOperation, ShoppingItem, UserId,
};

use crate::ports::{ItemIdGenerator, ListAccessRequest, RepositoryError, ShoppingItemRepository};

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
    /// The requested order contains duplicates or too many items.
    InvalidOrder(ItemOrderError),
    InvalidCursor,
    ItemLimitReached,
    VersionExhausted,
    NotFound,
    /// The actor can see the list but lacks the permission the operation requires.
    Forbidden,
    Conflict,
    PersistenceUnavailable,
}

pub struct AddShoppingItem<'a, R> {
    repository: &'a R,
    id_generator: &'a dyn ItemIdGenerator,
}

impl<'a, R: ShoppingItemRepository> AddShoppingItem<'a, R> {
    pub fn new(repository: &'a R, id_generator: &'a dyn ItemIdGenerator) -> Self {
        Self {
            repository,
            id_generator,
        }
    }

    pub async fn execute(
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
        let ItemDetails {
            name,
            quantity,
            unit,
            note,
        } = item_details(command.name, command.quantity, command.unit, command.note)?;
        let item_id = ItemId::try_new(self.id_generator.generate())
            .map_err(|_| ShoppingItemError::InvalidItemId)?;
        // Position is deliberately not client-controlled. The adapter assigns the next unique
        // position in the same atomic operation that enforces the list capacity.
        let item = ShoppingItem::new(item_id, name, quantity, unit, note, 0, actor.clone());
        let access = ListAccessRequest::for_operation(actor, ListOperation::EditItems);
        let next_list_version = self
            .repository
            .add_item(
                &access,
                &list_id,
                AggregateVersion::try_new(command.expected_list_version)
                    .map_err(|_| ShoppingItemError::InvalidExpectedVersion)?,
                item.clone(),
            )
            .await
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

pub struct SetShoppingItemChecked<'a, R> {
    repository: &'a R,
}

impl<'a, R: ShoppingItemRepository> SetShoppingItemChecked<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    /// Returns the persisted item and the new parent-list version.
    pub async fn execute(
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
        let access = ListAccessRequest::for_operation(actor, ListOperation::EditItems);
        self.repository
            .set_item_checked(
                &access,
                &list_id,
                &item_id,
                command.expected_item_version,
                command.checked,
            )
            .await
            .map(|(item, version)| (item, version.as_u64()))
            .map_err(map_repository_error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateShoppingItemCommand {
    /// Derived from verified authentication context, never accepted from the client body.
    pub actor_id: String,
    pub list_id: String,
    pub item_id: String,
    pub expected_item_version: u64,
    /// Full replacement of the editable details; empty optional fields clear the value.
    pub name: String,
    pub quantity: Option<String>,
    pub unit: Option<String>,
    pub note: Option<String>,
}

pub struct UpdateShoppingItem<'a, R> {
    repository: &'a R,
}

impl<'a, R: ShoppingItemRepository> UpdateShoppingItem<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    /// Returns the persisted item and the parent-list version after the update.
    pub async fn execute(
        &self,
        command: UpdateShoppingItemCommand,
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
        let details = item_details(command.name, command.quantity, command.unit, command.note)?;
        let access = ListAccessRequest::for_operation(actor, ListOperation::EditItems);
        self.repository
            .update_item_details(
                &access,
                &list_id,
                &item_id,
                command.expected_item_version,
                details,
            )
            .await
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

pub struct DeleteShoppingItem<'a, R> {
    repository: &'a R,
}

impl<'a, R: ShoppingItemRepository> DeleteShoppingItem<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    /// Returns the new parent-list version (not an item version).
    pub async fn execute(
        &self,
        command: DeleteShoppingItemCommand,
    ) -> Result<u64, ShoppingItemError> {
        let actor =
            UserId::try_new(command.actor_id).map_err(|_| ShoppingItemError::InvalidActor)?;
        let list_id =
            ListId::try_new(command.list_id).map_err(|_| ShoppingItemError::InvalidListId)?;
        let item_id =
            ItemId::try_new(command.item_id).map_err(|_| ShoppingItemError::InvalidItemId)?;
        if command.expected_item_version == 0 {
            return Err(ShoppingItemError::InvalidExpectedVersion);
        }
        let access = ListAccessRequest::for_operation(actor, ListOperation::EditItems);
        self.repository
            .delete_item(&access, &list_id, &item_id, command.expected_item_version)
            .await
            .map(|version| version.as_u64())
            .map_err(map_repository_error)
    }
}

fn item_details(
    name: String,
    quantity: Option<String>,
    unit: Option<String>,
    note: Option<String>,
) -> Result<ItemDetails, ShoppingItemError> {
    let name = ItemName::try_new(name).map_err(ShoppingItemError::InvalidName)?;
    // Empty optional form fields are normalized to None before constructing value objects.
    let quantity = optional_value(quantity)
        .map(ItemQuantity::try_new)
        .transpose()
        .map_err(ShoppingItemError::InvalidQuantity)?;
    let unit = optional_value(unit)
        .map(ItemUnit::try_new)
        .transpose()
        .map_err(ShoppingItemError::InvalidUnit)?;
    let note = optional_value(note)
        .map(ItemNote::try_new)
        .transpose()
        .map_err(ShoppingItemError::InvalidNote)?;
    Ok(ItemDetails {
        name,
        quantity,
        unit,
        note,
    })
}

fn optional_value(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    })
}

pub(crate) fn map_repository_error(error: RepositoryError) -> ShoppingItemError {
    match error {
        RepositoryError::NotFound => ShoppingItemError::NotFound,
        RepositoryError::Forbidden => ShoppingItemError::Forbidden,
        RepositoryError::Conflict | RepositoryError::AlreadyExists => ShoppingItemError::Conflict,
        RepositoryError::ItemLimitReached => ShoppingItemError::ItemLimitReached,
        RepositoryError::VersionExhausted => ShoppingItemError::VersionExhausted,
        RepositoryError::Unavailable => ShoppingItemError::PersistenceUnavailable,
        RepositoryError::InvalidCursor => ShoppingItemError::InvalidCursor,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use einkaufio_domain::{ItemId, ItemName, ListPermission, ShoppingItem, UserId};

    use super::*;
    use crate::ports::RepositoryError;
    use crate::test_support::BlockOn;

    type CheckedCall = (UserId, String, ItemId, u64, bool);

    #[derive(Default)]
    struct FakeRepository {
        added: Mutex<Vec<(UserId, String, u64, ShoppingItem)>>,
        checked: Mutex<Vec<CheckedCall>>,
        deleted: Mutex<Vec<(UserId, String, ItemId, u64)>>,
        updated: Mutex<Vec<(UserId, ItemId, u64, ItemDetails)>>,
        failure: Option<RepositoryError>,
        list_version_after_mutation: u64,
        requests: Mutex<Vec<ListAccessRequest>>,
    }

    impl ShoppingItemRepository for FakeRepository {
        async fn add_item(
            &self,
            access: &ListAccessRequest,
            list_id: &ListId,
            expected_version: AggregateVersion,
            item: ShoppingItem,
        ) -> Result<AggregateVersion, RepositoryError> {
            self.requests.lock().unwrap().push(access.clone());
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            self.added.lock().unwrap().push((
                access.principal().clone(),
                list_id.as_str().to_owned(),
                expected_version.as_u64(),
                item,
            ));
            Ok(AggregateVersion::try_new(
                self.list_version_after_mutation
                    .max(expected_version.as_u64() + 1),
            )
            .unwrap())
        }

        async fn set_item_checked(
            &self,
            access: &ListAccessRequest,
            list_id: &ListId,
            item_id: &ItemId,
            expected_version: u64,
            checked: bool,
        ) -> Result<(ShoppingItem, AggregateVersion), RepositoryError> {
            self.requests.lock().unwrap().push(access.clone());
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            self.checked.lock().unwrap().push((
                access.principal().clone(),
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
                access.principal().clone(),
            );
            item.set_checked(checked).unwrap();
            // Set an independently controlled value in the fake so the test can prove the
            // returned list version is not accidentally read from the item version.
            Ok((
                item,
                AggregateVersion::try_new(
                    self.list_version_after_mutation.max(expected_version + 10),
                )
                .unwrap(),
            ))
        }

        async fn update_item_details(
            &self,
            access: &ListAccessRequest,
            _list_id: &ListId,
            item_id: &ItemId,
            expected_version: u64,
            details: ItemDetails,
        ) -> Result<(ShoppingItem, AggregateVersion), RepositoryError> {
            self.requests.lock().unwrap().push(access.clone());
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            self.updated.lock().unwrap().push((
                access.principal().clone(),
                item_id.clone(),
                expected_version,
                details.clone(),
            ));
            let mut item = ShoppingItem::new(
                item_id.clone(),
                ItemName::try_new("Alt").unwrap(),
                None,
                None,
                None,
                0,
                access.principal().clone(),
            );
            item.update_details(details).unwrap();
            Ok((
                item,
                AggregateVersion::try_new(expected_version + 10).unwrap(),
            ))
        }

        async fn delete_item(
            &self,
            access: &ListAccessRequest,
            list_id: &ListId,
            item_id: &ItemId,
            expected_version: u64,
        ) -> Result<AggregateVersion, RepositoryError> {
            self.requests.lock().unwrap().push(access.clone());
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            self.deleted.lock().unwrap().push((
                access.principal().clone(),
                list_id.as_str().to_owned(),
                item_id.clone(),
                expected_version,
            ));
            Ok(AggregateVersion::try_new(
                self.list_version_after_mutation.max(expected_version + 10),
            )
            .unwrap())
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
            .block_on()
            .unwrap();

        assert_eq!(item.id().as_str(), "item-123");
        assert_eq!(item.name().as_str(), "Milch");
        assert_eq!(item.quantity().unwrap().as_str(), "2");
        assert_eq!(list_version, 5);
        let added = repository.added.lock().unwrap();
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
            .block_on()
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
            AddShoppingItem::new(&repository, &FixedId)
                .execute(invalid)
                .block_on(),
            Err(ShoppingItemError::InvalidName(_))
        ));
        assert!(repository.added.lock().unwrap().is_empty());
    }

    #[test]
    fn rejects_invalid_actor_and_identifiers() {
        let repository = FakeRepository::default();
        assert_eq!(
            AddShoppingItem::new(&repository, &FixedId)
                .execute(AddShoppingItemCommand {
                    actor_id: " ".into(),
                    ..add_command()
                })
                .block_on(),
            Err(ShoppingItemError::InvalidActor)
        );
        assert_eq!(
            AddShoppingItem::new(&repository, &FixedId)
                .execute(AddShoppingItemCommand {
                    list_id: " ".into(),
                    ..add_command()
                })
                .block_on(),
            Err(ShoppingItemError::InvalidListId)
        );
        assert_eq!(
            SetShoppingItemChecked::new(&repository)
                .execute(SetShoppingItemCheckedCommand {
                    item_id: " ".into(),
                    ..check_command()
                })
                .block_on(),
            Err(ShoppingItemError::InvalidItemId)
        );
        assert!(repository.added.lock().unwrap().is_empty());
        assert!(repository.checked.lock().unwrap().is_empty());
    }

    #[test]
    fn rejects_invalid_actor_for_check_and_delete() {
        let repository = FakeRepository::default();
        assert_eq!(
            SetShoppingItemChecked::new(&repository)
                .execute(SetShoppingItemCheckedCommand {
                    actor_id: " ".into(),
                    ..check_command()
                })
                .block_on(),
            Err(ShoppingItemError::InvalidActor)
        );
        assert_eq!(
            DeleteShoppingItem::new(&repository)
                .execute(DeleteShoppingItemCommand {
                    actor_id: " ".into(),
                    ..delete_command()
                })
                .block_on(),
            Err(ShoppingItemError::InvalidActor)
        );
    }

    #[test]
    fn rejects_zero_expected_versions_for_check_and_delete() {
        let repository = FakeRepository::default();
        assert_eq!(
            SetShoppingItemChecked::new(&repository)
                .execute(SetShoppingItemCheckedCommand {
                    expected_item_version: 0,
                    ..check_command()
                })
                .block_on(),
            Err(ShoppingItemError::InvalidExpectedVersion)
        );
        assert_eq!(
            DeleteShoppingItem::new(&repository)
                .execute(DeleteShoppingItemCommand {
                    expected_item_version: 0,
                    ..delete_command()
                })
                .block_on(),
            Err(ShoppingItemError::InvalidExpectedVersion)
        );
        assert!(repository.checked.lock().unwrap().is_empty());
        assert!(repository.deleted.lock().unwrap().is_empty());
    }

    #[test]
    fn validates_each_optional_value_object() {
        let repository = FakeRepository::default();
        let quantity_too_long = AddShoppingItemCommand {
            quantity: Some("x".repeat(41)),
            ..add_command()
        };
        assert!(matches!(
            AddShoppingItem::new(&repository, &FixedId)
                .execute(quantity_too_long)
                .block_on(),
            Err(ShoppingItemError::InvalidQuantity(_))
        ));
        let unit_with_control = AddShoppingItemCommand {
            unit: Some("box\nunit".into()),
            ..add_command()
        };
        assert!(matches!(
            AddShoppingItem::new(&repository, &FixedId)
                .execute(unit_with_control)
                .block_on(),
            Err(ShoppingItemError::InvalidUnit(_))
        ));
        let note_too_long = AddShoppingItemCommand {
            note: Some("n".repeat(501)),
            ..add_command()
        };
        assert!(matches!(
            AddShoppingItem::new(&repository, &FixedId)
                .execute(note_too_long)
                .block_on(),
            Err(ShoppingItemError::InvalidNote(_))
        ));
        assert!(repository.added.lock().unwrap().is_empty());
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
                AddShoppingItem::new(&repository, &FixedId)
                    .execute(add_command())
                    .block_on(),
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
            .block_on()
            .unwrap();

        assert_eq!(item.version(), 2);
        assert_eq!(list_version, 42);
        let checked = repository.checked.lock().unwrap();
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
            .block_on()
            .unwrap();
        DeleteShoppingItem::new(&repository)
            .execute(delete_command())
            .block_on()
            .unwrap();

        assert_eq!(repository.checked.lock().unwrap()[0].0.as_str(), "user-1");
        assert_eq!(repository.deleted.lock().unwrap()[0].0.as_str(), "user-1");
        assert_eq!(repository.deleted.lock().unwrap()[0].3, 1);
    }

    #[test]
    fn maps_not_found_and_unavailable_errors() {
        let missing = FakeRepository {
            failure: Some(RepositoryError::NotFound),
            ..Default::default()
        };
        assert_eq!(
            DeleteShoppingItem::new(&missing)
                .execute(delete_command())
                .block_on(),
            Err(ShoppingItemError::NotFound)
        );

        let unavailable = FakeRepository {
            failure: Some(RepositoryError::Unavailable),
            ..Default::default()
        };
        assert_eq!(
            DeleteShoppingItem::new(&unavailable)
                .execute(delete_command())
                .block_on(),
            Err(ShoppingItemError::PersistenceUnavailable)
        );
    }

    fn update_command() -> UpdateShoppingItemCommand {
        UpdateShoppingItemCommand {
            actor_id: "user-1".into(),
            list_id: "list-1".into(),
            item_id: "item-1".into(),
            expected_item_version: 3,
            name: " Hafermilch ".into(),
            quantity: Some(" ".into()),
            unit: Some("Packung".into()),
            note: None,
        }
    }

    #[test]
    fn update_forwards_normalized_details_and_returns_versions() {
        let repository = FakeRepository::default();
        let (item, list_version) = UpdateShoppingItem::new(&repository)
            .execute(update_command())
            .block_on()
            .unwrap();
        assert_eq!(item.name().as_str(), "Hafermilch");
        assert_eq!(item.quantity(), None);
        assert_eq!(item.unit().unwrap().as_str(), "Packung");
        assert_eq!(list_version, 13);
        let calls = repository.updated.lock().unwrap();
        assert_eq!(calls.len(), 1);
        let (actor, item_id, expected, details) = &calls[0];
        assert_eq!(actor.as_str(), "user-1");
        assert_eq!(item_id.as_str(), "item-1");
        assert_eq!(*expected, 3);
        assert_eq!(details.name.as_str(), "Hafermilch");
        assert_eq!(details.quantity, None);
        assert_eq!(details.note, None);
    }

    #[test]
    fn update_validates_before_calling_repository() {
        let repository = FakeRepository::default();
        let use_case = UpdateShoppingItem::new(&repository);
        let cases = [
            (
                UpdateShoppingItemCommand {
                    actor_id: " ".into(),
                    ..update_command()
                },
                ShoppingItemError::InvalidActor,
            ),
            (
                UpdateShoppingItemCommand {
                    list_id: " ".into(),
                    ..update_command()
                },
                ShoppingItemError::InvalidListId,
            ),
            (
                UpdateShoppingItemCommand {
                    item_id: " ".into(),
                    ..update_command()
                },
                ShoppingItemError::InvalidItemId,
            ),
            (
                UpdateShoppingItemCommand {
                    expected_item_version: 0,
                    ..update_command()
                },
                ShoppingItemError::InvalidExpectedVersion,
            ),
            (
                UpdateShoppingItemCommand {
                    name: "  ".into(),
                    ..update_command()
                },
                ShoppingItemError::InvalidName(ItemValueError::Empty),
            ),
            (
                UpdateShoppingItemCommand {
                    quantity: Some("x".repeat(41)),
                    ..update_command()
                },
                ShoppingItemError::InvalidQuantity(ItemValueError::TooLong),
            ),
            (
                UpdateShoppingItemCommand {
                    unit: Some("a\tb".into()),
                    ..update_command()
                },
                ShoppingItemError::InvalidUnit(ItemValueError::ContainsControlCharacters),
            ),
            (
                UpdateShoppingItemCommand {
                    note: Some("n".repeat(501)),
                    ..update_command()
                },
                ShoppingItemError::InvalidNote(ItemValueError::TooLong),
            ),
        ];
        for (command, expected) in cases {
            assert_eq!(use_case.execute(command).block_on(), Err(expected));
        }
        assert!(repository.updated.lock().unwrap().is_empty());
    }

    #[test]
    fn update_maps_repository_errors() {
        for (failure, expected) in [
            (RepositoryError::NotFound, ShoppingItemError::NotFound),
            (RepositoryError::Conflict, ShoppingItemError::Conflict),
            (
                RepositoryError::VersionExhausted,
                ShoppingItemError::VersionExhausted,
            ),
            (
                RepositoryError::Unavailable,
                ShoppingItemError::PersistenceUnavailable,
            ),
        ] {
            let repository = FakeRepository {
                failure: Some(failure),
                ..FakeRepository::default()
            };
            assert_eq!(
                UpdateShoppingItem::new(&repository)
                    .execute(update_command())
                    .block_on(),
                Err(expected)
            );
        }
    }

    #[test]
    fn every_item_use_case_requests_contribute_permission_for_the_verified_actor() {
        let repository = FakeRepository::default();
        AddShoppingItem::new(&repository, &FixedId)
            .execute(add_command())
            .block_on()
            .unwrap();
        SetShoppingItemChecked::new(&repository)
            .execute(check_command())
            .block_on()
            .unwrap();
        UpdateShoppingItem::new(&repository)
            .execute(update_command())
            .block_on()
            .unwrap();
        DeleteShoppingItem::new(&repository)
            .execute(delete_command())
            .block_on()
            .unwrap();

        let requests = repository.requests.lock().unwrap();
        assert_eq!(requests.len(), 4);
        for request in requests.iter() {
            assert_eq!(request.principal().as_str(), "user-1");
            assert_eq!(request.permission(), ListPermission::Contribute);
        }
    }

    #[test]
    fn maps_forbidden_without_converting_it_to_not_found() {
        let repository = FakeRepository {
            failure: Some(RepositoryError::Forbidden),
            ..FakeRepository::default()
        };
        assert_eq!(
            DeleteShoppingItem::new(&repository)
                .execute(delete_command())
                .block_on(),
            Err(ShoppingItemError::Forbidden)
        );
        assert_eq!(
            AddShoppingItem::new(&repository, &FixedId)
                .execute(add_command())
                .block_on(),
            Err(ShoppingItemError::Forbidden)
        );
    }
}

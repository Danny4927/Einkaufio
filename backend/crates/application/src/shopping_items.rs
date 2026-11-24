//! Application use cases for shopping-list item mutations.

use einkaufio_domain::{
    ItemId, ItemName, ItemNote, ItemQuantity, ItemUnit, ItemValueError, ListId, ShoppingItem,
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
    pub position: u32,
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
        let quantity = command
            .quantity
            .map(ItemQuantity::try_new)
            .transpose()
            .map_err(ShoppingItemError::InvalidQuantity)?;
        let unit = command
            .unit
            .map(ItemUnit::try_new)
            .transpose()
            .map_err(ShoppingItemError::InvalidUnit)?;
        let note = command
            .note
            .map(ItemNote::try_new)
            .transpose()
            .map_err(ShoppingItemError::InvalidNote)?;
        let item_id = ItemId::try_new(self.id_generator.generate())
            .map_err(|_| ShoppingItemError::InvalidItemId)?;
        let item = ShoppingItem::new(item_id, name, quantity, unit, note, command.position, actor);
        let next_version = self
            .repository
            .add_item(
                list_id.as_str(),
                command.expected_list_version,
                item.clone(),
            )
            .map_err(map_repository_error)?;
        Ok((item, next_version))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetShoppingItemCheckedCommand {
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

    pub fn execute(
        &self,
        command: SetShoppingItemCheckedCommand,
    ) -> Result<(ShoppingItem, u64), ShoppingItemError> {
        let list_id =
            ListId::try_new(command.list_id).map_err(|_| ShoppingItemError::InvalidListId)?;
        let item_id =
            ItemId::try_new(command.item_id).map_err(|_| ShoppingItemError::InvalidItemId)?;
        if command.expected_item_version == 0 {
            return Err(ShoppingItemError::InvalidExpectedVersion);
        }
        self.repository
            .set_item_checked(
                list_id.as_str(),
                &item_id,
                command.expected_item_version,
                command.checked,
            )
            .map_err(map_repository_error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeleteShoppingItemCommand {
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

    pub fn execute(&self, command: DeleteShoppingItemCommand) -> Result<u64, ShoppingItemError> {
        let list_id =
            ListId::try_new(command.list_id).map_err(|_| ShoppingItemError::InvalidListId)?;
        let item_id =
            ItemId::try_new(command.item_id).map_err(|_| ShoppingItemError::InvalidItemId)?;
        if command.expected_item_version == 0 {
            return Err(ShoppingItemError::InvalidExpectedVersion);
        }
        self.repository
            .delete_item(list_id.as_str(), &item_id, command.expected_item_version)
            .map_err(map_repository_error)
    }
}

fn map_repository_error(error: RepositoryError) -> ShoppingItemError {
    match error {
        RepositoryError::NotFound => ShoppingItemError::NotFound,
        RepositoryError::Conflict | RepositoryError::AlreadyExists => ShoppingItemError::Conflict,
        RepositoryError::Unavailable => ShoppingItemError::PersistenceUnavailable,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use einkaufio_domain::{ItemId, ItemName, ShoppingItem, UserId};

    use super::*;
    use crate::ports::RepositoryError;

    #[derive(Default)]
    struct FakeRepository {
        added: RefCell<Vec<ShoppingItem>>,
        checked: RefCell<Vec<(ItemId, u64, bool)>>,
        deleted: RefCell<Vec<ItemId>>,
        failure: Option<RepositoryError>,
    }

    impl ShoppingItemRepository for FakeRepository {
        fn add_item(
            &self,
            _list_id: &str,
            expected_version: u64,
            item: ShoppingItem,
        ) -> Result<u64, RepositoryError> {
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            self.added.borrow_mut().push(item);
            Ok(expected_version + 1)
        }

        fn set_item_checked(
            &self,
            _list_id: &str,
            item_id: &ItemId,
            expected_version: u64,
            checked: bool,
        ) -> Result<(ShoppingItem, u64), RepositoryError> {
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            self.checked
                .borrow_mut()
                .push((item_id.clone(), expected_version, checked));
            let mut item = ShoppingItem::new(
                item_id.clone(),
                ItemName::try_new("Milch").unwrap(),
                None,
                None,
                None,
                0,
                UserId::try_new("owner").unwrap(),
            );
            item.set_checked(checked);
            Ok((item, expected_version + 1))
        }

        fn delete_item(
            &self,
            _list_id: &str,
            item_id: &ItemId,
            expected_version: u64,
        ) -> Result<u64, RepositoryError> {
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            self.deleted.borrow_mut().push(item_id.clone());
            Ok(expected_version + 1)
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
            position: 0,
        }
    }

    #[test]
    fn adds_item_and_returns_new_list_version() {
        let repository = FakeRepository::default();
        let (item, version) = AddShoppingItem::new(&repository, &FixedId)
            .execute(add_command())
            .unwrap();

        assert_eq!(item.id().as_str(), "item-123");
        assert_eq!(item.name().as_str(), "Milch");
        assert_eq!(item.quantity().unwrap().as_str(), "2");
        assert_eq!(version, 5);
        assert_eq!(repository.added.borrow().len(), 1);
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
    fn rejects_zero_expected_list_version() {
        let repository = FakeRepository::default();
        let invalid = AddShoppingItemCommand {
            expected_list_version: 0,
            ..add_command()
        };
        assert_eq!(
            AddShoppingItem::new(&repository, &FixedId).execute(invalid),
            Err(ShoppingItemError::InvalidExpectedVersion)
        );
        assert!(repository.added.borrow().is_empty());
    }

    #[test]
    fn maps_concurrency_conflicts() {
        let repository = FakeRepository {
            failure: Some(RepositoryError::Conflict),
            ..Default::default()
        };
        assert_eq!(
            AddShoppingItem::new(&repository, &FixedId).execute(add_command()),
            Err(ShoppingItemError::Conflict)
        );
    }

    #[test]
    fn checks_item_with_expected_version() {
        let repository = FakeRepository::default();
        let (item, version) = SetShoppingItemChecked::new(&repository)
            .execute(SetShoppingItemCheckedCommand {
                list_id: "list-1".into(),
                item_id: "item-1".into(),
                expected_item_version: 1,
                checked: true,
            })
            .unwrap();

        assert!(item.checked());
        assert_eq!(item.version(), 2);
        assert_eq!(version, 2);
        assert_eq!(repository.checked.borrow()[0].1, 1);
    }

    #[test]
    fn deletes_item_with_expected_version() {
        let repository = FakeRepository::default();
        let version = DeleteShoppingItem::new(&repository)
            .execute(DeleteShoppingItemCommand {
                list_id: "list-1".into(),
                item_id: "item-2".into(),
                expected_item_version: 1,
            })
            .unwrap();

        assert_eq!(version, 2);
        assert_eq!(repository.deleted.borrow().len(), 1);
    }

    #[test]
    fn maps_not_found_and_unavailable_errors() {
        let missing = FakeRepository {
            failure: Some(RepositoryError::NotFound),
            ..Default::default()
        };
        assert_eq!(
            DeleteShoppingItem::new(&missing).execute(DeleteShoppingItemCommand {
                list_id: "list".into(),
                item_id: "item".into(),
                expected_item_version: 1,
            }),
            Err(ShoppingItemError::NotFound)
        );

        let unavailable = FakeRepository {
            failure: Some(RepositoryError::Unavailable),
            ..Default::default()
        };
        assert_eq!(
            DeleteShoppingItem::new(&unavailable).execute(DeleteShoppingItemCommand {
                list_id: "list".into(),
                item_id: "item".into(),
                expected_item_version: 1,
            }),
            Err(ShoppingItemError::PersistenceUnavailable)
        );
    }
}

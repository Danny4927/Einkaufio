//! Owner-scoped personal-list rename use case.

use einkaufio_domain::{IdentifierError, ListId, ListName, ListNameError, ShoppingList, UserId};

use crate::ports::{RepositoryError, ShoppingListRepository};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenamePersonalListCommand {
    /// Identity from verified authentication context, never from client input.
    pub owner_id: String,
    pub list_id: String,
    pub expected_version: u64,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RenamePersonalListError {
    InvalidOwner(IdentifierError),
    InvalidListId(IdentifierError),
    InvalidName(ListNameError),
    InvalidExpectedVersion,
    NotFound,
    Conflict,
    PersistenceUnavailable,
    RepositoryInvariantViolation,
}

pub struct RenamePersonalList<'a> {
    repository: &'a dyn ShoppingListRepository,
}

impl<'a> RenamePersonalList<'a> {
    pub fn new(repository: &'a dyn ShoppingListRepository) -> Self {
        Self { repository }
    }

    pub fn execute(
        &self,
        command: RenamePersonalListCommand,
    ) -> Result<ShoppingList, RenamePersonalListError> {
        let owner_id =
            UserId::try_new(command.owner_id).map_err(RenamePersonalListError::InvalidOwner)?;
        let list_id =
            ListId::try_new(command.list_id).map_err(RenamePersonalListError::InvalidListId)?;
        let name =
            ListName::try_new(command.name).map_err(RenamePersonalListError::InvalidName)?;
        if command.expected_version == 0 {
            return Err(RenamePersonalListError::InvalidExpectedVersion);
        }

        self.repository
            .rename_personal(&owner_id, &list_id, command.expected_version, name)
            .map_err(map_repository_error)
    }
}

fn map_repository_error(error: RepositoryError) -> RenamePersonalListError {
    match error {
        RepositoryError::NotFound => RenamePersonalListError::NotFound,
        RepositoryError::Conflict => RenamePersonalListError::Conflict,
        RepositoryError::Unavailable => RenamePersonalListError::PersistenceUnavailable,
        RepositoryError::AlreadyExists | RepositoryError::ItemLimitReached => {
            RenamePersonalListError::RepositoryInvariantViolation
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use einkaufio_domain::{ListId, ListName, ShoppingList, UserId};

    use super::{RenamePersonalList, RenamePersonalListCommand, RenamePersonalListError};
    use crate::ports::{RepositoryError, ShoppingListRepository};

    #[derive(Default)]
    struct FakeRepository {
        call: RefCell<Option<(UserId, ListId, u64, String)>>,
        result: Option<Result<ShoppingList, RepositoryError>>,
    }

    impl ShoppingListRepository for FakeRepository {
        fn create_personal(&self, _list: ShoppingList) -> Result<(), RepositoryError> {
            Ok(())
        }

        fn rename_personal(
            &self,
            owner_id: &UserId,
            list_id: &ListId,
            expected_version: u64,
            name: ListName,
        ) -> Result<ShoppingList, RepositoryError> {
            *self.call.borrow_mut() = Some((
                owner_id.clone(),
                list_id.clone(),
                expected_version,
                name.as_str().to_owned(),
            ));
            self.result
                .clone()
                .unwrap_or_else(|| Ok(ShoppingList::new_personal(
                    list_id.clone(),
                    owner_id.clone(),
                    name,
                )))
        }
    }

    fn command() -> RenamePersonalListCommand {
        RenamePersonalListCommand {
            owner_id: "user-1".to_owned(),
            list_id: "list-1".to_owned(),
            expected_version: 7,
            name: "  Drogerie  ".to_owned(),
        }
    }

    #[test]
    fn passes_authenticated_owner_and_expected_version_to_repository() {
        let repository = FakeRepository::default();
        let use_case = RenamePersonalList::new(&repository);

        let renamed = use_case.execute(command()).expect("rename should succeed");

        assert_eq!(renamed.name().as_str(), "Drogerie");
        let call = repository.call.borrow();
        let (owner, list, version, name) = call.as_ref().expect("repository called");
        assert_eq!(owner.as_str(), "user-1");
        assert_eq!(list.as_str(), "list-1");
        assert_eq!(*version, 7);
        assert_eq!(name, "Drogerie");
    }

    #[test]
    fn rejects_invalid_input_before_repository_access() {
        let repository = FakeRepository::default();
        let use_case = RenamePersonalList::new(&repository);
        let invalid = RenamePersonalListCommand {
            expected_version: 0,
            ..command()
        };

        assert_eq!(
            use_case.execute(invalid),
            Err(RenamePersonalListError::InvalidExpectedVersion)
        );
        assert!(repository.call.borrow().is_none());
    }

    #[test]
    fn maps_stale_version_to_conflict() {
        let repository = FakeRepository {
            result: Some(Err(RepositoryError::Conflict)),
            ..Default::default()
        };
        let use_case = RenamePersonalList::new(&repository);

        assert_eq!(
            use_case.execute(command()),
            Err(RenamePersonalListError::Conflict)
        );
    }

    #[test]
    fn maps_missing_or_not_owned_list_to_not_found() {
        let repository = FakeRepository {
            result: Some(Err(RepositoryError::NotFound)),
            ..Default::default()
        };
        let use_case = RenamePersonalList::new(&repository);

        assert_eq!(
            use_case.execute(command()),
            Err(RenamePersonalListError::NotFound)
        );
    }
}

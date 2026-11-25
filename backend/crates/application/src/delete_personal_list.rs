//! Owner-scoped personal-list deletion use case.

use einkaufio_domain::{IdentifierError, ListId, UserId};

use crate::ports::{RepositoryError, ShoppingListRepository};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeletePersonalListCommand {
    /// Identity from verified authentication context, never from client input.
    pub owner_id: String,
    pub list_id: String,
    pub expected_version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeletePersonalListError {
    InvalidOwner(IdentifierError),
    InvalidListId(IdentifierError),
    InvalidExpectedVersion,
    NotFound,
    Conflict,
    PersistenceUnavailable,
    RepositoryInvariantViolation,
}

pub struct DeletePersonalList<'a> {
    repository: &'a dyn ShoppingListRepository,
}

impl<'a> DeletePersonalList<'a> {
    pub fn new(repository: &'a dyn ShoppingListRepository) -> Self {
        Self { repository }
    }

    pub fn execute(
        &self,
        command: DeletePersonalListCommand,
    ) -> Result<(), DeletePersonalListError> {
        let owner_id =
            UserId::try_new(command.owner_id).map_err(DeletePersonalListError::InvalidOwner)?;
        let list_id =
            ListId::try_new(command.list_id).map_err(DeletePersonalListError::InvalidListId)?;
        if command.expected_version == 0 {
            return Err(DeletePersonalListError::InvalidExpectedVersion);
        }

        self.repository
            .delete_personal(&owner_id, &list_id, command.expected_version)
            .map_err(map_repository_error)
    }
}

fn map_repository_error(error: RepositoryError) -> DeletePersonalListError {
    match error {
        RepositoryError::NotFound => DeletePersonalListError::NotFound,
        RepositoryError::Conflict => DeletePersonalListError::Conflict,
        RepositoryError::Unavailable => DeletePersonalListError::PersistenceUnavailable,
        RepositoryError::AlreadyExists | RepositoryError::ItemLimitReached => {
            DeletePersonalListError::RepositoryInvariantViolation
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use einkaufio_domain::{ListId, ShoppingList, UserId};

    use super::{DeletePersonalList, DeletePersonalListCommand, DeletePersonalListError};
    use crate::ports::{RepositoryError, ShoppingListRepository};

    #[derive(Default)]
    struct FakeRepository {
        call: RefCell<Option<(UserId, ListId, u64)>>,
        result: Option<Result<(), RepositoryError>>,
    }

    impl ShoppingListRepository for FakeRepository {
        fn create_personal(&self, _list: ShoppingList) -> Result<(), RepositoryError> {
            Ok(())
        }

        fn delete_personal(
            &self,
            owner_id: &UserId,
            list_id: &ListId,
            expected_version: u64,
        ) -> Result<(), RepositoryError> {
            *self.call.borrow_mut() =
                Some((owner_id.clone(), list_id.clone(), expected_version));
            self.result.clone().unwrap_or(Ok(()))
        }
    }

    fn command() -> DeletePersonalListCommand {
        DeletePersonalListCommand {
            owner_id: "user-1".to_owned(),
            list_id: "list-1".to_owned(),
            expected_version: 3,
        }
    }

    #[test]
    fn passes_authenticated_owner_and_expected_version_to_repository() {
        let repository = FakeRepository::default();
        let use_case = DeletePersonalList::new(&repository);

        use_case.execute(command()).expect("delete should succeed");

        let call = repository.call.borrow();
        let (owner, list, version) = call.as_ref().expect("repository called");
        assert_eq!(owner.as_str(), "user-1");
        assert_eq!(list.as_str(), "list-1");
        assert_eq!(*version, 3);
    }

    #[test]
    fn rejects_zero_expected_version_before_repository_access() {
        let repository = FakeRepository::default();
        let use_case = DeletePersonalList::new(&repository);
        let invalid = DeletePersonalListCommand {
            expected_version: 0,
            ..command()
        };

        assert_eq!(
            use_case.execute(invalid),
            Err(DeletePersonalListError::InvalidExpectedVersion)
        );
        assert!(repository.call.borrow().is_none());
    }

    #[test]
    fn maps_stale_version_to_conflict() {
        let repository = FakeRepository {
            result: Some(Err(RepositoryError::Conflict)),
            ..Default::default()
        };
        let use_case = DeletePersonalList::new(&repository);

        assert_eq!(
            use_case.execute(command()),
            Err(DeletePersonalListError::Conflict)
        );
    }

    #[test]
    fn maps_missing_or_not_owned_list_to_not_found() {
        let repository = FakeRepository {
            result: Some(Err(RepositoryError::NotFound)),
            ..Default::default()
        };
        let use_case = DeletePersonalList::new(&repository);

        assert_eq!(
            use_case.execute(command()),
            Err(DeletePersonalListError::NotFound)
        );
    }
}

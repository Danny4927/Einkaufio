//! Owner-scoped personal-list deletion use case.

use einkaufio_domain::{AggregateVersion, IdentifierError, ListId, UserId};

use crate::ports::{RepositoryError, ShoppingListRepository};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeletePersonalListCommand {
    /// Identity from verified authentication context, never from client input.
    pub actor_id: String,
    pub list_id: String,
    pub expected_version: AggregateVersion,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeletePersonalListError {
    InvalidActor(IdentifierError),
    InvalidListId(IdentifierError),
    Repository(RepositoryError),
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
        let actor_id =
            UserId::try_new(command.actor_id).map_err(DeletePersonalListError::InvalidActor)?;
        let list_id =
            ListId::try_new(command.list_id).map_err(DeletePersonalListError::InvalidListId)?;

        self.repository
            .delete_personal(&actor_id, &list_id, command.expected_version)
            .map_err(DeletePersonalListError::Repository)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use einkaufio_domain::{AggregateVersion, ListId, ShoppingList, UserId};

    use super::{DeletePersonalList, DeletePersonalListCommand, DeletePersonalListError};
    use crate::ports::{RepositoryError, ShoppingListRepository};

    #[derive(Default)]
    struct FakeRepository {
        deleted: RefCell<bool>,
        failure: Option<RepositoryError>,
        call: RefCell<Option<(UserId, ListId, AggregateVersion)>>,
    }

    impl ShoppingListRepository for FakeRepository {
        fn create_personal(&self, _list: ShoppingList) -> Result<(), RepositoryError> {
            Ok(())
        }

        fn delete_personal(
            &self,
            actor_id: &UserId,
            list_id: &ListId,
            expected_version: AggregateVersion,
        ) -> Result<(), RepositoryError> {
            *self.call.borrow_mut() = Some((actor_id.clone(), list_id.clone(), expected_version));
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            let mut deleted = self.deleted.borrow_mut();
            if *deleted {
                return Err(RepositoryError::NotFound);
            }
            *deleted = true;
            Ok(())
        }
    }

    fn command() -> DeletePersonalListCommand {
        DeletePersonalListCommand {
            actor_id: "user-1".to_owned(),
            list_id: "list-1".to_owned(),
            expected_version: AggregateVersion::try_new(3).unwrap(),
        }
    }

    #[test]
    fn passes_actor_and_expected_version_to_repository() {
        let repository = FakeRepository::default();
        let use_case = DeletePersonalList::new(&repository);

        use_case.execute(command()).expect("delete should succeed");

        let call = repository.call.borrow();
        let (actor, list, version) = call.as_ref().expect("repository called");
        assert_eq!(actor.as_str(), "user-1");
        assert_eq!(list.as_str(), "list-1");
        assert_eq!(*version, AggregateVersion::try_new(3).unwrap());
    }

    #[test]
    fn rejects_invalid_actor_and_list_id_before_repository_access() {
        for invalid in [
            DeletePersonalListCommand {
                actor_id: " ".to_owned(),
                ..command()
            },
            DeletePersonalListCommand {
                list_id: " ".to_owned(),
                ..command()
            },
        ] {
            let repository = FakeRepository::default();
            let use_case = DeletePersonalList::new(&repository);

            assert!(use_case.execute(invalid).is_err());
            assert!(repository.call.borrow().is_none());
        }
    }

    #[test]
    fn repeated_delete_returns_not_found() {
        let repository = FakeRepository::default();
        let use_case = DeletePersonalList::new(&repository);

        use_case.execute(command()).expect("first delete succeeds");
        assert_eq!(
            use_case.execute(command()),
            Err(DeletePersonalListError::Repository(
                RepositoryError::NotFound
            ))
        );
    }

    #[test]
    fn propagates_repository_failures_including_version_exhaustion() {
        for error in [
            RepositoryError::NotFound,
            RepositoryError::Conflict,
            RepositoryError::Unavailable,
            RepositoryError::AlreadyExists,
            RepositoryError::ItemLimitReached,
            RepositoryError::VersionExhausted,
        ] {
            let repository = FakeRepository {
                failure: Some(error.clone()),
                ..Default::default()
            };
            let use_case = DeletePersonalList::new(&repository);

            assert_eq!(
                use_case.execute(command()),
                Err(DeletePersonalListError::Repository(error))
            );
        }
    }
}

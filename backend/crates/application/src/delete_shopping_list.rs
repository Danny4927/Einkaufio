//! Access-scoped shopping-list deletion use case (ADR 015).

use einkaufio_domain::{AggregateVersion, IdentifierError, ListId, ListOperation, UserId};

use crate::ports::{ListAccessRequest, RepositoryError, ShoppingListRepository};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeleteShoppingListCommand {
    /// Identity from verified authentication context, never from client input.
    pub actor_id: String,
    pub list_id: String,
    pub expected_version: AggregateVersion,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeleteShoppingListError {
    InvalidActor(IdentifierError),
    InvalidListId(IdentifierError),
    Repository(RepositoryError),
}

pub struct DeleteShoppingList<'a, R> {
    repository: &'a R,
}

impl<'a, R: ShoppingListRepository> DeleteShoppingList<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    pub async fn execute(
        &self,
        command: DeleteShoppingListCommand,
    ) -> Result<(), DeleteShoppingListError> {
        let actor_id =
            UserId::try_new(command.actor_id).map_err(DeleteShoppingListError::InvalidActor)?;
        let list_id =
            ListId::try_new(command.list_id).map_err(DeleteShoppingListError::InvalidListId)?;
        let access = ListAccessRequest::for_operation(actor_id, ListOperation::Delete);

        self.repository
            .delete_list(&access, &list_id, command.expected_version)
            .await
            .map_err(DeleteShoppingListError::Repository)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use einkaufio_domain::{AggregateVersion, ListId, ListPermission, ShoppingList};

    use super::{DeleteShoppingList, DeleteShoppingListCommand, DeleteShoppingListError};
    use crate::ports::{ListAccessRequest, RepositoryError, ShoppingListRepository};
    use crate::test_support::BlockOn;

    #[derive(Default)]
    struct FakeRepository {
        deleted: Mutex<bool>,
        failure: Option<RepositoryError>,
        call: Mutex<Option<(ListAccessRequest, ListId, AggregateVersion)>>,
    }

    impl ShoppingListRepository for FakeRepository {
        async fn create_personal(&self, _list: ShoppingList) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn delete_list(
            &self,
            access: &ListAccessRequest,
            list_id: &ListId,
            expected_version: AggregateVersion,
        ) -> Result<(), RepositoryError> {
            *self.call.lock().unwrap() = Some((access.clone(), list_id.clone(), expected_version));
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            let mut deleted = self.deleted.lock().unwrap();
            if *deleted {
                return Err(RepositoryError::NotFound);
            }
            *deleted = true;
            Ok(())
        }
    }

    fn command() -> DeleteShoppingListCommand {
        DeleteShoppingListCommand {
            actor_id: "user-1".to_owned(),
            list_id: "list-1".to_owned(),
            expected_version: AggregateVersion::try_new(3).unwrap(),
        }
    }

    #[test]
    fn passes_actor_administer_permission_and_expected_version_to_repository() {
        let repository = FakeRepository::default();
        let use_case = DeleteShoppingList::new(&repository);

        use_case
            .execute(command())
            .block_on()
            .expect("delete should succeed");

        let call = repository.call.lock().unwrap();
        let (access, list, version) = call.as_ref().expect("repository called");
        assert_eq!(access.principal().as_str(), "user-1");
        assert_eq!(access.permission(), ListPermission::Administer);
        assert_eq!(list.as_str(), "list-1");
        assert_eq!(*version, AggregateVersion::try_new(3).unwrap());
    }

    #[test]
    fn rejects_invalid_actor_and_list_id_before_repository_access() {
        for invalid in [
            DeleteShoppingListCommand {
                actor_id: " ".to_owned(),
                ..command()
            },
            DeleteShoppingListCommand {
                list_id: " ".to_owned(),
                ..command()
            },
        ] {
            let repository = FakeRepository::default();
            let use_case = DeleteShoppingList::new(&repository);

            assert!(use_case.execute(invalid).block_on().is_err());
            assert!(repository.call.lock().unwrap().is_none());
        }
    }

    #[test]
    fn repeated_delete_returns_not_found() {
        let repository = FakeRepository::default();
        let use_case = DeleteShoppingList::new(&repository);

        use_case
            .execute(command())
            .block_on()
            .expect("first delete succeeds");
        assert_eq!(
            use_case.execute(command()).block_on(),
            Err(DeleteShoppingListError::Repository(
                RepositoryError::NotFound
            ))
        );
    }

    #[test]
    fn propagates_repository_failures_including_forbidden_and_version_exhaustion() {
        for error in [
            RepositoryError::NotFound,
            RepositoryError::Forbidden,
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
            let use_case = DeleteShoppingList::new(&repository);

            assert_eq!(
                use_case.execute(command()).block_on(),
                Err(DeleteShoppingListError::Repository(error))
            );
        }
    }
}

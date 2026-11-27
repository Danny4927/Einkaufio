//! Access-scoped shopping-list rename use case (ADR 015).

use einkaufio_domain::{
    AggregateVersion, IdentifierError, ListId, ListName, ListNameError, ListOperation,
    ShoppingList, UserId,
};

use crate::ports::{ListAccessRequest, RepositoryError, ShoppingListRepository};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenameShoppingListCommand {
    /// Identity from verified authentication context, never from client input.
    pub actor_id: String,
    pub list_id: String,
    pub expected_version: AggregateVersion,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RenameShoppingListError {
    InvalidActor(IdentifierError),
    InvalidListId(IdentifierError),
    InvalidName(ListNameError),
    Repository(RepositoryError),
}

pub struct RenameShoppingList<'a, R> {
    repository: &'a R,
}

impl<'a, R: ShoppingListRepository> RenameShoppingList<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    pub async fn execute(
        &self,
        command: RenameShoppingListCommand,
    ) -> Result<ShoppingList, RenameShoppingListError> {
        let actor_id =
            UserId::try_new(command.actor_id).map_err(RenameShoppingListError::InvalidActor)?;
        let list_id =
            ListId::try_new(command.list_id).map_err(RenameShoppingListError::InvalidListId)?;
        let name = ListName::try_new(command.name).map_err(RenameShoppingListError::InvalidName)?;
        let access = ListAccessRequest::for_operation(actor_id, ListOperation::Rename);

        self.repository
            .rename_list(&access, &list_id, command.expected_version, name)
            .await
            .map_err(RenameShoppingListError::Repository)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use einkaufio_domain::{
        AggregateVersion, ListId, ListName, ListPermission, ShoppingList, UserId,
    };

    use super::{RenameShoppingList, RenameShoppingListCommand, RenameShoppingListError};
    use crate::ports::{ListAccessRequest, RepositoryError, ShoppingListRepository};
    use crate::test_support::BlockOn;

    struct FakeRepository {
        list: Mutex<ShoppingList>,
        failure: Option<RepositoryError>,
        requests: Mutex<Vec<ListAccessRequest>>,
    }

    impl Default for FakeRepository {
        fn default() -> Self {
            Self {
                list: Mutex::new(ShoppingList::new_personal(
                    ListId::try_new("list-1").unwrap(),
                    UserId::try_new("user-1").unwrap(),
                    ListName::try_new("Einkauf").unwrap(),
                )),
                failure: None,
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    impl ShoppingListRepository for FakeRepository {
        async fn create_personal(&self, _list: ShoppingList) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn rename_list(
            &self,
            access: &ListAccessRequest,
            list_id: &ListId,
            expected_version: AggregateVersion,
            name: ListName,
        ) -> Result<ShoppingList, RepositoryError> {
            self.requests.lock().unwrap().push(access.clone());
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }

            let mut stored = self.list.lock().unwrap();
            // Contract: authorize first, then check optimistic concurrency before the no-op
            // name comparison.
            let held = stored.ownership_access(access.principal());
            match held {
                Some(held) if stored.id() == list_id => {
                    if !held.allows(access.permission()) {
                        return Err(RepositoryError::Forbidden);
                    }
                }
                _ => return Err(RepositoryError::NotFound),
            }
            if stored.aggregate_version() != expected_version {
                return Err(RepositoryError::Conflict);
            }
            stored
                .rename(name)
                .map_err(|_| RepositoryError::VersionExhausted)?;
            Ok(stored.clone())
        }
    }

    fn command() -> RenameShoppingListCommand {
        RenameShoppingListCommand {
            actor_id: "user-1".to_owned(),
            list_id: "list-1".to_owned(),
            expected_version: AggregateVersion::try_new(1).unwrap(),
            name: "  Drogerie  ".to_owned(),
        }
    }

    #[test]
    fn requests_administer_permission_for_the_verified_actor() {
        let repository = FakeRepository::default();
        let use_case = RenameShoppingList::new(&repository);

        use_case.execute(command()).block_on().unwrap();

        let requests = repository.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].principal().as_str(), "user-1");
        assert_eq!(requests[0].permission(), ListPermission::Administer);
    }

    #[test]
    fn passes_expected_version_and_returns_incremented_aggregate() {
        let repository = FakeRepository::default();
        let use_case = RenameShoppingList::new(&repository);

        let renamed = use_case
            .execute(command())
            .block_on()
            .expect("rename should succeed");

        assert_eq!(renamed.name().as_str(), "Drogerie");
        assert_eq!(renamed.version(), 2);
        assert_eq!(repository.list.lock().unwrap().version(), 2);
    }

    #[test]
    fn rejects_invalid_actor_list_id_and_name_before_repository_access() {
        for invalid in [
            RenameShoppingListCommand {
                actor_id: " ".to_owned(),
                ..command()
            },
            RenameShoppingListCommand {
                list_id: " ".to_owned(),
                ..command()
            },
            RenameShoppingListCommand {
                name: "  ".to_owned(),
                ..command()
            },
        ] {
            let repository = FakeRepository::default();
            let use_case = RenameShoppingList::new(&repository);

            assert!(use_case.execute(invalid).block_on().is_err());
            assert!(repository.requests.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn principal_without_access_gets_not_found() {
        let repository = FakeRepository::default();
        let use_case = RenameShoppingList::new(&repository);

        assert_eq!(
            use_case
                .execute(RenameShoppingListCommand {
                    actor_id: "stranger".to_owned(),
                    ..command()
                })
                .block_on(),
            Err(RenameShoppingListError::Repository(
                RepositoryError::NotFound
            ))
        );
        assert_eq!(repository.list.lock().unwrap().version(), 1);
    }

    #[test]
    fn stale_version_conflicts_even_when_requested_name_is_unchanged() {
        let repository = FakeRepository::default();
        let use_case = RenameShoppingList::new(&repository);
        let stale_no_op = RenameShoppingListCommand {
            expected_version: AggregateVersion::try_new(2).unwrap(),
            name: "Einkauf".to_owned(),
            ..command()
        };

        assert_eq!(
            use_case.execute(stale_no_op).block_on(),
            Err(RenameShoppingListError::Repository(
                RepositoryError::Conflict
            ))
        );
        assert_eq!(repository.list.lock().unwrap().version(), 1);
    }

    #[test]
    fn current_version_no_op_does_not_increment_version() {
        let repository = FakeRepository::default();
        let use_case = RenameShoppingList::new(&repository);
        let no_op = RenameShoppingListCommand {
            name: "Einkauf".to_owned(),
            ..command()
        };

        let unchanged = use_case.execute(no_op).block_on().unwrap();

        assert_eq!(unchanged.version(), 1);
    }

    #[test]
    fn propagates_all_repository_failures_without_losing_version_exhaustion() {
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
            let use_case = RenameShoppingList::new(&repository);

            assert_eq!(
                use_case.execute(command()).block_on(),
                Err(RenameShoppingListError::Repository(error))
            );
        }
    }
}

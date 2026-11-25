//! Owner-scoped personal-list rename use case.

use einkaufio_domain::{
    AggregateVersion, IdentifierError, ListId, ListName, ListNameError, ShoppingList, UserId,
};

use crate::ports::{RepositoryError, ShoppingListRepository};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenamePersonalListCommand {
    /// Identity from verified authentication context, never from client input.
    pub actor_id: String,
    pub list_id: String,
    pub expected_version: AggregateVersion,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RenamePersonalListError {
    InvalidActor(IdentifierError),
    InvalidListId(IdentifierError),
    InvalidName(ListNameError),
    Repository(RepositoryError),
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
        let actor_id =
            UserId::try_new(command.actor_id).map_err(RenamePersonalListError::InvalidActor)?;
        let list_id =
            ListId::try_new(command.list_id).map_err(RenamePersonalListError::InvalidListId)?;
        let name = ListName::try_new(command.name).map_err(RenamePersonalListError::InvalidName)?;

        self.repository
            .rename_personal(&actor_id, &list_id, command.expected_version, name)
            .map_err(RenamePersonalListError::Repository)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use einkaufio_domain::{AggregateVersion, ListId, ListName, ShoppingList, UserId};

    use super::{RenamePersonalList, RenamePersonalListCommand, RenamePersonalListError};
    use crate::ports::{RepositoryError, ShoppingListRepository};

    struct FakeRepository {
        list: RefCell<ShoppingList>,
        failure: Option<RepositoryError>,
        calls: RefCell<usize>,
    }

    impl Default for FakeRepository {
        fn default() -> Self {
            Self {
                list: RefCell::new(ShoppingList::new_personal(
                    ListId::try_new("list-1").unwrap(),
                    UserId::try_new("user-1").unwrap(),
                    ListName::try_new("Einkauf").unwrap(),
                )),
                failure: None,
                calls: RefCell::new(0),
            }
        }
    }

    impl ShoppingListRepository for FakeRepository {
        fn create_personal(&self, _list: ShoppingList) -> Result<(), RepositoryError> {
            Ok(())
        }

        fn rename_personal(
            &self,
            actor_id: &UserId,
            list_id: &ListId,
            expected_version: AggregateVersion,
            name: ListName,
        ) -> Result<ShoppingList, RepositoryError> {
            *self.calls.borrow_mut() += 1;
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }

            let mut stored = self.list.borrow_mut();
            // Contract: check optimistic concurrency before the no-op name comparison.
            if stored.owner_id() != actor_id || stored.id() != list_id {
                return Err(RepositoryError::NotFound);
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

    fn command() -> RenamePersonalListCommand {
        RenamePersonalListCommand {
            actor_id: "user-1".to_owned(),
            list_id: "list-1".to_owned(),
            expected_version: AggregateVersion::try_new(1).unwrap(),
            name: "  Drogerie  ".to_owned(),
        }
    }

    #[test]
    fn passes_actor_and_expected_version_and_returns_incremented_aggregate() {
        let repository = FakeRepository::default();
        let use_case = RenamePersonalList::new(&repository);

        let renamed = use_case.execute(command()).expect("rename should succeed");

        assert_eq!(renamed.name().as_str(), "Drogerie");
        assert_eq!(renamed.version(), 2);
        assert_eq!(repository.list.borrow().version(), 2);
        assert_eq!(*repository.calls.borrow(), 1);
    }

    #[test]
    fn rejects_invalid_actor_list_id_and_name_before_repository_access() {
        for invalid in [
            RenamePersonalListCommand {
                actor_id: " ".to_owned(),
                ..command()
            },
            RenamePersonalListCommand {
                list_id: " ".to_owned(),
                ..command()
            },
            RenamePersonalListCommand {
                name: "  ".to_owned(),
                ..command()
            },
        ] {
            let repository = FakeRepository::default();
            let use_case = RenamePersonalList::new(&repository);

            assert!(use_case.execute(invalid).is_err());
            assert_eq!(*repository.calls.borrow(), 0);
        }
    }

    #[test]
    fn stale_version_conflicts_even_when_requested_name_is_unchanged() {
        let repository = FakeRepository::default();
        let use_case = RenamePersonalList::new(&repository);
        let stale_no_op = RenamePersonalListCommand {
            expected_version: AggregateVersion::try_new(2).unwrap(),
            name: "Einkauf".to_owned(),
            ..command()
        };

        assert_eq!(
            use_case.execute(stale_no_op),
            Err(RenamePersonalListError::Repository(
                RepositoryError::Conflict
            ))
        );
        assert_eq!(repository.list.borrow().version(), 1);
    }

    #[test]
    fn current_version_no_op_does_not_increment_version() {
        let repository = FakeRepository::default();
        let use_case = RenamePersonalList::new(&repository);
        let no_op = RenamePersonalListCommand {
            name: "Einkauf".to_owned(),
            ..command()
        };

        let unchanged = use_case.execute(no_op).unwrap();

        assert_eq!(unchanged.version(), 1);
    }

    #[test]
    fn propagates_all_repository_failures_without_losing_version_exhaustion() {
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
            let use_case = RenamePersonalList::new(&repository);

            assert_eq!(
                use_case.execute(command()),
                Err(RenamePersonalListError::Repository(error))
            );
        }
    }
}

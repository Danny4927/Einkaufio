//! Use case for listing the authenticated user's personal shopping lists.

use einkaufio_domain::{IdentifierError, ShoppingList, UserId};

use crate::ports::{RepositoryError, ShoppingListRepository};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListPersonalListsQuery {
    /// Derived from verified authentication context, never from a query parameter or body.
    pub owner_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListPersonalListsError {
    InvalidOwner(IdentifierError),
    PersistenceUnavailable,
    RepositoryInvariantViolation,
}

pub struct ListPersonalLists<'a> {
    repository: &'a dyn ShoppingListRepository,
}

impl<'a> ListPersonalLists<'a> {
    pub fn new(repository: &'a dyn ShoppingListRepository) -> Self {
        Self { repository }
    }

    pub fn execute(
        &self,
        query: ListPersonalListsQuery,
    ) -> Result<Vec<ShoppingList>, ListPersonalListsError> {
        let owner_id =
            UserId::try_new(query.owner_id).map_err(ListPersonalListsError::InvalidOwner)?;

        self.repository
            .list_personal_for_owner(&owner_id)
            .map_err(|error| match error {
                RepositoryError::Unavailable => ListPersonalListsError::PersistenceUnavailable,
                RepositoryError::AlreadyExists
                | RepositoryError::NotFound
                | RepositoryError::Conflict
                | RepositoryError::ItemLimitReached
                | RepositoryError::VersionExhausted => {
                    ListPersonalListsError::RepositoryInvariantViolation
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use einkaufio_domain::{ListId, ListName, ShoppingList, UserId};

    use super::{ListPersonalLists, ListPersonalListsError, ListPersonalListsQuery};
    use crate::ports::{RepositoryError, ShoppingListRepository};

    #[derive(Default)]
    struct FakeRepository {
        lists: Vec<ShoppingList>,
        queried_owner: RefCell<Option<UserId>>,
        failure: Option<RepositoryError>,
    }

    impl ShoppingListRepository for FakeRepository {
        fn create_personal(&self, _list: ShoppingList) -> Result<(), RepositoryError> {
            Ok(())
        }

        fn list_personal_for_owner(
            &self,
            owner_id: &UserId,
        ) -> Result<Vec<ShoppingList>, RepositoryError> {
            *self.queried_owner.borrow_mut() = Some(owner_id.clone());
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            Ok(self
                .lists
                .iter()
                .filter(|list| list.owner_id() == owner_id)
                .cloned()
                .collect())
        }
    }

    fn list(id: &str, owner: &str, name: &str) -> ShoppingList {
        ShoppingList::new_personal(
            ListId::try_new(id).unwrap(),
            UserId::try_new(owner).unwrap(),
            ListName::try_new(name).unwrap(),
        )
    }

    #[test]
    fn returns_only_lists_for_the_authenticated_owner() {
        let repository = FakeRepository {
            lists: vec![
                list("list-1", "user-a", "Wocheneinkauf"),
                list("list-2", "user-b", "Geburtstag"),
                list("list-3", "user-a", "Drogerie"),
            ],
            ..Default::default()
        };
        let use_case = ListPersonalLists::new(&repository);

        let result = use_case
            .execute(ListPersonalListsQuery {
                owner_id: "user-a".to_owned(),
            })
            .expect("query should succeed");

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id().as_str(), "list-1");
        assert_eq!(result[1].id().as_str(), "list-3");
        assert_eq!(
            repository.queried_owner.borrow().as_ref().unwrap().as_str(),
            "user-a"
        );
    }

    #[test]
    fn rejects_invalid_owner_before_repository_access() {
        let repository = FakeRepository::default();
        let use_case = ListPersonalLists::new(&repository);

        assert!(matches!(
            use_case.execute(ListPersonalListsQuery {
                owner_id: " ".to_owned(),
            }),
            Err(ListPersonalListsError::InvalidOwner(_))
        ));
        assert!(repository.queried_owner.borrow().is_none());
    }

    #[test]
    fn maps_storage_outage_to_application_error() {
        let repository = FakeRepository {
            failure: Some(RepositoryError::Unavailable),
            ..Default::default()
        };
        let use_case = ListPersonalLists::new(&repository);

        assert_eq!(
            use_case.execute(ListPersonalListsQuery {
                owner_id: "user-a".to_owned(),
            }),
            Err(ListPersonalListsError::PersistenceUnavailable)
        );
    }
}

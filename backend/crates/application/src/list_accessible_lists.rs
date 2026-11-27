//! Use case for listing every shopping list the authenticated principal can access.

use einkaufio_domain::{IdentifierError, ShoppingList, UserId};

use crate::ports::{RepositoryError, ShoppingListRepository};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListAccessibleListsQuery {
    /// Derived from verified authentication context, never from a query parameter or body.
    pub principal_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListAccessibleListsError {
    InvalidPrincipal(IdentifierError),
    PersistenceUnavailable,
    RepositoryInvariantViolation,
}

pub struct ListAccessibleLists<'a, R> {
    repository: &'a R,
}

impl<'a, R: ShoppingListRepository> ListAccessibleLists<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    pub async fn execute(
        &self,
        query: ListAccessibleListsQuery,
    ) -> Result<Vec<ShoppingList>, ListAccessibleListsError> {
        let principal = UserId::try_new(query.principal_id)
            .map_err(ListAccessibleListsError::InvalidPrincipal)?;

        self.repository
            .list_accessible(&principal)
            .await
            .map_err(|error| match error {
                RepositoryError::Unavailable => ListAccessibleListsError::PersistenceUnavailable,
                RepositoryError::AlreadyExists
                | RepositoryError::NotFound
                | RepositoryError::Forbidden
                | RepositoryError::Conflict
                | RepositoryError::ItemLimitReached
                | RepositoryError::VersionExhausted
                | RepositoryError::InvalidCursor => {
                    ListAccessibleListsError::RepositoryInvariantViolation
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use einkaufio_domain::{ListId, ListName, ShoppingList, UserId};

    use super::{ListAccessibleLists, ListAccessibleListsError, ListAccessibleListsQuery};
    use crate::ports::{RepositoryError, ShoppingListRepository};
    use crate::test_support::BlockOn;

    #[derive(Default)]
    struct FakeRepository {
        lists: Vec<ShoppingList>,
        queried_principal: Mutex<Option<UserId>>,
        failure: Option<RepositoryError>,
    }

    impl ShoppingListRepository for FakeRepository {
        async fn create_personal(&self, _list: ShoppingList) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn list_accessible(
            &self,
            principal: &UserId,
        ) -> Result<Vec<ShoppingList>, RepositoryError> {
            *self.queried_principal.lock().unwrap() = Some(principal.clone());
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            Ok(self
                .lists
                .iter()
                .filter(|list| list.ownership_access(principal).is_some())
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
    fn returns_lists_the_repository_resolves_for_the_verified_principal() {
        let repository = FakeRepository {
            lists: vec![
                list("list-1", "user-a", "Wocheneinkauf"),
                list("list-2", "user-b", "Geburtstag"),
                list("list-3", "user-a", "Drogerie"),
            ],
            ..Default::default()
        };
        let use_case = ListAccessibleLists::new(&repository);

        let result = use_case
            .execute(ListAccessibleListsQuery {
                principal_id: "user-a".to_owned(),
            })
            .block_on()
            .expect("query should succeed");

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id().as_str(), "list-1");
        assert_eq!(result[1].id().as_str(), "list-3");
        assert_eq!(
            repository
                .queried_principal
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .as_str(),
            "user-a"
        );
    }

    #[test]
    fn rejects_invalid_principal_before_repository_access() {
        let repository = FakeRepository::default();
        let use_case = ListAccessibleLists::new(&repository);

        assert!(matches!(
            use_case
                .execute(ListAccessibleListsQuery {
                    principal_id: " ".to_owned(),
                })
                .block_on(),
            Err(ListAccessibleListsError::InvalidPrincipal(_))
        ));
        assert!(repository.queried_principal.lock().unwrap().is_none());
    }

    #[test]
    fn maps_storage_outage_to_application_error() {
        let repository = FakeRepository {
            failure: Some(RepositoryError::Unavailable),
            ..Default::default()
        };
        let use_case = ListAccessibleLists::new(&repository);

        assert_eq!(
            use_case
                .execute(ListAccessibleListsQuery {
                    principal_id: "user-a".to_owned(),
                })
                .block_on(),
            Err(ListAccessibleListsError::PersistenceUnavailable)
        );
    }

    #[test]
    fn maps_impossible_query_outcomes_to_invariant_violation() {
        for failure in [RepositoryError::Forbidden, RepositoryError::NotFound] {
            let repository = FakeRepository {
                failure: Some(failure),
                ..Default::default()
            };
            assert_eq!(
                ListAccessibleLists::new(&repository)
                    .execute(ListAccessibleListsQuery {
                        principal_id: "user-a".to_owned(),
                    })
                    .block_on(),
                Err(ListAccessibleListsError::RepositoryInvariantViolation)
            );
        }
    }
}

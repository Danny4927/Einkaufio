//! Use case for creating a personal shopping list.

use einkaufio_domain::{IdentifierError, ListId, ListName, ListNameError, ShoppingList, UserId};

use crate::ports::{ListIdGenerator, RepositoryError, ShoppingListRepository};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatePersonalListCommand {
    /// Identity from the authenticated request context, never from a request body.
    pub owner_id: String,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CreatePersonalListError {
    InvalidOwner(IdentifierError),
    InvalidName(ListNameError),
    InvalidGeneratedId(IdentifierError),
    AlreadyExists,
    PersistenceUnavailable,
    RepositoryInvariantViolation,
}

pub struct CreatePersonalList<'a, R> {
    repository: &'a R,
    id_generator: &'a dyn ListIdGenerator,
}

impl<'a, R: ShoppingListRepository> CreatePersonalList<'a, R> {
    pub fn new(repository: &'a R, id_generator: &'a dyn ListIdGenerator) -> Self {
        Self {
            repository,
            id_generator,
        }
    }

    pub async fn execute(
        &self,
        command: CreatePersonalListCommand,
    ) -> Result<ShoppingList, CreatePersonalListError> {
        let owner_id =
            UserId::try_new(command.owner_id).map_err(CreatePersonalListError::InvalidOwner)?;
        let name = ListName::try_new(command.name).map_err(CreatePersonalListError::InvalidName)?;
        let id = ListId::try_new(self.id_generator.generate())
            .map_err(CreatePersonalListError::InvalidGeneratedId)?;

        let list = ShoppingList::new_personal(id, owner_id, name);
        self.repository
            .create_personal(list.clone())
            .await
            .map_err(map_repository_error)?;

        Ok(list)
    }
}

fn map_repository_error(error: RepositoryError) -> CreatePersonalListError {
    match error {
        RepositoryError::AlreadyExists | RepositoryError::Conflict => {
            CreatePersonalListError::AlreadyExists
        }
        // A newly generated list ID cannot legitimately be missing or require a permission the
        // creating owner lacks, and the list-create repository contract has no parent-item
        // operation that could hit the item limit.
        RepositoryError::NotFound
        | RepositoryError::Forbidden
        | RepositoryError::ItemLimitReached
        | RepositoryError::VersionExhausted => {
            CreatePersonalListError::RepositoryInvariantViolation
        }
        RepositoryError::Unavailable => CreatePersonalListError::PersistenceUnavailable,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use einkaufio_domain::ShoppingList;

    use super::{CreatePersonalList, CreatePersonalListCommand, CreatePersonalListError};
    use crate::ports::{ListIdGenerator, RepositoryError, ShoppingListRepository};
    use crate::test_support::BlockOn;

    #[derive(Default)]
    struct FakeRepository {
        created: Mutex<Vec<ShoppingList>>,
        failure: Option<RepositoryError>,
    }

    impl ShoppingListRepository for FakeRepository {
        async fn create_personal(&self, list: ShoppingList) -> Result<(), RepositoryError> {
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            self.created.lock().unwrap().push(list);
            Ok(())
        }
    }

    struct FixedIdGenerator(&'static str);

    impl ListIdGenerator for FixedIdGenerator {
        fn generate(&self) -> String {
            self.0.to_owned()
        }
    }

    fn command() -> CreatePersonalListCommand {
        CreatePersonalListCommand {
            owner_id: "authenticated-user-1".to_owned(),
            name: "  Wocheneinkauf  ".to_owned(),
        }
    }

    #[test]
    fn creates_personal_list_for_authenticated_owner() {
        let repository = FakeRepository::default();
        let ids = FixedIdGenerator("list-123");
        let use_case = CreatePersonalList::new(&repository, &ids);

        let created = use_case
            .execute(command())
            .block_on()
            .expect("list should be created");

        assert_eq!(created.id().as_str(), "list-123");
        assert_eq!(created.owner_id().as_str(), "authenticated-user-1");
        assert_eq!(created.name().as_str(), "Wocheneinkauf");
        assert_eq!(created.version(), 1);
        assert_eq!(repository.created.lock().unwrap().len(), 1);
    }

    #[test]
    fn rejects_invalid_name_before_calling_repository() {
        let repository = FakeRepository::default();
        let ids = FixedIdGenerator("list-123");
        let use_case = CreatePersonalList::new(&repository, &ids);
        let invalid = CreatePersonalListCommand {
            name: "  ".to_owned(),
            ..command()
        };

        assert!(matches!(
            use_case.execute(invalid).block_on(),
            Err(CreatePersonalListError::InvalidName(_))
        ));
        assert!(repository.created.lock().unwrap().is_empty());
    }

    #[test]
    fn rejects_invalid_owner_before_calling_repository() {
        let repository = FakeRepository::default();
        let ids = FixedIdGenerator("list-123");
        let use_case = CreatePersonalList::new(&repository, &ids);
        let invalid = CreatePersonalListCommand {
            owner_id: " ".to_owned(),
            ..command()
        };

        assert!(matches!(
            use_case.execute(invalid).block_on(),
            Err(CreatePersonalListError::InvalidOwner(_))
        ));
        assert!(repository.created.lock().unwrap().is_empty());
    }

    #[test]
    fn rejects_invalid_generated_id() {
        let repository = FakeRepository::default();
        let ids = FixedIdGenerator(" ");
        let use_case = CreatePersonalList::new(&repository, &ids);

        assert!(matches!(
            use_case.execute(command()).block_on(),
            Err(CreatePersonalListError::InvalidGeneratedId(_))
        ));
        assert!(repository.created.lock().unwrap().is_empty());
    }

    #[test]
    fn maps_repository_conflict_without_leaking_adapter_details() {
        let repository = FakeRepository {
            failure: Some(RepositoryError::AlreadyExists),
            ..Default::default()
        };
        let ids = FixedIdGenerator("list-123");
        let use_case = CreatePersonalList::new(&repository, &ids);

        assert_eq!(
            use_case.execute(command()).block_on(),
            Err(CreatePersonalListError::AlreadyExists)
        );
    }

    #[test]
    fn maps_impossible_create_outcomes_to_adapter_invariant_violation() {
        for failure in [RepositoryError::NotFound, RepositoryError::Forbidden] {
            let repository = FakeRepository {
                failure: Some(failure),
                ..Default::default()
            };
            let ids = FixedIdGenerator("list-123");
            let use_case = CreatePersonalList::new(&repository, &ids);

            assert_eq!(
                use_case.execute(command()).block_on(),
                Err(CreatePersonalListError::RepositoryInvariantViolation)
            );
        }
    }

    #[test]
    fn maps_repository_outage_to_application_error() {
        let repository = FakeRepository {
            failure: Some(RepositoryError::Unavailable),
            ..Default::default()
        };
        let ids = FixedIdGenerator("list-123");
        let use_case = CreatePersonalList::new(&repository, &ids);

        assert_eq!(
            use_case.execute(command()).block_on(),
            Err(CreatePersonalListError::PersistenceUnavailable)
        );
    }
}

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

pub struct CreatePersonalList<'a> {
    repository: &'a dyn ShoppingListRepository,
    id_generator: &'a dyn ListIdGenerator,
}

impl<'a> CreatePersonalList<'a> {
    pub fn new(
        repository: &'a dyn ShoppingListRepository,
        id_generator: &'a dyn ListIdGenerator,
    ) -> Self {
        Self {
            repository,
            id_generator,
        }
    }

    pub fn execute(
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
            .map_err(map_repository_error)?;

        Ok(list)
    }
}

fn map_repository_error(error: RepositoryError) -> CreatePersonalListError {
    match error {
        RepositoryError::AlreadyExists | RepositoryError::Conflict => {
            CreatePersonalListError::AlreadyExists
        }
        RepositoryError::NotFound => CreatePersonalListError::RepositoryInvariantViolation,
        RepositoryError::ItemLimitReached => CreatePersonalListError::RepositoryInvariantViolation,
        RepositoryError::Unavailable => CreatePersonalListError::PersistenceUnavailable,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use einkaufio_domain::ShoppingList;

    use super::{CreatePersonalList, CreatePersonalListCommand, CreatePersonalListError};
    use crate::ports::{ListIdGenerator, RepositoryError, ShoppingListRepository};

    #[derive(Default)]
    struct FakeRepository {
        created: RefCell<Vec<ShoppingList>>,
        failure: Option<RepositoryError>,
    }

    impl ShoppingListRepository for FakeRepository {
        fn create_personal(&self, list: ShoppingList) -> Result<(), RepositoryError> {
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            self.created.borrow_mut().push(list);
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

        let created = use_case.execute(command()).expect("list should be created");

        assert_eq!(created.id().as_str(), "list-123");
        assert_eq!(created.owner_id().as_str(), "authenticated-user-1");
        assert_eq!(created.name().as_str(), "Wocheneinkauf");
        assert_eq!(created.version(), 1);
        assert_eq!(repository.created.borrow().len(), 1);
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
            use_case.execute(invalid),
            Err(CreatePersonalListError::InvalidName(_))
        ));
        assert!(repository.created.borrow().is_empty());
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
            use_case.execute(invalid),
            Err(CreatePersonalListError::InvalidOwner(_))
        ));
        assert!(repository.created.borrow().is_empty());
    }

    #[test]
    fn rejects_invalid_generated_id() {
        let repository = FakeRepository::default();
        let ids = FixedIdGenerator(" ");
        let use_case = CreatePersonalList::new(&repository, &ids);

        assert!(matches!(
            use_case.execute(command()),
            Err(CreatePersonalListError::InvalidGeneratedId(_))
        ));
        assert!(repository.created.borrow().is_empty());
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
            use_case.execute(command()),
            Err(CreatePersonalListError::AlreadyExists)
        );
    }

    #[test]
    fn maps_missing_list_during_create_to_adapter_invariant_violation() {
        let repository = FakeRepository {
            failure: Some(RepositoryError::NotFound),
            ..Default::default()
        };
        let ids = FixedIdGenerator("list-123");
        let use_case = CreatePersonalList::new(&repository, &ids);

        assert_eq!(
            use_case.execute(command()),
            Err(CreatePersonalListError::RepositoryInvariantViolation)
        );
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
            use_case.execute(command()),
            Err(CreatePersonalListError::PersistenceUnavailable)
        );
    }
}

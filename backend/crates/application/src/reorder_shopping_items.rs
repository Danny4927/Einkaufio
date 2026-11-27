//! Use case for reordering all items of a shopping list (roadmap B2).

use einkaufio_domain::{
    AggregateVersion, ItemId, ItemOrder, ListId, ListOperation, ShoppingItem, UserId,
};

use crate::ports::{ListAccessRequest, ShoppingItemRepository};
use crate::shopping_items::{ShoppingItemError, map_repository_error};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReorderShoppingItemsCommand {
    /// Derived from verified authentication context, never accepted from the client body.
    pub actor_id: String,
    pub list_id: String,
    pub expected_list_version: u64,
    /// Complete new order: every current item id exactly once, first item first.
    pub item_ids: Vec<String>,
}

pub struct ReorderShoppingItems<'a, R> {
    repository: &'a R,
}

impl<'a, R: ShoppingItemRepository> ReorderShoppingItems<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    /// Returns the items in their new order and the parent-list version.
    pub async fn execute(
        &self,
        command: ReorderShoppingItemsCommand,
    ) -> Result<(Vec<ShoppingItem>, u64), ShoppingItemError> {
        let actor =
            UserId::try_new(command.actor_id).map_err(|_| ShoppingItemError::InvalidActor)?;
        let list_id =
            ListId::try_new(command.list_id).map_err(|_| ShoppingItemError::InvalidListId)?;
        let expected = AggregateVersion::try_new(command.expected_list_version)
            .map_err(|_| ShoppingItemError::InvalidExpectedVersion)?;
        let item_ids = command
            .item_ids
            .into_iter()
            .map(ItemId::try_new)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| ShoppingItemError::InvalidItemId)?;
        let order = ItemOrder::try_new(item_ids).map_err(ShoppingItemError::InvalidOrder)?;
        let access = ListAccessRequest::for_operation(actor, ListOperation::EditItems);
        self.repository
            .reorder_items(&access, &list_id, expected, &order)
            .await
            .map(|(items, version)| (items, version.as_u64()))
            .map_err(map_repository_error)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use einkaufio_domain::{ItemOrderError, ListPermission};

    use super::*;
    use crate::ports::RepositoryError;
    use crate::test_support::BlockOn;

    #[derive(Default)]
    struct FakeRepository {
        calls: Mutex<Vec<(ListAccessRequest, ListId, AggregateVersion, ItemOrder)>>,
        failure: Option<RepositoryError>,
    }

    impl ShoppingItemRepository for FakeRepository {
        async fn add_item(
            &self,
            _: &ListAccessRequest,
            _: &ListId,
            _: AggregateVersion,
            _: ShoppingItem,
        ) -> Result<AggregateVersion, RepositoryError> {
            Err(RepositoryError::Unavailable)
        }
        async fn set_item_checked(
            &self,
            _: &ListAccessRequest,
            _: &ListId,
            _: &ItemId,
            _: u64,
            _: bool,
        ) -> Result<(ShoppingItem, AggregateVersion), RepositoryError> {
            Err(RepositoryError::Unavailable)
        }
        async fn delete_item(
            &self,
            _: &ListAccessRequest,
            _: &ListId,
            _: &ItemId,
            _: u64,
        ) -> Result<AggregateVersion, RepositoryError> {
            Err(RepositoryError::Unavailable)
        }
        async fn reorder_items(
            &self,
            access: &ListAccessRequest,
            list_id: &ListId,
            expected: AggregateVersion,
            order: &ItemOrder,
        ) -> Result<(Vec<ShoppingItem>, AggregateVersion), RepositoryError> {
            self.calls.lock().unwrap().push((
                access.clone(),
                list_id.clone(),
                expected,
                order.clone(),
            ));
            match &self.failure {
                Some(error) => Err(error.clone()),
                None => Ok((Vec::new(), expected.checked_next().unwrap())),
            }
        }
    }

    fn command(item_ids: &[&str]) -> ReorderShoppingItemsCommand {
        ReorderShoppingItemsCommand {
            actor_id: "user-1".into(),
            list_id: "list-1".into(),
            expected_list_version: 4,
            item_ids: item_ids.iter().map(|id| (*id).to_owned()).collect(),
        }
    }

    #[test]
    fn passes_validated_order_with_edit_permission_for_the_verified_actor() {
        let repository = FakeRepository::default();
        let (_, version) = ReorderShoppingItems::new(&repository)
            .execute(command(&["b", "a"]))
            .block_on()
            .unwrap();
        assert_eq!(version, 5);
        let calls = repository.calls.lock().unwrap();
        let (access, list_id, expected, order) = &calls[0];
        assert_eq!(access.principal().as_str(), "user-1");
        assert_eq!(access.permission(), ListPermission::Contribute);
        assert_eq!(list_id.as_str(), "list-1");
        assert_eq!(expected.as_u64(), 4);
        assert_eq!(order.position_of(&ItemId::try_new("b").unwrap()), Some(0));
    }

    #[test]
    fn rejects_invalid_input_before_the_repository() {
        let repository = FakeRepository::default();
        let use_case = ReorderShoppingItems::new(&repository);
        assert_eq!(
            use_case.execute(command(&["a", "a"])).block_on(),
            Err(ShoppingItemError::InvalidOrder(
                ItemOrderError::DuplicateItem
            ))
        );
        assert_eq!(
            use_case.execute(command(&["a", " "])).block_on(),
            Err(ShoppingItemError::InvalidItemId)
        );
        let mut zero = command(&["a"]);
        zero.expected_list_version = 0;
        assert_eq!(
            use_case.execute(zero).block_on(),
            Err(ShoppingItemError::InvalidExpectedVersion)
        );
        let mut actor = command(&["a"]);
        actor.actor_id = String::new();
        assert_eq!(
            use_case.execute(actor).block_on(),
            Err(ShoppingItemError::InvalidActor)
        );
        assert!(repository.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn maps_repository_failures_without_masking_them() {
        for (failure, expected) in [
            (RepositoryError::NotFound, ShoppingItemError::NotFound),
            (RepositoryError::Forbidden, ShoppingItemError::Forbidden),
            (RepositoryError::Conflict, ShoppingItemError::Conflict),
            (
                RepositoryError::VersionExhausted,
                ShoppingItemError::VersionExhausted,
            ),
            (
                RepositoryError::Unavailable,
                ShoppingItemError::PersistenceUnavailable,
            ),
        ] {
            let repository = FakeRepository {
                failure: Some(failure),
                ..FakeRepository::default()
            };
            assert_eq!(
                ReorderShoppingItems::new(&repository)
                    .execute(command(&["a"]))
                    .block_on(),
                Err(expected)
            );
        }
    }
}

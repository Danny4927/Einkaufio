//! In-memory reference adapter used by the repository contract tests.
//!
//! This adapter is deliberately small and deterministic. It models repository port
//! guarantees without durable persistence. All state sits behind one `Mutex`, so every port
//! operation is atomic and the adapter is `Send + Sync`; it does not model the conditional
//! writes and transaction cancellations of a real database (ADR 014).
//!
//! Access is resolved per ADR 015: personal ownership grants administrator access, and
//! contributor grants stand in for household membership (roadmap C2) so the shared-access
//! contract can exercise permission enforcement before households exist.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use einkaufio_application::{
    ListAccessRequest, RepositoryError, ShoppingItemRepository, ShoppingListRepository,
};
use einkaufio_domain::{
    AggregateVersion, ItemDetails, ItemId, ListAccess, ListId, ListName, ShoppingItem,
    ShoppingList, UserId,
};

use crate::repository_contracts::SharedListFixture;

#[derive(Default)]
struct State {
    lists: HashMap<ListId, ShoppingList>,
    items: HashMap<(ListId, ItemId), ShoppingItem>,
    next_positions: HashMap<ListId, u32>,
    /// Non-ownership access grants (stand-in for household membership until roadmap C2).
    grants: HashMap<(ListId, UserId), ListAccess>,
}

impl State {
    /// Resolves the strongest access the principal currently holds on an existing list.
    fn access(&self, principal: &UserId, list_id: &ListId) -> Option<ListAccess> {
        let list = self.lists.get(list_id)?;
        let granted = self
            .grants
            .get(&(list_id.clone(), principal.clone()))
            .copied();
        list.ownership_access(principal).max(granted)
    }

    /// Authorizes before anything else is disclosed: no access is indistinguishable from a
    /// missing list; insufficient access is only reported to principals that can see the list.
    fn authorize(
        &mut self,
        request: &ListAccessRequest,
        list_id: &ListId,
    ) -> Result<&mut ShoppingList, RepositoryError> {
        match self.access(request.principal(), list_id) {
            None => Err(RepositoryError::NotFound),
            Some(held) if !held.allows(request.permission()) => Err(RepositoryError::Forbidden),
            Some(_) => self.lists.get_mut(list_id).ok_or(RepositoryError::NotFound),
        }
    }

    fn list_version(&self, list_id: &ListId) -> Result<AggregateVersion, RepositoryError> {
        self.lists
            .get(list_id)
            .map(ShoppingList::aggregate_version)
            .ok_or(RepositoryError::NotFound)
    }
}

#[derive(Default)]
pub struct InMemoryRepository {
    state: Mutex<State>,
}

impl InMemoryRepository {
    pub fn new() -> Self {
        Self::default()
    }

    /// A poisoned lock means an earlier operation panicked mid-mutation; fail closed.
    fn state(&self) -> Result<MutexGuard<'_, State>, RepositoryError> {
        self.state.lock().map_err(|_| RepositoryError::Unavailable)
    }
}

impl ShoppingListRepository for InMemoryRepository {
    async fn create_personal(&self, list: ShoppingList) -> Result<(), RepositoryError> {
        let mut state = self.state()?;
        if state.lists.contains_key(list.id()) {
            return Err(RepositoryError::AlreadyExists);
        }
        state.lists.insert(list.id().clone(), list);
        Ok(())
    }

    async fn list_accessible(
        &self,
        principal: &UserId,
    ) -> Result<Vec<ShoppingList>, RepositoryError> {
        let state = self.state()?;
        let mut lists: Vec<_> = state
            .lists
            .values()
            .filter(|list| state.access(principal, list.id()).is_some())
            .cloned()
            .collect();
        lists.sort_by(|left, right| left.id().as_str().cmp(right.id().as_str()));
        Ok(lists)
    }

    async fn rename_list(
        &self,
        access: &ListAccessRequest,
        list_id: &ListId,
        expected_version: AggregateVersion,
        name: ListName,
    ) -> Result<ShoppingList, RepositoryError> {
        let mut state = self.state()?;
        let list = state.authorize(access, list_id)?;
        if list.aggregate_version() != expected_version {
            return Err(RepositoryError::Conflict);
        }
        list.rename(name)
            .map_err(|_| RepositoryError::VersionExhausted)?;
        Ok(list.clone())
    }

    async fn delete_list(
        &self,
        access: &ListAccessRequest,
        list_id: &ListId,
        expected_version: AggregateVersion,
    ) -> Result<(), RepositoryError> {
        let mut state = self.state()?;
        let list = state.authorize(access, list_id)?;
        if list.aggregate_version() != expected_version {
            return Err(RepositoryError::Conflict);
        }
        state.lists.remove(list_id);
        state.items.retain(|(candidate, _), _| candidate != list_id);
        state.next_positions.remove(list_id);
        state
            .grants
            .retain(|(candidate, _), _| candidate != list_id);
        Ok(())
    }
}

impl ShoppingItemRepository for InMemoryRepository {
    async fn add_item(
        &self,
        access: &ListAccessRequest,
        list_id: &ListId,
        expected_list_version: AggregateVersion,
        item: ShoppingItem,
    ) -> Result<AggregateVersion, RepositoryError> {
        let mut state = self.state()?;

        // Authorize before exposing version, capacity or item existence.
        if state.authorize(access, list_id)?.aggregate_version() != expected_list_version {
            return Err(RepositoryError::Conflict);
        }
        let key = (list_id.clone(), item.id().clone());
        if state.items.contains_key(&key) {
            return Err(RepositoryError::AlreadyExists);
        }
        let active_count = state
            .items
            .keys()
            .filter(|(candidate, _)| candidate == list_id)
            .count();
        if active_count >= ShoppingList::MAX_ACTIVE_ITEMS {
            return Err(RepositoryError::ItemLimitReached);
        }

        let position = *state.next_positions.get(list_id).unwrap_or(&0);
        let assigned = item.with_position(position);
        let list = state.authorize(access, list_id)?;
        list.record_item_mutation()
            .map_err(|_| RepositoryError::VersionExhausted)?;
        let next_version = list.aggregate_version();
        state.items.insert(key, assigned);
        state.next_positions.insert(list_id.clone(), position + 1);
        Ok(next_version)
    }

    async fn list_items(
        &self,
        access: &ListAccessRequest,
        list_id: &ListId,
    ) -> Result<Vec<ShoppingItem>, RepositoryError> {
        let mut state = self.state()?;
        state.authorize(access, list_id)?;
        let mut items: Vec<_> = state
            .items
            .iter()
            .filter(|((candidate, _), _)| candidate == list_id)
            .map(|(_, item)| item.clone())
            .collect();
        items.sort_by_key(ShoppingItem::position);
        Ok(items)
    }

    async fn set_item_checked(
        &self,
        access: &ListAccessRequest,
        list_id: &ListId,
        item_id: &ItemId,
        expected_item_version: u64,
        checked: bool,
    ) -> Result<(ShoppingItem, AggregateVersion), RepositoryError> {
        let mut state = self.state()?;
        state.authorize(access, list_id)?;

        let key = (list_id.clone(), item_id.clone());
        let stored = state.items.get(&key).ok_or(RepositoryError::NotFound)?;
        if stored.version() != expected_item_version {
            return Err(RepositoryError::Conflict);
        }
        let mut updated = stored.clone();
        if updated
            .set_checked(checked)
            .map_err(|_| RepositoryError::VersionExhausted)?
        {
            state
                .authorize(access, list_id)?
                .record_item_mutation()
                .map_err(|_| RepositoryError::VersionExhausted)?;
        }
        let list_version = state.list_version(list_id)?;
        state.items.insert(key, updated.clone());
        Ok((updated, list_version))
    }

    async fn update_item_details(
        &self,
        access: &ListAccessRequest,
        list_id: &ListId,
        item_id: &ItemId,
        expected_item_version: u64,
        details: ItemDetails,
    ) -> Result<(ShoppingItem, AggregateVersion), RepositoryError> {
        let mut state = self.state()?;
        state.authorize(access, list_id)?;

        let key = (list_id.clone(), item_id.clone());
        let stored = state.items.get(&key).ok_or(RepositoryError::NotFound)?;
        if stored.version() != expected_item_version {
            return Err(RepositoryError::Conflict);
        }
        let mut updated = stored.clone();
        if updated
            .update_details(details)
            .map_err(|_| RepositoryError::VersionExhausted)?
        {
            state
                .authorize(access, list_id)?
                .record_item_mutation()
                .map_err(|_| RepositoryError::VersionExhausted)?;
        }
        let list_version = state.list_version(list_id)?;
        state.items.insert(key, updated.clone());
        Ok((updated, list_version))
    }

    async fn delete_item(
        &self,
        access: &ListAccessRequest,
        list_id: &ListId,
        item_id: &ItemId,
        expected_item_version: u64,
    ) -> Result<AggregateVersion, RepositoryError> {
        let mut state = self.state()?;
        state.authorize(access, list_id)?;

        let key = (list_id.clone(), item_id.clone());
        let stored = state.items.get(&key).ok_or(RepositoryError::NotFound)?;
        if stored.version() != expected_item_version {
            return Err(RepositoryError::Conflict);
        }
        let list = state.authorize(access, list_id)?;
        list.record_item_mutation()
            .map_err(|_| RepositoryError::VersionExhausted)?;
        let next_version = list.aggregate_version();
        state.items.remove(&key);
        Ok(next_version)
    }
}

impl SharedListFixture for InMemoryRepository {
    async fn provision_shared_list(
        &self,
        list: ShoppingList,
        contributor: &UserId,
    ) -> Result<(), RepositoryError> {
        let mut state = self.state()?;
        if state.lists.contains_key(list.id()) {
            return Err(RepositoryError::AlreadyExists);
        }
        state.grants.insert(
            (list.id().clone(), contributor.clone()),
            ListAccess::Contributor,
        );
        state.lists.insert(list.id().clone(), list);
        Ok(())
    }

    async fn revoke_contributor(
        &self,
        list_id: &ListId,
        contributor: &UserId,
    ) -> Result<(), RepositoryError> {
        self.state()?
            .grants
            .remove(&(list_id.clone(), contributor.clone()))
            .map(|_| ())
            .ok_or(RepositoryError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use einkaufio_domain::{ItemName, ListOperation};

    use super::*;

    fn list() -> ShoppingList {
        ShoppingList::new_personal(
            ListId::try_new("concurrent-list").unwrap(),
            UserId::try_new("concurrent-owner").unwrap(),
            ListName::try_new("Concurrent").unwrap(),
        )
    }

    fn item(index: usize) -> ShoppingItem {
        ShoppingItem::new(
            ItemId::try_new(format!("concurrent-item-{index}")).unwrap(),
            ItemName::try_new("Milch").unwrap(),
            None,
            None,
            None,
            0,
            UserId::try_new("concurrent-owner").unwrap(),
        )
    }

    /// Parallel writers holding the same expected list version: exactly one commits. This
    /// also proves at compile time that port futures are `Send + 'static`-spawnable.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_adds_with_the_same_expected_version_commit_exactly_once() {
        let repository = Arc::new(InMemoryRepository::new());
        repository.create_personal(list()).await.unwrap();
        let access = ListAccessRequest::for_operation(
            UserId::try_new("concurrent-owner").unwrap(),
            ListOperation::EditItems,
        );

        let tasks: Vec<_> = (0..16)
            .map(|index| {
                let repository = Arc::clone(&repository);
                let access = access.clone();
                tokio::spawn(async move {
                    repository
                        .add_item(
                            &access,
                            &ListId::try_new("concurrent-list").unwrap(),
                            AggregateVersion::initial(),
                            item(index),
                        )
                        .await
                })
            })
            .collect();
        let mut committed = 0;
        for task in tasks {
            match task.await.unwrap() {
                Ok(version) => {
                    committed += 1;
                    assert_eq!(version, AggregateVersion::try_new(2).unwrap());
                }
                Err(error) => assert_eq!(error, RepositoryError::Conflict),
            }
        }
        assert_eq!(committed, 1);
        let items = repository
            .list_items(&access, &ListId::try_new("concurrent-list").unwrap())
            .await
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].position(), 0);
    }

    #[tokio::test]
    async fn poisoned_state_fails_closed() {
        let repository = Arc::new(InMemoryRepository::new());
        let poisoner = Arc::clone(&repository);
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.state.lock().unwrap();
            panic!("poison the adapter lock");
        })
        .join();

        assert_eq!(
            repository.create_personal(list()).await,
            Err(RepositoryError::Unavailable)
        );
        assert_eq!(
            repository
                .list_accessible(&UserId::try_new("concurrent-owner").unwrap())
                .await,
            Err(RepositoryError::Unavailable)
        );
    }
}

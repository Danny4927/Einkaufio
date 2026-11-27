//! In-memory reference adapter used by the repository contract tests.
//!
//! This adapter is deliberately small and deterministic. It models repository port
//! guarantees without durable persistence or production concurrency. It uses `RefCell`,
//! so it is intentionally `!Sync` and must only be used as single-threaded test support.

use std::cell::RefCell;
use std::collections::HashMap;

use einkaufio_application::{RepositoryError, ShoppingItemRepository, ShoppingListRepository};
use einkaufio_domain::{
    AggregateVersion, ItemDetails, ItemId, ListId, ListName, ShoppingItem, ShoppingList, UserId,
};

#[derive(Default)]
struct State {
    lists: HashMap<ListId, ShoppingList>,
    items: HashMap<(ListId, ItemId), ShoppingItem>,
    next_positions: HashMap<ListId, u32>,
}

#[derive(Default)]
pub struct InMemoryRepository {
    state: RefCell<State>,
}

impl InMemoryRepository {
    pub fn new() -> Self {
        Self::default()
    }

    fn authorized_list<'a>(
        state: &'a mut State,
        actor_id: &UserId,
        list_id: &ListId,
    ) -> Result<&'a mut ShoppingList, RepositoryError> {
        match state.lists.get_mut(list_id) {
            Some(list) if list.owner_id() == actor_id => Ok(list),
            _ => Err(RepositoryError::NotFound),
        }
    }
}

impl ShoppingListRepository for InMemoryRepository {
    fn create_personal(&self, list: ShoppingList) -> Result<(), RepositoryError> {
        let mut state = self.state.borrow_mut();
        if state.lists.contains_key(list.id()) {
            return Err(RepositoryError::AlreadyExists);
        }
        state.lists.insert(list.id().clone(), list);
        Ok(())
    }

    fn list_personal_for_owner(
        &self,
        owner_id: &UserId,
    ) -> Result<Vec<ShoppingList>, RepositoryError> {
        let state = self.state.borrow();
        let mut lists: Vec<_> = state
            .lists
            .values()
            .filter(|list| list.owner_id() == owner_id)
            .cloned()
            .collect();
        lists.sort_by(|left, right| left.id().as_str().cmp(right.id().as_str()));
        Ok(lists)
    }

    fn rename_personal(
        &self,
        actor_id: &UserId,
        list_id: &ListId,
        expected_version: AggregateVersion,
        name: ListName,
    ) -> Result<ShoppingList, RepositoryError> {
        let mut state = self.state.borrow_mut();
        let list = Self::authorized_list(&mut state, actor_id, list_id)?;
        if list.aggregate_version() != expected_version {
            return Err(RepositoryError::Conflict);
        }
        list.rename(name)
            .map_err(|_| RepositoryError::VersionExhausted)?;
        Ok(list.clone())
    }

    fn delete_personal(
        &self,
        actor_id: &UserId,
        list_id: &ListId,
        expected_version: AggregateVersion,
    ) -> Result<(), RepositoryError> {
        let mut state = self.state.borrow_mut();
        let list = Self::authorized_list(&mut state, actor_id, list_id)?;
        if list.aggregate_version() != expected_version {
            return Err(RepositoryError::Conflict);
        }
        state.lists.remove(list_id);
        state.items.retain(|(candidate, _), _| candidate != list_id);
        state.next_positions.remove(list_id);
        Ok(())
    }
}

impl ShoppingItemRepository for InMemoryRepository {
    fn add_item(
        &self,
        actor_id: &UserId,
        list_id: &ListId,
        expected_list_version: AggregateVersion,
        item: ShoppingItem,
    ) -> Result<AggregateVersion, RepositoryError> {
        let mut state = self.state.borrow_mut();

        // Authorize before exposing version, capacity or item existence.
        let stored_list = state.lists.get(list_id).ok_or(RepositoryError::NotFound)?;
        if stored_list.owner_id() != actor_id {
            return Err(RepositoryError::NotFound);
        }
        if stored_list.aggregate_version() != expected_list_version {
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
        let list = state
            .lists
            .get_mut(list_id)
            .ok_or(RepositoryError::NotFound)?;
        list.record_item_mutation()
            .map_err(|_| RepositoryError::VersionExhausted)?;
        let next_version = list.aggregate_version();
        state.items.insert(key, assigned);
        state.next_positions.insert(list_id.clone(), position + 1);
        Ok(next_version)
    }

    fn list_items(
        &self,
        actor_id: &UserId,
        list_id: &ListId,
    ) -> Result<Vec<ShoppingItem>, RepositoryError> {
        let state = self.state.borrow();
        let list = state.lists.get(list_id).ok_or(RepositoryError::NotFound)?;
        if list.owner_id() != actor_id {
            return Err(RepositoryError::NotFound);
        }
        let mut items: Vec<_> = state
            .items
            .iter()
            .filter(|((candidate, _), _)| candidate == list_id)
            .map(|(_, item)| item.clone())
            .collect();
        items.sort_by_key(ShoppingItem::position);
        Ok(items)
    }

    fn set_item_checked(
        &self,
        actor_id: &UserId,
        list_id: &ListId,
        item_id: &ItemId,
        expected_item_version: u64,
        checked: bool,
    ) -> Result<(ShoppingItem, AggregateVersion), RepositoryError> {
        let mut state = self.state.borrow_mut();
        Self::authorized_list(&mut state, actor_id, list_id)?;

        let key = (list_id.clone(), item_id.clone());
        let stored = state.items.get(&key).ok_or(RepositoryError::NotFound)?;
        if stored.version() != expected_item_version {
            return Err(RepositoryError::Conflict);
        }
        let mut updated = stored.clone();
        if updated.set_checked(checked) {
            let list = state
                .lists
                .get_mut(list_id)
                .ok_or(RepositoryError::NotFound)?;
            list.record_item_mutation()
                .map_err(|_| RepositoryError::VersionExhausted)?;
        }
        let list_version = state
            .lists
            .get(list_id)
            .ok_or(RepositoryError::NotFound)?
            .aggregate_version();
        state.items.insert(key, updated.clone());
        Ok((updated, list_version))
    }

    fn update_item_details(
        &self,
        actor_id: &UserId,
        list_id: &ListId,
        item_id: &ItemId,
        expected_item_version: u64,
        details: ItemDetails,
    ) -> Result<(ShoppingItem, AggregateVersion), RepositoryError> {
        let mut state = self.state.borrow_mut();
        Self::authorized_list(&mut state, actor_id, list_id)?;

        let key = (list_id.clone(), item_id.clone());
        let stored = state.items.get(&key).ok_or(RepositoryError::NotFound)?;
        if stored.version() != expected_item_version {
            return Err(RepositoryError::Conflict);
        }
        let mut updated = stored.clone();
        if updated.update_details(details) {
            let list = state
                .lists
                .get_mut(list_id)
                .ok_or(RepositoryError::NotFound)?;
            list.record_item_mutation()
                .map_err(|_| RepositoryError::VersionExhausted)?;
        }
        let list_version = state
            .lists
            .get(list_id)
            .ok_or(RepositoryError::NotFound)?
            .aggregate_version();
        state.items.insert(key, updated.clone());
        Ok((updated, list_version))
    }

    fn delete_item(
        &self,
        actor_id: &UserId,
        list_id: &ListId,
        item_id: &ItemId,
        expected_item_version: u64,
    ) -> Result<AggregateVersion, RepositoryError> {
        let mut state = self.state.borrow_mut();
        Self::authorized_list(&mut state, actor_id, list_id)?;

        let key = (list_id.clone(), item_id.clone());
        let stored = state.items.get(&key).ok_or(RepositoryError::NotFound)?;
        if stored.version() != expected_item_version {
            return Err(RepositoryError::Conflict);
        }
        let list = state
            .lists
            .get_mut(list_id)
            .ok_or(RepositoryError::NotFound)?;
        list.record_item_mutation()
            .map_err(|_| RepositoryError::VersionExhausted)?;
        let next_version = list.aggregate_version();
        state.items.remove(&key);
        Ok(next_version)
    }
}

//! In-memory reference adapter for repository contract tests.
//!
//! This adapter is intentionally not a production persistence implementation. It provides
//! deterministic semantics against which future adapters can run the same contract suite.

use std::cell::RefCell;
use std::collections::HashMap;

use einkaufio_application::{RepositoryError, ShoppingItemRepository, ShoppingListRepository};
use einkaufio_domain::{
    AggregateVersion, ItemId, ItemName, ItemNote, ItemQuantity, ItemUnit, ListId, ListName,
    ShoppingItem, ShoppingList, UserId,
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

    pub fn items_for_list(&self, list_id: &ListId) -> Vec<ShoppingItem> {
        let state = self.state.borrow();
        let mut items: Vec<_> = state.items.iter()
            .filter(|((candidate, _), _)| candidate == list_id)
            .map(|(_, item)| item.clone())
            .collect();
        items.sort_by_key(ShoppingItem::position);
        items
    }

    fn owned_list<'a>(
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

    fn list_personal_for_owner(&self, owner_id: &UserId) -> Result<Vec<ShoppingList>, RepositoryError> {
        let state = self.state.borrow();
        let mut lists: Vec<_> = state.lists.values()
            .filter(|list| list.owner_id() == owner_id)
            .cloned()
            .collect();
        lists.sort_by(|a, b| a.id().as_str().cmp(b.id().as_str()));
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
        let list = Self::owned_list(&mut state, actor_id, list_id)?;
        if list.aggregate_version() != expected_version {
            return Err(RepositoryError::Conflict);
        }
        list.rename(name).map_err(|_| RepositoryError::VersionExhausted)?;
        Ok(list.clone())
    }

    fn delete_personal(
        &self,
        actor_id: &UserId,
        list_id: &ListId,
        expected_version: AggregateVersion,
    ) -> Result<(), RepositoryError> {
        let mut state = self.state.borrow_mut();
        let list = Self::owned_list(&mut state, actor_id, list_id)?;
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
        list_id: &str,
        expected_list_version: u64,
        item: ShoppingItem,
    ) -> Result<u64, RepositoryError> {
        let list_id = ListId::try_new(list_id).map_err(|_| RepositoryError::NotFound)?;
        let mut state = self.state.borrow_mut();

        let active_count = state.items.keys().filter(|(candidate, _)| candidate == &list_id).count();
        let list = Self::owned_list(&mut state, actor_id, &list_id)?;
        if list.version() != expected_list_version {
            return Err(RepositoryError::Conflict);
        }
        if active_count >= ShoppingList::MAX_ACTIVE_ITEMS {
            return Err(RepositoryError::ItemLimitReached);
        }

        let key = (list_id.clone(), item.id().clone());
        if state.items.contains_key(&key) {
            return Err(RepositoryError::AlreadyExists);
        }

        let position = *state.next_positions.get(&list_id).unwrap_or(&0);
        let assigned = ShoppingItem::new(
            item.id().clone(),
            ItemName::try_new(item.name().as_str()).map_err(|_| RepositoryError::Unavailable)?,
            item.quantity().map(|v| ItemQuantity::try_new(v.as_str()).map_err(|_| RepositoryError::Unavailable)).transpose()?,
            item.unit().map(|v| ItemUnit::try_new(v.as_str()).map_err(|_| RepositoryError::Unavailable)).transpose()?,
            item.note().map(|v| ItemNote::try_new(v.as_str()).map_err(|_| RepositoryError::Unavailable)).transpose()?,
            position,
            actor_id.clone(),
        );

        // All checks are complete. The following writes are protected by one exclusive
        // state borrow and therefore represent one atomic adapter operation.
        let list = state.lists.get_mut(&list_id).ok_or(RepositoryError::NotFound)?;
        list.record_item_mutation().map_err(|_| RepositoryError::VersionExhausted)?;
        let next_list_version = list.version();
        state.items.insert(key, assigned);
        state.next_positions.insert(list_id, position.saturating_add(1));
        Ok(next_list_version)
    }

    fn set_item_checked(
        &self,
        actor_id: &UserId,
        list_id: &str,
        item_id: &ItemId,
        expected_item_version: u64,
        checked: bool,
    ) -> Result<(ShoppingItem, u64), RepositoryError> {
        let list_id = ListId::try_new(list_id).map_err(|_| RepositoryError::NotFound)?;
        let mut state = self.state.borrow_mut();

        // Authorization first; only then may the adapter disclose item existence/version.
        Self::owned_list(&mut state, actor_id, &list_id)?;
        let key = (list_id.clone(), item_id.clone());
        let stored = state.items.get(&key).ok_or(RepositoryError::NotFound)?;
        if stored.version() != expected_item_version {
            return Err(RepositoryError::Conflict);
        }

        let mut updated = stored.clone();
        let changed = updated.set_checked(checked);
        if changed {
            let list = state.lists.get_mut(&list_id).ok_or(RepositoryError::NotFound)?;
            list.record_item_mutation().map_err(|_| RepositoryError::VersionExhausted)?;
            state.items.insert(key, updated.clone());
        }
        let list_version = state.lists.get(&list_id).ok_or(RepositoryError::NotFound)?.version();
        Ok((updated, list_version))
    }

    fn delete_item(
        &self,
        actor_id: &UserId,
        list_id: &str,
        item_id: &ItemId,
        expected_item_version: u64,
    ) -> Result<u64, RepositoryError> {
        let list_id = ListId::try_new(list_id).map_err(|_| RepositoryError::NotFound)?;
        let mut state = self.state.borrow_mut();

        Self::owned_list(&mut state, actor_id, &list_id)?;
        let key = (list_id.clone(), item_id.clone());
        let stored = state.items.get(&key).ok_or(RepositoryError::NotFound)?;
        if stored.version() != expected_item_version {
            return Err(RepositoryError::Conflict);
        }
        let position = stored.position();

        let list = state.lists.get_mut(&list_id).ok_or(RepositoryError::NotFound)?;
        list.record_item_mutation().map_err(|_| RepositoryError::VersionExhausted)?;
        let next_list_version = list.version();
        state.items.remove(&key);
        let _ = position;
        Ok(next_list_version)
    }
}

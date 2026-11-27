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
    ListAccessRequest, ListSync, Page, PageCursor, PageRequest, ProfileRepository, RepositoryError,
    ShoppingItemRepository, ShoppingListRepository,
};
use einkaufio_domain::{
    AggregateVersion, DisplayName, ItemDetails, ItemId, ItemOrder, ListAccess, ListId, ListName,
    ShoppingItem, ShoppingList, UserId, UserProfile,
};

use crate::repository_contracts::SharedListFixture;

#[derive(Default)]
struct State {
    lists: HashMap<ListId, ShoppingList>,
    items: HashMap<(ListId, ItemId), ShoppingItem>,
    next_positions: HashMap<ListId, u32>,
    /// Non-ownership access grants (stand-in for household membership until roadmap C2).
    grants: HashMap<(ListId, UserId), ListAccess>,
    /// List version at which each live item last changed (sync, roadmap B5).
    changed_at: HashMap<(ListId, ItemId), AggregateVersion>,
    /// List version at which each removed item was deleted. Retained for the list's lifetime,
    /// so this adapter can always answer with changes instead of a snapshot.
    tombstones: HashMap<(ListId, ItemId), AggregateVersion>,
    profiles: HashMap<UserId, UserProfile>,
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

    fn sorted_items(&self, list_id: &ListId) -> Vec<ShoppingItem> {
        let mut items: Vec<_> = self
            .items
            .iter()
            .filter(|((candidate, _), _)| candidate == list_id)
            .map(|(_, item)| item.clone())
            .collect();
        items.sort_by_key(ShoppingItem::position);
        items
    }

    fn record_change(&mut self, list_id: &ListId, item_id: &ItemId, at: AggregateVersion) {
        let key = (list_id.clone(), item_id.clone());
        self.tombstones.remove(&key);
        self.changed_at.insert(key, at);
    }

    fn list_version(&self, list_id: &ListId) -> Result<AggregateVersion, RepositoryError> {
        self.lists
            .get(list_id)
            .map(ShoppingList::aggregate_version)
            .ok_or(RepositoryError::NotFound)
    }
}

// Opaque cursors: hex-encoded, kind-prefixed and (for items) bound to their list. Clients must
// not parse them; a cursor for another query or list is rejected.
const LIST_CURSOR: &str = "lists:";

fn item_cursor_prefix(list_id: &ListId) -> String {
    format!("items:{}:", list_id.as_str())
}

fn encode_cursor(value: &str) -> Result<PageCursor, RepositoryError> {
    let hex: String = value.bytes().map(|byte| format!("{byte:02x}")).collect();
    PageCursor::try_new(hex).map_err(|_| RepositoryError::Unavailable)
}

fn decode_cursor(cursor: &PageCursor, prefix: &str) -> Result<String, RepositoryError> {
    let hex = cursor.as_str().as_bytes();
    if !hex.len().is_multiple_of(2) {
        return Err(RepositoryError::InvalidCursor);
    }
    let bytes = hex
        .chunks(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|digits| u8::from_str_radix(digits, 16).ok())
        })
        .collect::<Option<Vec<u8>>>()
        .ok_or(RepositoryError::InvalidCursor)?;
    String::from_utf8(bytes)
        .ok()
        .and_then(|text| text.strip_prefix(prefix).map(str::to_owned))
        .ok_or(RepositoryError::InvalidCursor)
}

/// Takes one page from `sorted` entries strictly after the cursor key.
fn paginate<T, K: PartialOrd>(
    sorted: Vec<T>,
    after: Option<K>,
    size: usize,
    key: impl Fn(&T) -> K,
) -> (Vec<T>, bool) {
    let mut remaining: Vec<T> = sorted
        .into_iter()
        .filter(|entry| after.as_ref().is_none_or(|after| key(entry) > *after))
        .collect();
    let more = remaining.len() > size;
    remaining.truncate(size);
    (remaining, more)
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

    async fn list_accessible_page(
        &self,
        principal: &UserId,
        page: &PageRequest,
    ) -> Result<Page<ShoppingList>, RepositoryError> {
        let after = page
            .after
            .as_ref()
            .map(|cursor| decode_cursor(cursor, LIST_CURSOR))
            .transpose()?;
        let lists = self.list_accessible(principal).await?;
        let (items, more) = paginate(lists, after, page.size.as_usize(), |list| {
            list.id().as_str().to_owned()
        });
        let next = match (more, items.last()) {
            (true, Some(last)) => Some(encode_cursor(&format!(
                "{LIST_CURSOR}{}",
                last.id().as_str()
            ))?),
            _ => None,
        };
        Ok(Page { items, next })
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
        state
            .changed_at
            .retain(|(candidate, _), _| candidate != list_id);
        state
            .tombstones
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
        state.record_change(list_id, assigned.id(), next_version);
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
        Ok(state.sorted_items(list_id))
    }

    async fn list_items_page(
        &self,
        access: &ListAccessRequest,
        list_id: &ListId,
        page: &PageRequest,
    ) -> Result<Page<ShoppingItem>, RepositoryError> {
        let mut state = self.state()?;
        // Authorize before interpreting the cursor so that it cannot probe list existence.
        state.authorize(access, list_id)?;
        let prefix = item_cursor_prefix(list_id);
        let after = page
            .after
            .as_ref()
            .map(|cursor| {
                decode_cursor(cursor, &prefix)?
                    .parse::<u32>()
                    .map_err(|_| RepositoryError::InvalidCursor)
            })
            .transpose()?;
        let (items, more) = paginate(
            state.sorted_items(list_id),
            after,
            page.size.as_usize(),
            ShoppingItem::position,
        );
        let next = match (more, items.last()) {
            (true, Some(last)) => Some(encode_cursor(&format!("{prefix}{}", last.position()))?),
            _ => None,
        };
        Ok(Page { items, next })
    }

    async fn reorder_items(
        &self,
        access: &ListAccessRequest,
        list_id: &ListId,
        expected_list_version: AggregateVersion,
        order: &ItemOrder,
    ) -> Result<(Vec<ShoppingItem>, AggregateVersion), RepositoryError> {
        let mut state = self.state()?;
        if state.authorize(access, list_id)?.aggregate_version() != expected_list_version {
            return Err(RepositoryError::Conflict);
        }
        let current = state.sorted_items(list_id);
        // The order must name exactly the current items (ids are unique by construction).
        if current.len() != order.item_ids().len() {
            return Err(RepositoryError::Conflict);
        }
        let mut moved = Vec::new();
        for item in &current {
            let position = order
                .position_of(item.id())
                .ok_or(RepositoryError::Conflict)?;
            let mut updated = item.clone();
            if updated
                .move_to(position)
                .map_err(|_| RepositoryError::VersionExhausted)?
            {
                moved.push(updated);
            }
        }
        if !moved.is_empty() {
            // Every fallible step happens before the first write, keeping the change atomic.
            let list = state.authorize(access, list_id)?;
            list.record_item_mutation()
                .map_err(|_| RepositoryError::VersionExhausted)?;
            let next_version = list.aggregate_version();
            for item in moved {
                state.record_change(list_id, item.id(), next_version);
                state
                    .items
                    .insert((list_id.clone(), item.id().clone()), item);
            }
        }
        Ok((state.sorted_items(list_id), state.list_version(list_id)?))
    }

    async fn sync_list(
        &self,
        access: &ListAccessRequest,
        list_id: &ListId,
        since: Option<AggregateVersion>,
    ) -> Result<ListSync, RepositoryError> {
        let mut state = self.state()?;
        let list = state.authorize(access, list_id)?.clone();
        let since = match since {
            Some(since) if since <= list.aggregate_version() => since,
            // No baseline, or one this list never had: the client must resynchronize.
            _ => {
                let items = state.sorted_items(list_id);
                return Ok(ListSync::Snapshot { list, items });
            }
        };
        let changed = state
            .sorted_items(list_id)
            .into_iter()
            .filter(|item| {
                state
                    .changed_at
                    .get(&(list_id.clone(), item.id().clone()))
                    .is_none_or(|at| *at > since)
            })
            .collect();
        let mut removed: Vec<ItemId> = state
            .tombstones
            .iter()
            .filter(|((candidate, _), at)| candidate == list_id && **at > since)
            .map(|((_, item_id), _)| item_id.clone())
            .collect();
        removed.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        Ok(ListSync::Changes {
            list,
            since,
            changed,
            removed,
        })
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
            let at = state.list_version(list_id)?;
            state.record_change(list_id, item_id, at);
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
            let at = state.list_version(list_id)?;
            state.record_change(list_id, item_id, at);
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
        state.changed_at.remove(&key);
        state.tombstones.insert(key, next_version);
        Ok(next_version)
    }
}

impl ProfileRepository for InMemoryRepository {
    async fn create_profile(&self, profile: UserProfile) -> Result<(), RepositoryError> {
        let mut state = self.state()?;
        if state.profiles.contains_key(profile.user_id()) {
            return Err(RepositoryError::AlreadyExists);
        }
        state.profiles.insert(profile.user_id().clone(), profile);
        Ok(())
    }

    async fn get_profile(&self, principal: &UserId) -> Result<UserProfile, RepositoryError> {
        self.state()?
            .profiles
            .get(principal)
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }

    async fn update_profile(
        &self,
        principal: &UserId,
        expected_version: AggregateVersion,
        display_name: DisplayName,
    ) -> Result<UserProfile, RepositoryError> {
        let mut state = self.state()?;
        let profile = state
            .profiles
            .get_mut(principal)
            .ok_or(RepositoryError::NotFound)?;
        if profile.version() != expected_version {
            return Err(RepositoryError::Conflict);
        }
        profile
            .rename(display_name)
            .map_err(|_| RepositoryError::VersionExhausted)?;
        Ok(profile.clone())
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

    /// Parallel reorders holding the same expected list version: exactly one commits and the
    /// stored positions remain unique and contiguous.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_reorders_with_the_same_expected_version_commit_exactly_once() {
        let repository = Arc::new(InMemoryRepository::new());
        repository.create_personal(list()).await.unwrap();
        let access = ListAccessRequest::for_operation(
            UserId::try_new("concurrent-owner").unwrap(),
            ListOperation::EditItems,
        );
        let list_id = ListId::try_new("concurrent-list").unwrap();
        let mut current = AggregateVersion::initial();
        for index in 0..4 {
            current = repository
                .add_item(&access, &list_id, current, item(index))
                .await
                .unwrap();
        }
        let tasks: Vec<_> = (0..16)
            .map(|rotation| {
                let repository = Arc::clone(&repository);
                let access = access.clone();
                let list_id = list_id.clone();
                tokio::spawn(async move {
                    let ids = (0..4)
                        .map(|offset| {
                            ItemId::try_new(format!(
                                "concurrent-item-{}",
                                (offset + rotation % 3 + 1) % 4
                            ))
                            .unwrap()
                        })
                        .collect();
                    repository
                        .reorder_items(
                            &access,
                            &list_id,
                            current,
                            &ItemOrder::try_new(ids).unwrap(),
                        )
                        .await
                })
            })
            .collect();
        let mut committed = 0;
        for task in tasks {
            match task.await.unwrap() {
                Ok((_, version)) => {
                    committed += 1;
                    assert_eq!(version, current.checked_next().unwrap());
                }
                Err(error) => assert_eq!(error, RepositoryError::Conflict),
            }
        }
        assert_eq!(committed, 1);
        let positions: Vec<_> = repository
            .list_items(&access, &list_id)
            .await
            .unwrap()
            .iter()
            .map(ShoppingItem::position)
            .collect();
        assert_eq!(positions, vec![0, 1, 2, 3]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_profile_updates_with_the_same_version_commit_exactly_once() {
        let repository = Arc::new(InMemoryRepository::new());
        let principal = UserId::try_new("concurrent-owner").unwrap();
        repository
            .create_profile(UserProfile::new(
                principal.clone(),
                DisplayName::try_new("Start").unwrap(),
            ))
            .await
            .unwrap();
        let tasks: Vec<_> = (0..16)
            .map(|index| {
                let repository = Arc::clone(&repository);
                let principal = principal.clone();
                tokio::spawn(async move {
                    repository
                        .update_profile(
                            &principal,
                            AggregateVersion::initial(),
                            DisplayName::try_new(format!("Name {index}")).unwrap(),
                        )
                        .await
                })
            })
            .collect();
        let mut committed = 0;
        for task in tasks {
            match task.await.unwrap() {
                Ok(profile) => {
                    committed += 1;
                    assert_eq!(profile.version().as_u64(), 2);
                }
                Err(error) => assert_eq!(error, RepositoryError::Conflict),
            }
        }
        assert_eq!(committed, 1);
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

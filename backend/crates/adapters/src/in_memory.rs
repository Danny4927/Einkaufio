//! In-memory reference adapter used by the repository contract tests.
//!
//! This adapter is deliberately small and deterministic. It models repository port
//! guarantees without durable persistence. All state sits behind one `Mutex`, so every port
//! operation is atomic and the adapter is `Send + Sync`; it does not model the conditional
//! writes and transaction cancellations of a real database (ADR 014).
//!
//! Access is resolved per ADR 015: personal ownership grants administrator access, and active
//! household membership grants the household role's access to every household-owned list.
//! Because one lock covers lists, households, memberships and invitations, household deletion
//! cascades atomically here; persistent adapters follow the bounded strategy of ADR 017.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard};

use einkaufio_application::{
    HouseholdAccessRequest, HouseholdDetails, HouseholdRepository, HouseholdRepositoryError,
    HouseholdSummary, InvitationRepository, ListAccessRequest, ListSync, Page, PageCursor,
    PageRequest, ProfileRepository, RepositoryError, ShoppingItemRepository,
    ShoppingListRepository,
};
use einkaufio_domain::{
    AggregateVersion, DisplayName, Household, HouseholdId, HouseholdMember, HouseholdName,
    HouseholdRole, Invitation, InvitationId, InvitationTokenHash, ItemDetails, ItemId, ItemOrder,
    ListAccess, ListId, ListName, ShoppingItem, ShoppingList, Timestamp, UserId, UserProfile,
};

#[derive(Default)]
struct State {
    lists: HashMap<ListId, ShoppingList>,
    items: HashMap<(ListId, ItemId), ShoppingItem>,
    next_positions: HashMap<ListId, u32>,
    households: HashMap<HouseholdId, Household>,
    /// Active memberships, owner included. The role is derived from `Household::owner_id`.
    members: HashSet<(HouseholdId, UserId)>,
    /// Invitations keyed by token hash; the raw token is never stored.
    invitations: HashMap<InvitationTokenHash, Invitation>,
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
        let role = list
            .household_id()
            .and_then(|household_id| self.household_role(household_id, principal));
        list.access_for(principal, role)
    }

    /// The principal's role from persisted membership; `None` for non-members.
    fn household_role(
        &self,
        household_id: &HouseholdId,
        principal: &UserId,
    ) -> Option<HouseholdRole> {
        let household = self.households.get(household_id)?;
        self.members
            .contains(&(household_id.clone(), principal.clone()))
            .then(|| household.role_of_member(principal))
    }

    /// Household authorization: non-members get `NotFound`, members lacking the permission
    /// get `Forbidden`.
    fn authorize_household(
        &mut self,
        request: &HouseholdAccessRequest,
        household_id: &HouseholdId,
    ) -> Result<&mut Household, HouseholdRepositoryError> {
        match self.household_role(household_id, request.principal()) {
            None => Err(HouseholdRepositoryError::NotFound),
            Some(role) if !role.allows(request.permission()) => {
                Err(HouseholdRepositoryError::Forbidden)
            }
            Some(_) => self
                .households
                .get_mut(household_id)
                .ok_or(HouseholdRepositoryError::NotFound),
        }
    }

    fn households_of(&self, user: &UserId) -> usize {
        self.members
            .iter()
            .filter(|(_, member)| member == user)
            .count()
    }

    fn member_count(&self, household_id: &HouseholdId) -> usize {
        self.members
            .iter()
            .filter(|(candidate, _)| candidate == household_id)
            .count()
    }

    /// Ends a non-owner membership and advances the household version atomically.
    fn end_membership(
        &mut self,
        household_id: &HouseholdId,
        member: &UserId,
    ) -> Result<Household, HouseholdRepositoryError> {
        let household = self
            .households
            .get_mut(household_id)
            .ok_or(HouseholdRepositoryError::NotFound)?;
        household
            .record_membership_change()
            .map_err(household_version_error)?;
        let updated = household.clone();
        self.members.remove(&(household_id.clone(), member.clone()));
        Ok(updated)
    }

    fn remove_list_data(&mut self, list_id: &ListId) {
        self.lists.remove(list_id);
        self.items.retain(|(candidate, _), _| candidate != list_id);
        self.next_positions.remove(list_id);
        self.changed_at
            .retain(|(candidate, _), _| candidate != list_id);
        self.tombstones
            .retain(|(candidate, _), _| candidate != list_id);
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

    fn household_state(&self) -> Result<MutexGuard<'_, State>, HouseholdRepositoryError> {
        self.state
            .lock()
            .map_err(|_| HouseholdRepositoryError::Unavailable)
    }
}

impl ShoppingListRepository for InMemoryRepository {
    async fn create_personal(&self, list: ShoppingList) -> Result<(), RepositoryError> {
        let mut state = self.state()?;
        // Household lists are created only through the household port, which authorizes
        // membership and enforces the list limit.
        if list.personal_owner().is_none() {
            return Err(RepositoryError::Forbidden);
        }
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
        state.remove_list_data(list_id);
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

    async fn delete_profile(&self, principal: &UserId) -> Result<(), RepositoryError> {
        self.state()?
            .profiles
            .remove(principal)
            .map(|_| ())
            .ok_or(RepositoryError::NotFound)
    }
}

fn household_version_error(
    _: einkaufio_domain::HouseholdMutationError,
) -> HouseholdRepositoryError {
    HouseholdRepositoryError::VersionExhausted
}

impl HouseholdRepository for InMemoryRepository {
    async fn create_household(&self, household: Household) -> Result<(), HouseholdRepositoryError> {
        let mut state = self.household_state()?;
        if state.households.contains_key(household.id()) {
            return Err(HouseholdRepositoryError::AlreadyExists);
        }
        if state.households_of(household.owner_id()) >= Household::MAX_HOUSEHOLDS_PER_USER {
            return Err(HouseholdRepositoryError::HouseholdLimitReached);
        }
        state
            .members
            .insert((household.id().clone(), household.owner_id().clone()));
        state.households.insert(household.id().clone(), household);
        Ok(())
    }

    async fn list_households(
        &self,
        principal: &UserId,
    ) -> Result<Vec<HouseholdSummary>, HouseholdRepositoryError> {
        let state = self.household_state()?;
        let mut summaries: Vec<_> = state
            .members
            .iter()
            .filter(|(_, member)| member == principal)
            .filter_map(|(household_id, _)| state.households.get(household_id))
            .map(|household| HouseholdSummary {
                household: household.clone(),
                role: household.role_of_member(principal),
            })
            .collect();
        summaries.sort_by(|left, right| {
            left.household
                .id()
                .as_str()
                .cmp(right.household.id().as_str())
        });
        Ok(summaries)
    }

    async fn get_household(
        &self,
        access: &HouseholdAccessRequest,
        household_id: &HouseholdId,
    ) -> Result<HouseholdDetails, HouseholdRepositoryError> {
        let mut state = self.household_state()?;
        let household = state.authorize_household(access, household_id)?.clone();
        let mut members: Vec<_> = state
            .members
            .iter()
            .filter(|(candidate, _)| candidate == household_id)
            .map(|(_, user_id)| HouseholdMember {
                user_id: user_id.clone(),
                role: household.role_of_member(user_id),
            })
            .collect();
        members.sort_by(|left, right| left.user_id.as_str().cmp(right.user_id.as_str()));
        Ok(HouseholdDetails { household, members })
    }

    async fn rename_household(
        &self,
        access: &HouseholdAccessRequest,
        household_id: &HouseholdId,
        expected_version: AggregateVersion,
        name: HouseholdName,
    ) -> Result<Household, HouseholdRepositoryError> {
        let mut state = self.household_state()?;
        let household = state.authorize_household(access, household_id)?;
        if household.version() != expected_version {
            return Err(HouseholdRepositoryError::Conflict);
        }
        household.rename(name).map_err(household_version_error)?;
        Ok(household.clone())
    }

    async fn create_household_list(
        &self,
        access: &HouseholdAccessRequest,
        list: ShoppingList,
    ) -> Result<(), HouseholdRepositoryError> {
        let household_id = list
            .household_id()
            .cloned()
            .ok_or(HouseholdRepositoryError::InvalidRequest)?;
        let mut state = self.household_state()?;
        state.authorize_household(access, &household_id)?;
        if state.lists.contains_key(list.id()) {
            return Err(HouseholdRepositoryError::AlreadyExists);
        }
        let owned = state
            .lists
            .values()
            .filter(|candidate| candidate.household_id() == Some(&household_id))
            .count();
        if owned >= Household::MAX_LISTS {
            return Err(HouseholdRepositoryError::ListLimitReached);
        }
        state.lists.insert(list.id().clone(), list);
        Ok(())
    }

    async fn leave_household(
        &self,
        principal: &UserId,
        household_id: &HouseholdId,
    ) -> Result<Household, HouseholdRepositoryError> {
        let mut state = self.household_state()?;
        match state.household_role(household_id, principal) {
            None => Err(HouseholdRepositoryError::NotFound),
            Some(HouseholdRole::Owner) => Err(HouseholdRepositoryError::OwnerMustTransferOwnership),
            Some(HouseholdRole::Member) => state.end_membership(household_id, principal),
        }
    }

    async fn remove_member(
        &self,
        access: &HouseholdAccessRequest,
        household_id: &HouseholdId,
        expected_version: AggregateVersion,
        member: &UserId,
    ) -> Result<Household, HouseholdRepositoryError> {
        let mut state = self.household_state()?;
        if state.authorize_household(access, household_id)?.version() != expected_version {
            return Err(HouseholdRepositoryError::Conflict);
        }
        match state.household_role(household_id, member) {
            None => Err(HouseholdRepositoryError::NotFound),
            Some(HouseholdRole::Owner) => Err(HouseholdRepositoryError::OwnerMustTransferOwnership),
            Some(HouseholdRole::Member) => state.end_membership(household_id, member),
        }
    }

    async fn transfer_ownership(
        &self,
        access: &HouseholdAccessRequest,
        household_id: &HouseholdId,
        expected_version: AggregateVersion,
        new_owner: &UserId,
    ) -> Result<Household, HouseholdRepositoryError> {
        let mut state = self.household_state()?;
        if state.authorize_household(access, household_id)?.version() != expected_version {
            return Err(HouseholdRepositoryError::Conflict);
        }
        if state.household_role(household_id, new_owner).is_none() {
            return Err(HouseholdRepositoryError::NotFound);
        }
        let household = state.authorize_household(access, household_id)?;
        household
            .transfer_ownership(new_owner.clone())
            .map_err(household_version_error)?;
        Ok(household.clone())
    }

    async fn delete_household(
        &self,
        access: &HouseholdAccessRequest,
        household_id: &HouseholdId,
        expected_version: AggregateVersion,
    ) -> Result<(), HouseholdRepositoryError> {
        let mut state = self.household_state()?;
        if state.authorize_household(access, household_id)?.version() != expected_version {
            return Err(HouseholdRepositoryError::Conflict);
        }
        // Single lock: the whole bounded cascade is atomic in this adapter.
        let owned: Vec<ListId> = state
            .lists
            .values()
            .filter(|list| list.household_id() == Some(household_id))
            .map(|list| list.id().clone())
            .collect();
        for list_id in &owned {
            state.remove_list_data(list_id);
        }
        state
            .invitations
            .retain(|_, invitation| invitation.household_id() != household_id);
        state
            .members
            .retain(|(candidate, _)| candidate != household_id);
        state.households.remove(household_id);
        Ok(())
    }
}

impl InvitationRepository for InMemoryRepository {
    async fn create_invitation(
        &self,
        access: &HouseholdAccessRequest,
        invitation: Invitation,
        now: Timestamp,
    ) -> Result<(), HouseholdRepositoryError> {
        let mut state = self.household_state()?;
        state.authorize_household(access, invitation.household_id())?;
        if invitation.created_by() != access.principal() {
            return Err(HouseholdRepositoryError::InvalidRequest);
        }
        let valid = state
            .invitations
            .values()
            .filter(|candidate| {
                candidate.household_id() == invitation.household_id() && candidate.is_valid_at(now)
            })
            .count();
        if valid >= Invitation::MAX_VALID_PER_HOUSEHOLD {
            return Err(HouseholdRepositoryError::InvitationLimitReached);
        }
        if state.invitations.contains_key(invitation.token_hash())
            || state
                .invitations
                .values()
                .any(|candidate| candidate.id() == invitation.id())
        {
            return Err(HouseholdRepositoryError::AlreadyExists);
        }
        state
            .invitations
            .insert(*invitation.token_hash(), invitation);
        Ok(())
    }

    async fn list_invitations(
        &self,
        access: &HouseholdAccessRequest,
        household_id: &HouseholdId,
    ) -> Result<Vec<Invitation>, HouseholdRepositoryError> {
        let mut state = self.household_state()?;
        state.authorize_household(access, household_id)?;
        let mut invitations: Vec<_> = state
            .invitations
            .values()
            .filter(|invitation| invitation.household_id() == household_id)
            .cloned()
            .collect();
        invitations.sort_by(|left, right| left.id().as_str().cmp(right.id().as_str()));
        Ok(invitations)
    }

    async fn revoke_invitation(
        &self,
        access: &HouseholdAccessRequest,
        household_id: &HouseholdId,
        invitation_id: &InvitationId,
    ) -> Result<Invitation, HouseholdRepositoryError> {
        let mut state = self.household_state()?;
        state.authorize_household(access, household_id)?;
        let invitation = state
            .invitations
            .values_mut()
            .find(|invitation| {
                invitation.id() == invitation_id && invitation.household_id() == household_id
            })
            .ok_or(HouseholdRepositoryError::NotFound)?;
        invitation.revoke();
        Ok(invitation.clone())
    }

    async fn accept_invitation(
        &self,
        principal: &UserId,
        token_hash: &InvitationTokenHash,
        now: Timestamp,
    ) -> Result<HouseholdSummary, HouseholdRepositoryError> {
        let mut state = self.household_state()?;
        let invitation = state
            .invitations
            .get(token_hash)
            .ok_or(HouseholdRepositoryError::InvitationUnavailable)?;
        if invitation.check_usable(now).is_err() {
            return Err(HouseholdRepositoryError::InvitationUnavailable);
        }
        let household_id = invitation.household_id().clone();
        let Some(household) = state.households.get(&household_id) else {
            return Err(HouseholdRepositoryError::InvitationUnavailable);
        };
        if state.household_role(&household_id, principal).is_some() {
            return Err(HouseholdRepositoryError::AlreadyMember);
        }
        if state.member_count(&household_id) >= Household::MAX_MEMBERS {
            return Err(HouseholdRepositoryError::MemberLimitReached);
        }
        if state.households_of(principal) >= Household::MAX_HOUSEHOLDS_PER_USER {
            return Err(HouseholdRepositoryError::HouseholdLimitReached);
        }
        // Every fallible step happens before the first write, keeping acceptance atomic.
        let mut household = household.clone();
        household
            .record_membership_change()
            .map_err(household_version_error)?;
        let mut invitation = invitation.clone();
        invitation
            .record_use(now)
            .map_err(|_| HouseholdRepositoryError::InvitationUnavailable)?;
        state.invitations.insert(*token_hash, invitation);
        state
            .members
            .insert((household_id.clone(), principal.clone()));
        state.households.insert(household_id, household.clone());
        Ok(HouseholdSummary {
            household,
            role: HouseholdRole::Member,
        })
    }

    async fn delete_invitations_created_by(
        &self,
        principal: &UserId,
    ) -> Result<(), HouseholdRepositoryError> {
        self.household_state()?
            .invitations
            .retain(|_, invitation| invitation.created_by() != principal);
        Ok(())
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

    use crate::repository_contracts::{
        NOW, at, household_access, household_id, invitation, provision_household, token_hash,
    };
    use einkaufio_domain::HouseholdOperation;

    async fn accept_concurrently(
        repository: &Arc<InMemoryRepository>,
        principals: Vec<String>,
        seeds: Vec<String>,
    ) -> Vec<Result<HouseholdSummary, HouseholdRepositoryError>> {
        let tasks: Vec<_> = principals
            .into_iter()
            .zip(seeds.into_iter().cycle())
            .map(|(principal, seed)| {
                let repository = Arc::clone(repository);
                tokio::spawn(async move {
                    repository
                        .accept_invitation(
                            &UserId::try_new(principal).unwrap(),
                            &token_hash(&seed),
                            at(NOW),
                        )
                        .await
                })
            })
            .collect();
        let mut results = Vec::new();
        for task in tasks {
            results.push(task.await.unwrap());
        }
        results
    }

    async fn issue(repository: &InMemoryRepository, id: &str, seed: &str, max_uses: u32) {
        repository
            .create_invitation(
                &household_access(
                    &UserId::try_new("race-owner").unwrap(),
                    HouseholdOperation::ManageInvitations,
                ),
                invitation(id, "race", "race-owner", seed, max_uses),
                at(NOW),
            )
            .await
            .unwrap();
    }

    async fn stored_invitations(repository: &InMemoryRepository) -> Vec<Invitation> {
        repository
            .list_invitations(
                &household_access(
                    &UserId::try_new("race-owner").unwrap(),
                    HouseholdOperation::ManageInvitations,
                ),
                &household_id("race"),
            )
            .await
            .unwrap()
    }

    async fn member_count(repository: &InMemoryRepository) -> usize {
        repository
            .get_household(
                &household_access(
                    &UserId::try_new("race-owner").unwrap(),
                    HouseholdOperation::View,
                ),
                &household_id("race"),
            )
            .await
            .unwrap()
            .members
            .len()
    }

    /// Many users race to accept one invitation: exactly `max_uses` join, the use count never
    /// exceeds the limit, and every accepted use corresponds to exactly one membership.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_acceptances_never_exceed_the_use_count() {
        let repository = Arc::new(InMemoryRepository::new());
        provision_household(repository.as_ref(), "race", "race-owner", &[]).await;
        issue(&repository, "race-invite", "race-seed", 3).await;

        let results = accept_concurrently(
            &repository,
            (0..16).map(|index| format!("race-user-{index}")).collect(),
            vec!["race-seed".to_owned()],
        )
        .await;
        let accepted = results.iter().filter(|result| result.is_ok()).count();
        assert_eq!(accepted, 3);
        assert!(results.iter().all(|result| matches!(
            result,
            Ok(_) | Err(HouseholdRepositoryError::InvitationUnavailable)
        )));
        assert_eq!(stored_invitations(&repository).await[0].used_count(), 3);
        assert_eq!(member_count(&repository).await, 4);
    }

    /// The same user racing with one token joins once and consumes exactly one use.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_acceptances_by_one_user_consume_one_use() {
        let repository = Arc::new(InMemoryRepository::new());
        provision_household(repository.as_ref(), "race", "race-owner", &[]).await;
        issue(&repository, "race-invite", "race-seed", 10).await;

        let results = accept_concurrently(
            &repository,
            vec!["race-same".to_owned(); 12],
            vec!["race-seed".to_owned()],
        )
        .await;
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert!(
            results.iter().all(|result| matches!(
                result,
                Ok(_) | Err(HouseholdRepositoryError::AlreadyMember)
            ))
        );
        assert_eq!(stored_invitations(&repository).await[0].used_count(), 1);
        assert_eq!(member_count(&repository).await, 2);
    }

    /// Acceptances racing through two invitations cannot overfill the household: the member
    /// limit holds and refused acceptances consume no use.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_acceptances_respect_the_member_limit() {
        let repository = Arc::new(InMemoryRepository::new());
        provision_household(repository.as_ref(), "race", "race-owner", &[]).await;
        issue(&repository, "race-a", "race-a", 10).await;
        issue(&repository, "race-b", "race-b", 10).await;

        let results = accept_concurrently(
            &repository,
            (0..20).map(|index| format!("race-user-{index}")).collect(),
            vec!["race-a".to_owned(), "race-b".to_owned()],
        )
        .await;
        let accepted = results.iter().filter(|result| result.is_ok()).count();
        assert_eq!(accepted, Household::MAX_MEMBERS - 1);
        assert!(results.iter().all(|result| matches!(
            result,
            Ok(_) | Err(HouseholdRepositoryError::MemberLimitReached)
        )));
        let used: u32 = stored_invitations(&repository)
            .await
            .iter()
            .map(Invitation::used_count)
            .sum();
        assert_eq!(used as usize, accepted);
        assert_eq!(member_count(&repository).await, Household::MAX_MEMBERS);
    }

    /// Revocation racing with acceptances: once revoke commits, no later acceptance succeeds,
    /// and the use count always equals the number of successful acceptances.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn revocation_racing_with_acceptance_stays_consistent() {
        let repository = Arc::new(InMemoryRepository::new());
        provision_household(repository.as_ref(), "race", "race-owner", &[]).await;
        issue(&repository, "race-invite", "race-seed", 10).await;
        let revoker = {
            let repository = Arc::clone(&repository);
            tokio::spawn(async move {
                repository
                    .revoke_invitation(
                        &household_access(
                            &UserId::try_new("race-owner").unwrap(),
                            HouseholdOperation::ManageInvitations,
                        ),
                        &household_id("race"),
                        &InvitationId::try_new("race-invite").unwrap(),
                    )
                    .await
            })
        };
        let results = accept_concurrently(
            &repository,
            (0..8).map(|index| format!("race-user-{index}")).collect(),
            vec!["race-seed".to_owned()],
        )
        .await;
        revoker.await.unwrap().unwrap();
        let accepted = results.iter().filter(|result| result.is_ok()).count();
        let stored = stored_invitations(&repository).await;
        assert!(stored[0].is_revoked());
        assert_eq!(stored[0].used_count() as usize, accepted);
        assert_eq!(member_count(&repository).await, accepted + 1);
        assert_eq!(
            repository
                .accept_invitation(
                    &UserId::try_new("race-late").unwrap(),
                    &token_hash("race-seed"),
                    at(NOW)
                )
                .await
                .map(|_| ()),
            Err(HouseholdRepositoryError::InvitationUnavailable)
        );
    }

    /// Concurrent ownership transfers holding the same household version: exactly one wins,
    /// so the household always has exactly one owner.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_ownership_transfers_commit_exactly_once() {
        let repository = Arc::new(InMemoryRepository::new());
        let members: Vec<String> = (0..4).map(|index| format!("race-member-{index}")).collect();
        let member_refs: Vec<&str> = members.iter().map(String::as_str).collect();
        let household =
            provision_household(repository.as_ref(), "race", "race-owner", &member_refs).await;
        let tasks: Vec<_> = members
            .iter()
            .map(|member| {
                let repository = Arc::clone(&repository);
                let member = UserId::try_new(member.clone()).unwrap();
                let expected = household.version();
                tokio::spawn(async move {
                    repository
                        .transfer_ownership(
                            &household_access(
                                &UserId::try_new("race-owner").unwrap(),
                                HouseholdOperation::TransferOwnership,
                            ),
                            &household_id("race"),
                            expected,
                            &member,
                        )
                        .await
                })
            })
            .collect();
        let mut committed = 0;
        for task in tasks {
            match task.await.unwrap() {
                Ok(_) => committed += 1,
                // Losers either saw the moved version or, once the winner committed, no longer
                // hold the owner role (authorization precedes the version check).
                Err(error) => assert!(matches!(
                    error,
                    HouseholdRepositoryError::Conflict | HouseholdRepositoryError::Forbidden
                )),
            }
        }
        assert_eq!(committed, 1);
        let details = repository
            .get_household(
                &household_access(
                    &UserId::try_new("race-member-0").unwrap(),
                    HouseholdOperation::View,
                ),
                &household_id("race"),
            )
            .await
            .unwrap();
        let owners = details
            .members
            .iter()
            .filter(|member| member.role == HouseholdRole::Owner)
            .count();
        assert_eq!(owners, 1);
    }

    #[tokio::test]
    async fn personal_port_refuses_household_lists() {
        let repository = InMemoryRepository::new();
        assert_eq!(
            repository
                .create_personal(ShoppingList::new_household(
                    ListId::try_new("household-list").unwrap(),
                    household_id("race"),
                    ListName::try_new("WG").unwrap(),
                ))
                .await,
            Err(RepositoryError::Forbidden)
        );
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

//! List and item ports (roadmap A5, B1–B3, B5; ADR 015, ADR 019).

use std::collections::{BTreeMap, HashMap};
use std::ops::Bound;

use einkaufio_application::{
    ListAccessRequest, ListSync, Page, PageCursor, PageRequest, RepositoryError,
    ShoppingItemRepository, ShoppingListRepository,
};
use einkaufio_domain::{
    AggregateVersion, HouseholdId, ItemDetails, ItemId, ItemMutationError, ItemOrder, ListAccess,
    ListId, ListName, ListOwner, ListPermission, ShoppingItem, ShoppingList, UserId,
};

use super::DynamoDbRepository;
use super::keys::{self, Key};
use super::records::{self, ACTIVE, COUNT, DELETING, Entry, ListRecord, OWNER_ID, STATE, VERSION};
use super::store::{
    Action, Check, Commit, Condition, Item, MAX_ATTEMPTS, StoreError, keys_of, number, sort_key,
    string,
};

impl From<StoreError> for RepositoryError {
    fn from(_: StoreError) -> Self {
        RepositoryError::Unavailable
    }
}

fn version_exhausted<E>(_: E) -> RepositoryError {
    RepositoryError::VersionExhausted
}

/// How a write re-checks the principal's access in the same transaction (ADR 015).
#[derive(Clone, Debug)]
enum Guard {
    /// Personal list: `ownerId = principal` joins the list metadata condition.
    Personal(UserId),
    /// Household list, contributor permission: the membership record must still exist.
    /// Membership records are removed atomically when membership ends or the household is
    /// marked for deletion.
    Member(Key),
    /// Household list, administrator permission: the principal must still own the active
    /// household.
    Owner { household: Key, principal: UserId },
}

impl Guard {
    fn for_request(list: &ShoppingList, request: &ListAccessRequest) -> Self {
        let principal = request.principal();
        match list.owner() {
            ListOwner::Personal(owner) => Self::Personal(owner.clone()),
            ListOwner::Household(household) => match request.permission() {
                ListPermission::Administer => Self::Owner {
                    household: keys::household_meta(household),
                    principal: principal.clone(),
                },
                ListPermission::Contribute => Self::Member(keys::member(household, principal)),
            },
        }
    }

    /// The list metadata must be unchanged since it was read: same version (every visible
    /// change advances it) and still active.
    fn meta_condition(&self, record: &ListRecord) -> Condition {
        let condition = Condition::present()
            .equals(VERSION, number(record.list.version()))
            .equals(STATE, string(ACTIVE));
        match self {
            Self::Personal(owner) => condition.equals(OWNER_ID, string(owner.as_str())),
            Self::Member(_) | Self::Owner { .. } => condition,
        }
    }

    fn actions(&self) -> Vec<Action> {
        match self {
            Self::Personal(_) => Vec::new(),
            Self::Member(key) => vec![Action::Check {
                key: key.clone(),
                condition: Condition::present(),
            }],
            Self::Owner {
                household,
                principal,
            } => vec![Action::Check {
                key: household.clone(),
                condition: Condition::present()
                    .equals(STATE, string(ACTIVE))
                    .equals(OWNER_ID, string(principal.as_str())),
            }],
        }
    }
}

/// A list record the principal may use for the requested permission.
struct Authorized {
    record: ListRecord,
    guard: Guard,
}

impl Authorized {
    /// Replaces the list metadata with `next`, conditioned on the state that was read and on
    /// the principal's access.
    fn write(&self, next: &ListRecord) -> Vec<Action> {
        let mut actions = vec![Action::Put {
            item: records::encode_list(next),
            condition: self.guard.meta_condition(&self.record),
        }];
        actions.extend(self.guard.actions());
        actions
    }
}

/// A consistent view of a list's items (and tombstones).
struct Content {
    authorized: Authorized,
    /// Ordered by position.
    items: Vec<(ShoppingItem, Entry)>,
    tombstones: Vec<(ItemId, AggregateVersion)>,
}

enum Expected {
    Personal,
    Household(HouseholdId),
}

// Opaque cursors: hex-encoded, kind-prefixed and (for items) bound to their list.
const LIST_CURSOR: &str = "ddb-lists:";

fn item_cursor_prefix(list: &ListId) -> String {
    format!("ddb-items:{}:", list.as_str())
}

fn encode_cursor(value: &str) -> Result<PageCursor, RepositoryError> {
    PageCursor::try_new(keys::hex_encode(value.as_bytes()))
        .map_err(|_| RepositoryError::Unavailable)
}

fn decode_cursor(cursor: &PageCursor, prefix: &str) -> Result<String, RepositoryError> {
    keys::hex_decode(cursor.as_str())
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| text.strip_prefix(prefix).map(str::to_owned))
        .ok_or(RepositoryError::InvalidCursor)
}

/// Accepts a candidate list only if its metadata still grants access through the path the
/// reference came from (references may dangle briefly during bounded deletion).
fn accessible(
    item: &Item,
    principal: &UserId,
    expected: &Expected,
) -> Result<Option<ShoppingList>, StoreError> {
    let record = records::decode_list(item)?;
    if record.deleting {
        return Ok(None);
    }
    let matches = match (record.list.owner(), expected) {
        (ListOwner::Personal(owner), Expected::Personal) => owner == principal,
        (ListOwner::Household(household), Expected::Household(expected)) => household == expected,
        _ => false,
    };
    Ok(matches.then_some(record.list))
}

impl DynamoDbRepository {
    /// The principal's effective access from persisted ownership/membership (ADR 015).
    async fn list_access(
        &self,
        record: &ListRecord,
        principal: &UserId,
    ) -> Result<Option<ListAccess>, StoreError> {
        let role = match record.list.owner() {
            ListOwner::Personal(_) => None,
            ListOwner::Household(household) => {
                let snapshot = self
                    .store
                    .get_atomic(&[
                        keys::household_meta(household),
                        keys::member(household, principal),
                    ])
                    .await?;
                match (&snapshot[0], &snapshot[1]) {
                    (Some(meta), Some(_)) => {
                        let household = records::decode_household(meta)?;
                        (!household.deleting).then(|| household.household.role_of_member(principal))
                    }
                    _ => None,
                }
            }
        };
        Ok(record.list.access_for(principal, role))
    }

    /// Authorizes before disclosing anything: no access or no list → `NotFound`; access too
    /// weak → `Forbidden`. Optionally reads one item record in the same snapshot as the list.
    async fn authorize_list(
        &self,
        request: &ListAccessRequest,
        list_id: &ListId,
        item_id: Option<&ItemId>,
    ) -> Result<(Authorized, Option<Item>), RepositoryError> {
        let meta_key = keys::list_meta(list_id);
        let (meta, item) = match item_id {
            Some(item_id) => {
                let mut snapshot = self
                    .store
                    .get_atomic(&[meta_key, keys::item(list_id, item_id)])
                    .await?;
                let item = snapshot.pop().flatten();
                (snapshot.pop().flatten(), item)
            }
            None => (self.store.get(&meta_key).await?, None),
        };
        let record = records::decode_list(&meta.ok_or(RepositoryError::NotFound)?)?;
        if record.deleting {
            return Err(RepositoryError::NotFound);
        }
        match self.list_access(&record, request.principal()).await? {
            None => Err(RepositoryError::NotFound),
            Some(held) if !held.allows(request.permission()) => Err(RepositoryError::Forbidden),
            Some(_) => Ok((
                Authorized {
                    guard: Guard::for_request(&record.list, request),
                    record,
                },
                item,
            )),
        }
    }

    /// Reads the list's items consistently: the list metadata is read before and after the
    /// partition query, and every item change advances the list version, so equal metadata
    /// proves that no write committed in between.
    async fn read_content(
        &self,
        request: &ListAccessRequest,
        list_id: &ListId,
        with_tombstones: bool,
    ) -> Result<Content, RepositoryError> {
        for attempt in 0..MAX_ATTEMPTS {
            self.store.pause(attempt).await;
            let (authorized, _) = self.authorize_list(request, list_id, None).await?;
            let prefix = (!with_tombstones).then_some(keys::ITEM_PREFIX);
            let rows = self.store.query(&keys::list_pk(list_id), prefix).await?;
            let stable = match self.store.get(&keys::list_meta(list_id)).await? {
                Some(after) => records::decode_list(&after)? == authorized.record,
                None => false,
            };
            if !stable {
                continue;
            }
            let mut details: HashMap<String, &Item> = HashMap::new();
            let mut tombstones = Vec::new();
            for row in &rows {
                match sort_key(row) {
                    Some(sk) if sk.starts_with(keys::ITEM_PREFIX) => {
                        details.insert(records::item_id_of(row)?.as_str().to_owned(), row);
                    }
                    Some(sk) if sk.starts_with(keys::TOMBSTONE_PREFIX) => tombstones
                        .push((records::item_id_of(row)?, records::tombstone_version(row)?)),
                    _ => {}
                }
            }
            // The item index and the item records must describe the same set of items.
            if details.len() != authorized.record.entries.len() {
                return Err(RepositoryError::Unavailable);
            }
            let mut items = authorized
                .record
                .entries
                .iter()
                .map(|(id, entry)| {
                    let row = details.get(id).ok_or(StoreError)?;
                    Ok((records::decode_item(row, entry)?, *entry))
                })
                .collect::<Result<Vec<_>, StoreError>>()?;
            items.sort_by_key(|(item, _)| item.position());
            return Ok(Content {
                authorized,
                items,
                tombstones,
            });
        }
        Err(RepositoryError::Unavailable)
    }

    /// Candidate lists from the principal's personal references and from every household
    /// the principal belongs to, ordered by list id (ADR 015 access patterns).
    async fn list_candidates(
        &self,
        principal: &UserId,
    ) -> Result<BTreeMap<String, (ListId, Expected)>, StoreError> {
        let user = keys::user_pk(principal);
        let mut candidates = BTreeMap::new();
        for row in self.store.query(&user, Some(keys::LIST_PREFIX)).await? {
            let list = records::list_ref_target(&row)?;
            candidates.insert(list.as_str().to_owned(), (list, Expected::Personal));
        }
        for row in self
            .store
            .query(&user, Some(keys::HOUSEHOLD_PREFIX))
            .await?
        {
            let household = records::household_ref_target(&row)?;
            let references = self
                .store
                .query(&keys::household_pk(&household), Some(keys::LIST_PREFIX))
                .await?;
            for reference in references {
                let list = records::list_ref_target(&reference)?;
                candidates.insert(
                    list.as_str().to_owned(),
                    (list, Expected::Household(household.clone())),
                );
            }
        }
        Ok(candidates)
    }

    /// Up to `size` accessible lists after `after` (by id); `true` if more follow.
    async fn accessible_lists(
        &self,
        principal: &UserId,
        after: Option<&str>,
        size: usize,
    ) -> Result<(Vec<ShoppingList>, bool), RepositoryError> {
        let candidates = self.list_candidates(principal).await?;
        let lower = after.map_or(Bound::Unbounded, Bound::Excluded);
        let remaining: Vec<_> = candidates
            .range::<str, _>((lower, Bound::Unbounded))
            .map(|(_, candidate)| candidate)
            .collect();
        let mut page = Vec::new();
        for chunk in remaining.chunks(100) {
            let metas: HashMap<String, Item> = self
                .store
                .batch_get(
                    &chunk
                        .iter()
                        .map(|(list, _)| keys::list_meta(list))
                        .collect::<Vec<_>>(),
                )
                .await?
                .into_iter()
                .filter_map(|item| match item.get(keys::PK) {
                    Some(aws_sdk_dynamodb::types::AttributeValue::S(pk)) => {
                        Some((pk.clone(), item))
                    }
                    _ => None,
                })
                .collect();
            for (list, expected) in chunk {
                let Some(meta) = metas.get(&keys::list_pk(list)) else {
                    continue;
                };
                if let Some(accessible) = accessible(meta, principal, expected)? {
                    if page.len() == size {
                        return Ok((page, true));
                    }
                    page.push(accessible);
                }
            }
        }
        Ok((page, false))
    }

    /// Removes every record of a list partition, metadata last (bounded, idempotent cleanup).
    pub(super) async fn sweep_list(
        &self,
        list_id: &ListId,
        require_deleting: bool,
    ) -> Result<(), StoreError> {
        let rows = self.store.query(&keys::list_pk(list_id), None).await?;
        let children = keys_of(rows.iter().filter(|row| sort_key(row) != Some(keys::META)))?;
        self.store.batch_delete(&children).await?;
        let condition = if require_deleting {
            Condition::none().equals(STATE, string(DELETING))
        } else {
            Condition::none()
        };
        // A cancelled delete means the metadata is already gone: the sweep is complete.
        self.store
            .commit(vec![Action::Delete {
                key: keys::list_meta(list_id),
                condition,
            }])
            .await?;
        Ok(())
    }

    /// Shared flow of check/uncheck and detail updates: authorize, compare the expected item
    /// version before no-op detection, then advance item and list versions together.
    async fn mutate_item(
        &self,
        access: &ListAccessRequest,
        list_id: &ListId,
        item_id: &ItemId,
        expected_item_version: u64,
        change: impl Fn(&mut ShoppingItem) -> Result<bool, ItemMutationError> + Sync,
    ) -> Result<(ShoppingItem, AggregateVersion), RepositoryError> {
        for attempt in 0..MAX_ATTEMPTS {
            self.store.pause(attempt).await;
            let (authorized, row) = self.authorize_list(access, list_id, Some(item_id)).await?;
            let record = &authorized.record;
            let (row, entry) = match (row, record.entries.get(item_id.as_str())) {
                (Some(row), Some(entry)) => (row, *entry),
                (None, None) => return Err(RepositoryError::NotFound),
                _ => return Err(RepositoryError::Unavailable),
            };
            let mut item = records::decode_item(&row, &entry)?;
            if item.version() != expected_item_version {
                return Err(RepositoryError::Conflict);
            }
            if !change(&mut item).map_err(version_exhausted)? {
                return Ok((item, record.list.aggregate_version()));
            }
            let mut next = record.clone();
            next.list
                .record_item_mutation()
                .map_err(version_exhausted)?;
            let at = next.list.aggregate_version();
            next.entries.insert(
                item_id.as_str().to_owned(),
                Entry {
                    position: entry.position,
                    version: AggregateVersion::try_new(item.version())
                        .map_err(|_| RepositoryError::Unavailable)?,
                    changed_at: at,
                },
            );
            let mut actions = authorized.write(&next);
            actions.push(Action::Put {
                item: records::encode_item(list_id, &item),
                condition: Condition::present(),
            });
            if self.store.commit(actions).await? == Commit::Committed {
                return Ok((item, at));
            }
        }
        Err(RepositoryError::Unavailable)
    }
}

impl ShoppingListRepository for DynamoDbRepository {
    async fn create_personal(&self, list: ShoppingList) -> Result<(), RepositoryError> {
        // Household lists are created only through the household port, which authorizes
        // membership and enforces the list limit.
        let owner = list
            .personal_owner()
            .cloned()
            .ok_or(RepositoryError::Forbidden)?;
        for attempt in 0..MAX_ATTEMPTS {
            self.store.pause(attempt).await;
            if self.store.get(&keys::list_meta(list.id())).await?.is_some() {
                return Err(RepositoryError::AlreadyExists);
            }
            let actions = vec![
                Action::Put {
                    item: records::encode_list(&ListRecord::new(list.clone())),
                    condition: Condition::missing(),
                },
                Action::Put {
                    item: records::encode_list_ref(
                        keys::user_list_ref(&owner, list.id()),
                        list.id(),
                    ),
                    condition: Condition::none(),
                },
            ];
            if self.store.commit(actions).await? == Commit::Committed {
                return Ok(());
            }
        }
        Err(RepositoryError::Unavailable)
    }

    async fn list_accessible(
        &self,
        principal: &UserId,
    ) -> Result<Vec<ShoppingList>, RepositoryError> {
        Ok(self.accessible_lists(principal, None, usize::MAX).await?.0)
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
        let (items, more) = self
            .accessible_lists(principal, after.as_deref(), page.size.as_usize())
            .await?;
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
        for attempt in 0..MAX_ATTEMPTS {
            self.store.pause(attempt).await;
            let (authorized, _) = self.authorize_list(access, list_id, None).await?;
            if authorized.record.list.aggregate_version() != expected_version {
                return Err(RepositoryError::Conflict);
            }
            let mut next = authorized.record.clone();
            next.list.rename(name.clone()).map_err(version_exhausted)?;
            if next.list == authorized.record.list {
                return Ok(next.list);
            }
            if self.store.commit(authorized.write(&next)).await? == Commit::Committed {
                return Ok(next.list);
            }
        }
        Err(RepositoryError::Unavailable)
    }

    /// Marks the list as deleting (version check, access check, reference removal and the
    /// household list counter in one transaction), then sweeps its partition. From the mark
    /// on, the list is `NotFound` for everyone. If the sweep fails, the call reports
    /// `Unavailable`; repeating the delete resumes the sweep and then reports `NotFound`,
    /// like any repeated delete.
    async fn delete_list(
        &self,
        access: &ListAccessRequest,
        list_id: &ListId,
        expected_version: AggregateVersion,
    ) -> Result<(), RepositoryError> {
        for attempt in 0..MAX_ATTEMPTS {
            self.store.pause(attempt).await;
            let Some(meta) = self.store.get(&keys::list_meta(list_id)).await? else {
                return Err(RepositoryError::NotFound);
            };
            let current = records::decode_list(&meta)?;
            if current.deleting {
                let administers = self
                    .list_access(&current, access.principal())
                    .await?
                    .is_some_and(|held| held.allows(access.permission()));
                if administers {
                    self.sweep_list(list_id, true).await?;
                }
                return Err(RepositoryError::NotFound);
            }
            let (authorized, _) = self.authorize_list(access, list_id, None).await?;
            if authorized.record.list.aggregate_version() != expected_version {
                return Err(RepositoryError::Conflict);
            }
            let mut marked = authorized.record.clone();
            marked.deleting = true;
            let mut actions = authorized.write(&marked);
            match marked.list.owner() {
                ListOwner::Personal(owner) => actions.push(Action::Delete {
                    key: keys::user_list_ref(owner, list_id),
                    condition: Condition::none(),
                }),
                ListOwner::Household(household) => {
                    actions.push(Action::Delete {
                        key: keys::household_list_ref(household, list_id),
                        condition: Condition::none(),
                    });
                    actions.push(Action::Add {
                        key: keys::household_list_count(household),
                        attribute: COUNT,
                        delta: -1,
                        condition: Condition::none().and(Check::GreaterThan(COUNT, number(0))),
                    });
                }
            }
            if self.store.commit(actions).await? == Commit::Committed {
                self.sweep_list(list_id, true).await?;
                return Ok(());
            }
        }
        Err(RepositoryError::Unavailable)
    }
}

impl ShoppingItemRepository for DynamoDbRepository {
    async fn add_item(
        &self,
        access: &ListAccessRequest,
        list_id: &ListId,
        expected_list_version: AggregateVersion,
        item: ShoppingItem,
    ) -> Result<AggregateVersion, RepositoryError> {
        for attempt in 0..MAX_ATTEMPTS {
            self.store.pause(attempt).await;
            // Authorize before exposing version, capacity or item existence.
            let (authorized, _) = self.authorize_list(access, list_id, None).await?;
            let record = &authorized.record;
            if record.list.aggregate_version() != expected_list_version {
                return Err(RepositoryError::Conflict);
            }
            if record.entries.contains_key(item.id().as_str()) {
                return Err(RepositoryError::AlreadyExists);
            }
            if record.entries.len() >= ShoppingList::MAX_ACTIVE_ITEMS {
                return Err(RepositoryError::ItemLimitReached);
            }
            // The repository assigns the position; a client-supplied one is never trusted.
            let position = record.next_position;
            let stored = item.clone().with_position(position);
            let mut next = record.clone();
            next.list
                .record_item_mutation()
                .map_err(version_exhausted)?;
            next.next_position = position
                .checked_add(1)
                .ok_or(RepositoryError::VersionExhausted)?;
            let at = next.list.aggregate_version();
            next.entries.insert(
                stored.id().as_str().to_owned(),
                Entry {
                    position,
                    version: AggregateVersion::try_new(stored.version())
                        .map_err(|_| RepositoryError::Unavailable)?,
                    changed_at: at,
                },
            );
            let mut actions = authorized.write(&next);
            actions.push(Action::Put {
                item: records::encode_item(list_id, &stored),
                condition: Condition::missing(),
            });
            // Re-adding a deleted item id replaces its tombstone.
            actions.push(Action::Delete {
                key: keys::tombstone(list_id, stored.id()),
                condition: Condition::none(),
            });
            if self.store.commit(actions).await? == Commit::Committed {
                return Ok(at);
            }
        }
        Err(RepositoryError::Unavailable)
    }

    async fn list_items(
        &self,
        access: &ListAccessRequest,
        list_id: &ListId,
    ) -> Result<Vec<ShoppingItem>, RepositoryError> {
        let content = self.read_content(access, list_id, false).await?;
        Ok(content.items.into_iter().map(|(item, _)| item).collect())
    }

    async fn list_items_page(
        &self,
        access: &ListAccessRequest,
        list_id: &ListId,
        page: &PageRequest,
    ) -> Result<Page<ShoppingItem>, RepositoryError> {
        // Authorize (inside read_content) before interpreting the cursor, so that a cursor
        // cannot probe list existence.
        let content = self.read_content(access, list_id, false).await?;
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
        let mut remaining: Vec<ShoppingItem> = content
            .items
            .into_iter()
            .map(|(item, _)| item)
            .filter(|item| after.is_none_or(|after| item.position() > after))
            .collect();
        let more = remaining.len() > page.size.as_usize();
        remaining.truncate(page.size.as_usize());
        let next = match (more, remaining.last()) {
            (true, Some(last)) => Some(encode_cursor(&format!("{prefix}{}", last.position()))?),
            _ => None,
        };
        Ok(Page {
            items: remaining,
            next,
        })
    }

    /// One write of the list metadata: positions and item versions live in its item index,
    /// so even a reorder that moves all 100 items stays within one transaction (ADR 019).
    async fn reorder_items(
        &self,
        access: &ListAccessRequest,
        list_id: &ListId,
        expected_list_version: AggregateVersion,
        order: &ItemOrder,
    ) -> Result<(Vec<ShoppingItem>, AggregateVersion), RepositoryError> {
        for attempt in 0..MAX_ATTEMPTS {
            self.store.pause(attempt).await;
            let content = self.read_content(access, list_id, false).await?;
            let record = &content.authorized.record;
            if record.list.aggregate_version() != expected_list_version {
                return Err(RepositoryError::Conflict);
            }
            // The order must name exactly the current items (ids are unique by construction).
            if content.items.len() != order.item_ids().len() {
                return Err(RepositoryError::Conflict);
            }
            let mut reordered = Vec::with_capacity(content.items.len());
            let mut moved = Vec::new();
            for (item, _) in &content.items {
                let position = order
                    .position_of(item.id())
                    .ok_or(RepositoryError::Conflict)?;
                let mut updated = item.clone();
                if updated.move_to(position).map_err(version_exhausted)? {
                    moved.push(updated.id().clone());
                }
                reordered.push(updated);
            }
            reordered.sort_by_key(ShoppingItem::position);
            if moved.is_empty() {
                return Ok((reordered, record.list.aggregate_version()));
            }
            let mut next = record.clone();
            next.list
                .record_item_mutation()
                .map_err(version_exhausted)?;
            let at = next.list.aggregate_version();
            for item in reordered.iter().filter(|item| moved.contains(item.id())) {
                let entry = next
                    .entries
                    .get_mut(item.id().as_str())
                    .ok_or(RepositoryError::Unavailable)?;
                entry.position = item.position();
                entry.version = AggregateVersion::try_new(item.version())
                    .map_err(|_| RepositoryError::Unavailable)?;
                entry.changed_at = at;
            }
            if self.store.commit(content.authorized.write(&next)).await? == Commit::Committed {
                return Ok((reordered, at));
            }
        }
        Err(RepositoryError::Unavailable)
    }

    /// Tombstones are kept for the list's lifetime, so changes can always be derived for a
    /// baseline the list actually had; they are removed with the list.
    async fn sync_list(
        &self,
        access: &ListAccessRequest,
        list_id: &ListId,
        since: Option<AggregateVersion>,
    ) -> Result<ListSync, RepositoryError> {
        let content = self.read_content(access, list_id, true).await?;
        let list = content.authorized.record.list;
        let since = match since {
            Some(since) if since <= list.aggregate_version() => since,
            // No baseline, or one this list never had: the client must resynchronize.
            _ => {
                let items = content.items.into_iter().map(|(item, _)| item).collect();
                return Ok(ListSync::Snapshot { list, items });
            }
        };
        let changed = content
            .items
            .into_iter()
            .filter(|(_, entry)| entry.changed_at > since)
            .map(|(item, _)| item)
            .collect();
        let mut removed: Vec<ItemId> = content
            .tombstones
            .into_iter()
            .filter(|(_, at)| *at > since)
            .map(|(item_id, _)| item_id)
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
        self.mutate_item(access, list_id, item_id, expected_item_version, |item| {
            item.set_checked(checked)
        })
        .await
    }

    async fn update_item_details(
        &self,
        access: &ListAccessRequest,
        list_id: &ListId,
        item_id: &ItemId,
        expected_item_version: u64,
        details: ItemDetails,
    ) -> Result<(ShoppingItem, AggregateVersion), RepositoryError> {
        self.mutate_item(access, list_id, item_id, expected_item_version, |item| {
            item.update_details(details.clone())
        })
        .await
    }

    async fn delete_item(
        &self,
        access: &ListAccessRequest,
        list_id: &ListId,
        item_id: &ItemId,
        expected_item_version: u64,
    ) -> Result<AggregateVersion, RepositoryError> {
        for attempt in 0..MAX_ATTEMPTS {
            self.store.pause(attempt).await;
            let (authorized, row) = self.authorize_list(access, list_id, Some(item_id)).await?;
            let record = &authorized.record;
            let entry = match (row, record.entries.get(item_id.as_str())) {
                (Some(_), Some(entry)) => *entry,
                (None, None) => return Err(RepositoryError::NotFound),
                _ => return Err(RepositoryError::Unavailable),
            };
            if entry.version.as_u64() != expected_item_version {
                return Err(RepositoryError::Conflict);
            }
            let mut next = record.clone();
            next.list
                .record_item_mutation()
                .map_err(version_exhausted)?;
            let at = next.list.aggregate_version();
            next.entries.remove(item_id.as_str());
            let mut actions = authorized.write(&next);
            actions.push(Action::Delete {
                key: keys::item(list_id, item_id),
                condition: Condition::present(),
            });
            actions.push(Action::Put {
                item: records::encode_tombstone(list_id, item_id, at),
                condition: Condition::none(),
            });
            if self.store.commit(actions).await? == Commit::Committed {
                return Ok(at);
            }
        }
        Err(RepositoryError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursors_are_opaque_and_bound_to_their_query() {
        let list = ListId::try_new("list-1").unwrap();
        let cursor = encode_cursor(&format!("{}7", item_cursor_prefix(&list))).unwrap();
        assert!(!cursor.as_str().contains("list-1"));
        assert_eq!(
            decode_cursor(&cursor, &item_cursor_prefix(&list)).unwrap(),
            "7"
        );
        let other = ListId::try_new("list-2").unwrap();
        assert_eq!(
            decode_cursor(&cursor, &item_cursor_prefix(&other)),
            Err(RepositoryError::InvalidCursor)
        );
        assert_eq!(
            decode_cursor(&cursor, LIST_CURSOR),
            Err(RepositoryError::InvalidCursor)
        );
        assert_eq!(
            decode_cursor(&PageCursor::try_new("zz").unwrap(), LIST_CURSOR),
            Err(RepositoryError::InvalidCursor)
        );
    }

    #[test]
    fn guards_follow_the_required_permission() {
        let household = HouseholdId::try_new("h").unwrap();
        let list = ShoppingList::new_household(
            ListId::try_new("l").unwrap(),
            household.clone(),
            ListName::try_new("WG").unwrap(),
        );
        let principal = UserId::try_new("p").unwrap();
        let contribute = ListAccessRequest::new(principal.clone(), ListPermission::Contribute);
        let administer = ListAccessRequest::new(principal.clone(), ListPermission::Administer);
        assert!(matches!(
            Guard::for_request(&list, &contribute),
            Guard::Member(key) if key == keys::member(&household, &principal)
        ));
        assert!(matches!(
            Guard::for_request(&list, &administer),
            Guard::Owner { household: key, .. } if key == keys::household_meta(&household)
        ));
        let personal = ShoppingList::new_personal(
            ListId::try_new("l").unwrap(),
            principal.clone(),
            ListName::try_new("Privat").unwrap(),
        );
        let guard = Guard::for_request(&personal, &administer);
        assert!(
            guard.actions().is_empty(),
            "owner condition joins the metadata write"
        );
    }
}

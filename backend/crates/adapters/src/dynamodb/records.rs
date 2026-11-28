//! Record codecs: persisted attribute maps ⇄ domain values.
//!
//! Decoding validates every value through the domain constructors. A record that does not
//! decode (missing attribute, wrong type, invalid value) is treated as unavailable data and
//! fails closed; it is never repaired or defaulted.

use std::collections::{BTreeMap, HashMap};

use aws_sdk_dynamodb::types::AttributeValue;
use einkaufio_domain::{
    AggregateVersion, DisplayName, Household, HouseholdId, HouseholdName, Invitation, InvitationId,
    InvitationMaxUses, InvitationTokenHash, ItemDetails, ItemId, ItemName, ItemNote, ItemQuantity,
    ItemUnit, ListId, ListName, ListOwner, ShoppingItem, ShoppingList, Timestamp, UserId,
    UserProfile,
};

use super::keys::{self, Key, PK, SK};
use super::store::{Item, StoreError, number, string};

// Attribute names. Kept short but readable; documented in docs/data-model.md.
pub(super) const ENTITY: &str = "entity";
pub(super) const VERSION: &str = "version";
pub(super) const STATE: &str = "state";
pub(super) const OWNER_ID: &str = "ownerId";
pub(super) const COUNT: &str = "count";
pub(super) const REV: &str = "rev";
pub(super) const USED_COUNT: &str = "usedCount";
pub(super) const REVOKED: &str = "revoked";
pub(super) const CREATED_BY: &str = "createdBy";
const LIST_ID: &str = "listId";
const OWNER_TYPE: &str = "ownerType";
const HOUSEHOLD_ID: &str = "householdId";
const NAME: &str = "name";
const NEXT_POSITION: &str = "nextPosition";
const ITEMS: &str = "items";
const ITEM_ID: &str = "itemId";
const QUANTITY: &str = "quantity";
const UNIT: &str = "unit";
const NOTE: &str = "note";
const CHECKED: &str = "checked";
const DELETED_AT: &str = "deletedAt";
const MEMBER_COUNT: &str = "memberCount";
const USER_ID: &str = "userId";
const INVITATION_ID: &str = "invitationId";
pub(super) const TOKEN_HASH: &str = "tokenHash";
const EXPIRES_AT: &str = "expiresAt";
const MAX_USES: &str = "maxUses";
const DISPLAY_NAME: &str = "displayName";
// Keys of the per-item entries in the list metadata `items` map.
const ENTRY_POSITION: &str = "p";
const ENTRY_VERSION: &str = "v";
const ENTRY_CHANGED_AT: &str = "c";

pub(super) const ACTIVE: &str = "ACTIVE";
pub(super) const DELETING: &str = "DELETING";
const PERSONAL: &str = "PERSONAL";
const HOUSEHOLD: &str = "HOUSEHOLD";

fn base(key: Key, entity: &str) -> Item {
    HashMap::from([
        (PK.to_owned(), string(key.pk)),
        (SK.to_owned(), string(key.sk)),
        (ENTITY.to_owned(), string(entity)),
    ])
}

fn text<'a>(item: &'a Item, name: &str) -> Result<&'a str, StoreError> {
    match item.get(name) {
        Some(AttributeValue::S(value)) => Ok(value),
        _ => Err(StoreError),
    }
}

fn optional_text<'a>(item: &'a Item, name: &str) -> Result<Option<&'a str>, StoreError> {
    match item.get(name) {
        None => Ok(None),
        Some(AttributeValue::S(value)) => Ok(Some(value)),
        Some(_) => Err(StoreError),
    }
}

fn unsigned(item: &Item, name: &str) -> Result<u64, StoreError> {
    match item.get(name) {
        Some(AttributeValue::N(value)) => value.parse().map_err(|_| StoreError),
        _ => Err(StoreError),
    }
}

fn small(item: &Item, name: &str) -> Result<u32, StoreError> {
    u32::try_from(unsigned(item, name)?).map_err(|_| StoreError)
}

fn flag(item: &Item, name: &str) -> Result<bool, StoreError> {
    match item.get(name) {
        Some(AttributeValue::Bool(value)) => Ok(*value),
        _ => Err(StoreError),
    }
}

fn aggregate_version(item: &Item, name: &str) -> Result<AggregateVersion, StoreError> {
    AggregateVersion::try_new(unsigned(item, name)?).map_err(|_| StoreError)
}

fn deleting(item: &Item) -> Result<bool, StoreError> {
    match text(item, STATE)? {
        ACTIVE => Ok(false),
        DELETING => Ok(true),
        _ => Err(StoreError),
    }
}

fn state(deleting: bool) -> AttributeValue {
    string(if deleting { DELETING } else { ACTIVE })
}

fn user_id(value: &str) -> Result<UserId, StoreError> {
    UserId::try_new(value).map_err(|_| StoreError)
}

fn household_id(value: &str) -> Result<HouseholdId, StoreError> {
    HouseholdId::try_new(value).map_err(|_| StoreError)
}

fn list_id(value: &str) -> Result<ListId, StoreError> {
    ListId::try_new(value).map_err(|_| StoreError)
}

/// Per-item ordering and versioning state, kept in the list metadata record so that a
/// reorder of up to 100 items is a single-record write (ADR 019).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Entry {
    pub(super) position: u32,
    pub(super) version: AggregateVersion,
    /// List version at which the item last changed (sync, roadmap B5).
    pub(super) changed_at: AggregateVersion,
}

/// `LIST#l / META`: the list aggregate plus its item index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ListRecord {
    pub(super) list: ShoppingList,
    pub(super) deleting: bool,
    pub(super) next_position: u32,
    /// Keyed by item id. Bounded by `ShoppingList::MAX_ACTIVE_ITEMS`.
    pub(super) entries: BTreeMap<String, Entry>,
}

impl ListRecord {
    pub(super) fn new(list: ShoppingList) -> Self {
        Self {
            list,
            deleting: false,
            next_position: 0,
            entries: BTreeMap::new(),
        }
    }
}

pub(super) fn encode_list(record: &ListRecord) -> Item {
    let list = &record.list;
    let mut item = base(keys::list_meta(list.id()), "LIST");
    item.insert(LIST_ID.into(), string(list.id().as_str()));
    match list.owner() {
        ListOwner::Personal(owner) => {
            item.insert(OWNER_TYPE.into(), string(PERSONAL));
            item.insert(OWNER_ID.into(), string(owner.as_str()));
        }
        ListOwner::Household(household) => {
            item.insert(OWNER_TYPE.into(), string(HOUSEHOLD));
            item.insert(HOUSEHOLD_ID.into(), string(household.as_str()));
        }
    }
    item.insert(NAME.into(), string(list.name().as_str()));
    item.insert(VERSION.into(), number(list.version()));
    item.insert(STATE.into(), state(record.deleting));
    item.insert(NEXT_POSITION.into(), number(record.next_position));
    let entries = record
        .entries
        .iter()
        .map(|(id, entry)| {
            let value = HashMap::from([
                (ENTRY_POSITION.to_owned(), number(entry.position)),
                (ENTRY_VERSION.to_owned(), number(entry.version.as_u64())),
                (
                    ENTRY_CHANGED_AT.to_owned(),
                    number(entry.changed_at.as_u64()),
                ),
            ]);
            (id.clone(), AttributeValue::M(value))
        })
        .collect();
    item.insert(ITEMS.into(), AttributeValue::M(entries));
    item
}

pub(super) fn decode_list(item: &Item) -> Result<ListRecord, StoreError> {
    let id = list_id(text(item, LIST_ID)?)?;
    let owner = match text(item, OWNER_TYPE)? {
        PERSONAL => ListOwner::Personal(user_id(text(item, OWNER_ID)?)?),
        HOUSEHOLD => ListOwner::Household(household_id(text(item, HOUSEHOLD_ID)?)?),
        _ => return Err(StoreError),
    };
    let name = ListName::try_new(text(item, NAME)?).map_err(|_| StoreError)?;
    let list = ShoppingList::restore(id, owner, name, aggregate_version(item, VERSION)?);
    let entries = match item.get(ITEMS) {
        Some(AttributeValue::M(map)) => map
            .iter()
            .map(|(id, value)| match value {
                AttributeValue::M(entry) => Ok((
                    id.clone(),
                    Entry {
                        position: small(entry, ENTRY_POSITION)?,
                        version: aggregate_version(entry, ENTRY_VERSION)?,
                        changed_at: aggregate_version(entry, ENTRY_CHANGED_AT)?,
                    },
                )),
                _ => Err(StoreError),
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?,
        _ => return Err(StoreError),
    };
    Ok(ListRecord {
        list,
        deleting: deleting(item)?,
        next_position: small(item, NEXT_POSITION)?,
        entries,
    })
}

/// `LIST#l / ITEM#i`: the item's details; position and version live in the list entry.
pub(super) fn encode_item(list: &ListId, item: &ShoppingItem) -> Item {
    let mut record = base(keys::item(list, item.id()), "ITEM");
    record.insert(ITEM_ID.into(), string(item.id().as_str()));
    record.insert(NAME.into(), string(item.name().as_str()));
    if let Some(quantity) = item.quantity() {
        record.insert(QUANTITY.into(), string(quantity.as_str()));
    }
    if let Some(unit) = item.unit() {
        record.insert(UNIT.into(), string(unit.as_str()));
    }
    if let Some(note) = item.note() {
        record.insert(NOTE.into(), string(note.as_str()));
    }
    record.insert(CHECKED.into(), AttributeValue::Bool(item.checked()));
    record.insert(CREATED_BY.into(), string(item.created_by().as_str()));
    record
}

/// The item id of an item or tombstone record.
pub(super) fn item_id_of(record: &Item) -> Result<ItemId, StoreError> {
    ItemId::try_new(text(record, ITEM_ID)?).map_err(|_| StoreError)
}

pub(super) fn decode_item(record: &Item, entry: &Entry) -> Result<ShoppingItem, StoreError> {
    let optional = |name| optional_text(record, name).map(|value| value.map(str::to_owned));
    let details = ItemDetails {
        name: ItemName::try_new(text(record, NAME)?).map_err(|_| StoreError)?,
        quantity: optional(QUANTITY)?
            .map(ItemQuantity::try_new)
            .transpose()
            .map_err(|_| StoreError)?,
        unit: optional(UNIT)?
            .map(ItemUnit::try_new)
            .transpose()
            .map_err(|_| StoreError)?,
        note: optional(NOTE)?
            .map(ItemNote::try_new)
            .transpose()
            .map_err(|_| StoreError)?,
    };
    Ok(ShoppingItem::restore(
        item_id_of(record)?,
        details,
        flag(record, CHECKED)?,
        entry.position,
        entry.version,
        user_id(text(record, CREATED_BY)?)?,
    ))
}

/// `LIST#l / TOMB#i`: an item deleted at list version `deleted_at` (sync, roadmap B5).
pub(super) fn encode_tombstone(list: &ListId, item: &ItemId, deleted_at: AggregateVersion) -> Item {
    let mut record = base(keys::tombstone(list, item), "TOMBSTONE");
    record.insert(ITEM_ID.into(), string(item.as_str()));
    record.insert(DELETED_AT.into(), number(deleted_at.as_u64()));
    record
}

pub(super) fn tombstone_version(record: &Item) -> Result<AggregateVersion, StoreError> {
    aggregate_version(record, DELETED_AT)
}

/// `HOUSEHOLD#h / META`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HouseholdRecord {
    pub(super) household: Household,
    pub(super) deleting: bool,
    /// Active members, owner included. Changes together with the household version.
    pub(super) member_count: u32,
}

pub(super) fn encode_household(record: &HouseholdRecord) -> Item {
    let household = &record.household;
    let mut item = base(keys::household_meta(household.id()), "HOUSEHOLD");
    item.insert(HOUSEHOLD_ID.into(), string(household.id().as_str()));
    item.insert(NAME.into(), string(household.name().as_str()));
    item.insert(OWNER_ID.into(), string(household.owner_id().as_str()));
    item.insert(VERSION.into(), number(household.version().as_u64()));
    item.insert(MEMBER_COUNT.into(), number(record.member_count));
    item.insert(STATE.into(), state(record.deleting));
    item
}

pub(super) fn decode_household(item: &Item) -> Result<HouseholdRecord, StoreError> {
    let household = Household::restore(
        household_id(text(item, HOUSEHOLD_ID)?)?,
        HouseholdName::try_new(text(item, NAME)?).map_err(|_| StoreError)?,
        user_id(text(item, OWNER_ID)?)?,
        aggregate_version(item, VERSION)?,
    );
    Ok(HouseholdRecord {
        household,
        deleting: deleting(item)?,
        member_count: small(item, MEMBER_COUNT)?,
    })
}

/// `HOUSEHOLD#h / MEMBER#u`: an active membership (the role derives from the owner id).
pub(super) fn encode_member(household: &HouseholdId, user: &UserId) -> Item {
    let mut item = base(keys::member(household, user), "MEMBER");
    item.insert(HOUSEHOLD_ID.into(), string(household.as_str()));
    item.insert(USER_ID.into(), string(user.as_str()));
    item
}

pub(super) fn member_user(item: &Item) -> Result<UserId, StoreError> {
    user_id(text(item, USER_ID)?)
}

pub(super) fn encode_list_ref(key: Key, list: &ListId) -> Item {
    let mut item = base(key, "LIST_REF");
    item.insert(LIST_ID.into(), string(list.as_str()));
    item
}

pub(super) fn list_ref_target(item: &Item) -> Result<ListId, StoreError> {
    list_id(text(item, LIST_ID)?)
}

pub(super) fn encode_household_ref(user: &UserId, household: &HouseholdId) -> Item {
    let mut item = base(keys::user_household_ref(user, household), "HOUSEHOLD_REF");
    item.insert(HOUSEHOLD_ID.into(), string(household.as_str()));
    item
}

pub(super) fn household_ref_target(item: &Item) -> Result<HouseholdId, StoreError> {
    household_id(text(item, HOUSEHOLD_ID)?)
}

/// Value of a counter record (`count`); an absent record counts as zero.
pub(super) fn counter(item: Option<&Item>) -> Result<u64, StoreError> {
    item.map_or(Ok(0), |item| unsigned(item, COUNT))
}

/// Value of a revision record (`rev`); `None` when the record does not exist yet.
pub(super) fn revision(item: Option<&Item>) -> Result<Option<u64>, StoreError> {
    item.map(|item| unsigned(item, REV)).transpose()
}

pub(super) fn encode_revision(key: Key, rev: u64) -> Item {
    let mut item = base(key, "REVISION");
    item.insert(REV.into(), number(rev));
    item
}

/// Pointer from a household (`INVITE#id`) or a creator (`INVITE#hash`) to an invitation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct InvitationRef {
    pub(super) household: HouseholdId,
    pub(super) invitation: InvitationId,
    pub(super) hash: InvitationTokenHash,
}

pub(super) fn encode_invitation_ref(key: Key, reference: &InvitationRef) -> Item {
    let mut item = base(key, "INVITE_REF");
    item.insert(HOUSEHOLD_ID.into(), string(reference.household.as_str()));
    item.insert(INVITATION_ID.into(), string(reference.invitation.as_str()));
    item.insert(TOKEN_HASH.into(), string(keys::hash_hex(&reference.hash)));
    item
}

pub(super) fn decode_invitation_ref(item: &Item) -> Result<InvitationRef, StoreError> {
    Ok(InvitationRef {
        household: household_id(text(item, HOUSEHOLD_ID)?)?,
        invitation: InvitationId::try_new(text(item, INVITATION_ID)?).map_err(|_| StoreError)?,
        hash: keys::parse_hash_hex(text(item, TOKEN_HASH)?).ok_or(StoreError)?,
    })
}

/// `INVITE#<sha256> / META`. The key is the token hash; the raw token is never stored.
pub(super) fn encode_invitation(invitation: &Invitation) -> Item {
    let mut item = base(keys::invitation(invitation.token_hash()), "INVITATION");
    item.insert(INVITATION_ID.into(), string(invitation.id().as_str()));
    item.insert(
        HOUSEHOLD_ID.into(),
        string(invitation.household_id().as_str()),
    );
    item.insert(CREATED_BY.into(), string(invitation.created_by().as_str()));
    item.insert(
        EXPIRES_AT.into(),
        number(invitation.expires_at().as_unix_seconds()),
    );
    item.insert(MAX_USES.into(), number(invitation.max_uses().get()));
    item.insert(USED_COUNT.into(), number(invitation.used_count()));
    item.insert(
        REVOKED.into(),
        AttributeValue::Bool(invitation.is_revoked()),
    );
    item
}

pub(super) fn decode_invitation(
    item: &Item,
    hash: &InvitationTokenHash,
) -> Result<Invitation, StoreError> {
    Invitation::restore(
        InvitationId::try_new(text(item, INVITATION_ID)?).map_err(|_| StoreError)?,
        household_id(text(item, HOUSEHOLD_ID)?)?,
        user_id(text(item, CREATED_BY)?)?,
        *hash,
        Timestamp::from_unix_seconds(unsigned(item, EXPIRES_AT)?),
        InvitationMaxUses::try_new(small(item, MAX_USES)?).map_err(|_| StoreError)?,
        small(item, USED_COUNT)?,
        flag(item, REVOKED)?,
    )
    .map_err(|_| StoreError)
}

/// `USER#u / PROFILE`.
pub(super) fn encode_profile(profile: &UserProfile) -> Item {
    let mut item = base(keys::profile(profile.user_id()), "PROFILE");
    item.insert(USER_ID.into(), string(profile.user_id().as_str()));
    item.insert(DISPLAY_NAME.into(), string(profile.display_name().as_str()));
    item.insert(VERSION.into(), number(profile.version().as_u64()));
    item
}

pub(super) fn decode_profile(item: &Item) -> Result<UserProfile, StoreError> {
    Ok(UserProfile::restore(
        user_id(text(item, USER_ID)?)?,
        DisplayName::try_new(text(item, DISPLAY_NAME)?).map_err(|_| StoreError)?,
        aggregate_version(item, VERSION)?,
    ))
}

#[cfg(test)]
mod tests {
    use einkaufio_domain::InvitationLifetime;

    use super::*;

    fn list(owner: ListOwner) -> ShoppingList {
        ShoppingList::restore(
            ListId::try_new("list-1").unwrap(),
            owner,
            ListName::try_new("Wocheneinkauf").unwrap(),
            AggregateVersion::try_new(4).unwrap(),
        )
    }

    #[test]
    fn list_records_round_trip_for_both_owner_kinds() {
        for owner in [
            ListOwner::Personal(UserId::try_new("owner").unwrap()),
            ListOwner::Household(HouseholdId::try_new("household").unwrap()),
        ] {
            let mut record = ListRecord::new(list(owner));
            record.next_position = 3;
            record.entries.insert(
                "item-1".into(),
                Entry {
                    position: 2,
                    version: AggregateVersion::try_new(5).unwrap(),
                    changed_at: AggregateVersion::try_new(4).unwrap(),
                },
            );
            let encoded = encode_list(&record);
            assert_eq!(encoded.get(PK), Some(&string("LIST#list-1")));
            assert_eq!(decode_list(&encoded).unwrap(), record);
        }
    }

    #[test]
    fn corrupted_records_fail_closed() {
        let mut encoded = encode_list(&ListRecord::new(list(ListOwner::Personal(
            UserId::try_new("owner").unwrap(),
        ))));
        encoded.insert(VERSION.into(), number(0_u64));
        assert_eq!(decode_list(&encoded), Err(StoreError));
        encoded.insert(VERSION.into(), string("1"));
        assert_eq!(decode_list(&encoded), Err(StoreError));
        encoded.insert(VERSION.into(), number(1_u64));
        encoded.insert(STATE.into(), string("UNKNOWN"));
        assert_eq!(decode_list(&encoded), Err(StoreError));
    }

    #[test]
    fn items_round_trip_with_their_entry() {
        let item = ShoppingItem::restore(
            ItemId::try_new("item-1").unwrap(),
            ItemDetails {
                name: ItemName::try_new("Milch").unwrap(),
                quantity: Some(ItemQuantity::try_new("2").unwrap()),
                unit: None,
                note: Some(ItemNote::try_new("Bio").unwrap()),
            },
            true,
            7,
            AggregateVersion::try_new(3).unwrap(),
            UserId::try_new("creator").unwrap(),
        );
        let entry = Entry {
            position: 7,
            version: AggregateVersion::try_new(3).unwrap(),
            changed_at: AggregateVersion::try_new(9).unwrap(),
        };
        let list_id = ListId::try_new("list-1").unwrap();
        let encoded = encode_item(&list_id, &item);
        assert!(!encoded.contains_key(UNIT));
        assert_eq!(decode_item(&encoded, &entry).unwrap(), item);
        assert_eq!(item_id_of(&encoded).unwrap(), *item.id());
    }

    #[test]
    fn invitations_round_trip_and_reject_impossible_use_counts() {
        let hash = InvitationTokenHash::from_bytes([9; 32]);
        let invitation = Invitation::new(
            InvitationId::try_new("invite-1").unwrap(),
            HouseholdId::try_new("household").unwrap(),
            UserId::try_new("owner").unwrap(),
            hash,
            Timestamp::from_unix_seconds(1_000),
            InvitationLifetime::try_from_seconds(3_600).unwrap(),
            InvitationMaxUses::try_new(2).unwrap(),
        )
        .unwrap();
        let mut encoded = encode_invitation(&invitation);
        assert!(
            !encoded
                .values()
                .any(|value| value == &string(keys::hash_hex(&hash))),
            "the hash is the key only"
        );
        assert_eq!(decode_invitation(&encoded, &hash).unwrap(), invitation);
        encoded.insert(USED_COUNT.into(), number(3_u32));
        assert_eq!(decode_invitation(&encoded, &hash), Err(StoreError));
    }

    #[test]
    fn households_profiles_and_counters_round_trip() {
        let record = HouseholdRecord {
            household: Household::restore(
                HouseholdId::try_new("household").unwrap(),
                HouseholdName::try_new("WG").unwrap(),
                UserId::try_new("owner").unwrap(),
                AggregateVersion::try_new(2).unwrap(),
            ),
            deleting: true,
            member_count: 3,
        };
        assert_eq!(
            decode_household(&encode_household(&record)).unwrap(),
            record
        );
        let profile = UserProfile::restore(
            UserId::try_new("user").unwrap(),
            DisplayName::try_new("Anna").unwrap(),
            AggregateVersion::try_new(6).unwrap(),
        );
        assert_eq!(decode_profile(&encode_profile(&profile)).unwrap(), profile);
        assert_eq!(counter(None), Ok(0));
        let revision_record = encode_revision(keys::household_invite_rev(record.household.id()), 4);
        assert_eq!(revision(Some(&revision_record)), Ok(Some(4)));
        assert_eq!(revision(None), Ok(None));
    }
}

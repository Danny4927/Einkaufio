//! Single-table key design (docs/data-model.md, ADR 019).
//!
//! Every key is built here so that the layout cannot drift between operations. Sort-key
//! prefixes that are queried with `begins_with` end with `#`; singleton records (`META`,
//! counters) never contain `#`, so a prefix query can never match them. Identifiers are
//! validated domain values (no control characters, at most 128 characters), which keeps every
//! key well below DynamoDB's key size limits.

use einkaufio_domain::{HouseholdId, InvitationId, InvitationTokenHash, ItemId, ListId, UserId};

pub(super) const PK: &str = "PK";
pub(super) const SK: &str = "SK";

pub(super) const META: &str = "META";
pub(super) const PROFILE: &str = "PROFILE";
/// `USER#u`: number of households the user is an active member of (owned or joined).
pub(super) const HOUSEHOLD_COUNT: &str = "HOUSEHOLD_COUNT";
/// `HOUSEHOLD#h`: number of lists the household owns.
pub(super) const LIST_COUNT: &str = "LIST_COUNT";
/// `HOUSEHOLD#h`: revision advanced by every invitation creation (serializes the
/// valid-invitation limit).
pub(super) const INVITE_REV: &str = "INVITE_REV";

pub(super) const LIST_PREFIX: &str = "LIST#";
pub(super) const ITEM_PREFIX: &str = "ITEM#";
pub(super) const TOMBSTONE_PREFIX: &str = "TOMB#";
pub(super) const HOUSEHOLD_PREFIX: &str = "HOUSEHOLD#";
pub(super) const MEMBER_PREFIX: &str = "MEMBER#";
pub(super) const INVITE_PREFIX: &str = "INVITE#";

const USER_PREFIX: &str = "USER#";

/// A primary key of the single table.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct Key {
    pub(super) pk: String,
    pub(super) sk: String,
}

impl Key {
    fn new(pk: String, sk: impl Into<String>) -> Self {
        Self { pk, sk: sk.into() }
    }
}

pub(super) fn user_pk(user: &UserId) -> String {
    format!("{USER_PREFIX}{}", user.as_str())
}
pub(super) fn list_pk(list: &ListId) -> String {
    format!("{LIST_PREFIX}{}", list.as_str())
}
pub(super) fn household_pk(household: &HouseholdId) -> String {
    format!("{HOUSEHOLD_PREFIX}{}", household.as_str())
}

pub(super) fn profile(user: &UserId) -> Key {
    Key::new(user_pk(user), PROFILE)
}
pub(super) fn list_meta(list: &ListId) -> Key {
    Key::new(list_pk(list), META)
}
pub(super) fn item(list: &ListId, item: &ItemId) -> Key {
    Key::new(list_pk(list), format!("{ITEM_PREFIX}{}", item.as_str()))
}
pub(super) fn tombstone(list: &ListId, item: &ItemId) -> Key {
    Key::new(
        list_pk(list),
        format!("{TOMBSTONE_PREFIX}{}", item.as_str()),
    )
}
/// Personal-list reference in the owner's partition.
pub(super) fn user_list_ref(user: &UserId, list: &ListId) -> Key {
    Key::new(user_pk(user), format!("{LIST_PREFIX}{}", list.as_str()))
}
pub(super) fn user_household_ref(user: &UserId, household: &HouseholdId) -> Key {
    Key::new(
        user_pk(user),
        format!("{HOUSEHOLD_PREFIX}{}", household.as_str()),
    )
}
pub(super) fn user_household_count(user: &UserId) -> Key {
    Key::new(user_pk(user), HOUSEHOLD_COUNT)
}
/// Reference from the invitation creator's partition (account deletion, roadmap D2).
pub(super) fn user_created_invite(user: &UserId, hash: &InvitationTokenHash) -> Key {
    Key::new(user_pk(user), format!("{INVITE_PREFIX}{}", hash_hex(hash)))
}
pub(super) fn household_meta(household: &HouseholdId) -> Key {
    Key::new(household_pk(household), META)
}
pub(super) fn member(household: &HouseholdId, user: &UserId) -> Key {
    Key::new(
        household_pk(household),
        format!("{MEMBER_PREFIX}{}", user.as_str()),
    )
}
pub(super) fn household_list_ref(household: &HouseholdId, list: &ListId) -> Key {
    Key::new(
        household_pk(household),
        format!("{LIST_PREFIX}{}", list.as_str()),
    )
}
pub(super) fn household_list_count(household: &HouseholdId) -> Key {
    Key::new(household_pk(household), LIST_COUNT)
}
pub(super) fn household_invite_ref(household: &HouseholdId, invitation: &InvitationId) -> Key {
    Key::new(
        household_pk(household),
        format!("{INVITE_PREFIX}{}", invitation.as_str()),
    )
}
pub(super) fn household_invite_rev(household: &HouseholdId) -> Key {
    Key::new(household_pk(household), INVITE_REV)
}
/// Invitation lookup by token hash (ADR 008/017); the raw token is never stored.
pub(super) fn invitation(hash: &InvitationTokenHash) -> Key {
    Key::new(format!("{INVITE_PREFIX}{}", hash_hex(hash)), META)
}

/// Lower-case hex of the token hash. Hashes are secrets-adjacent: never log keys built from
/// them (docs/security.md).
pub(super) fn hash_hex(hash: &InvitationTokenHash) -> String {
    hex_encode(hash.as_bytes())
}

pub(super) fn parse_hash_hex(value: &str) -> Option<InvitationTokenHash> {
    let bytes = hex_decode(value)?;
    let array: [u8; 32] = bytes.try_into().ok()?;
    Some(InvitationTokenHash::from_bytes(array))
}

pub(super) fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

pub(super) fn hex_decode(value: &str) -> Option<Vec<u8>> {
    let digits = value.as_bytes();
    if !digits.len().is_multiple_of(2) {
        return None;
    }
    digits
        .chunks(2)
        .map(|pair| {
            let high = char::from(pair[0]).to_digit(16)?;
            let low = char::from(pair[1]).to_digit(16)?;
            u8::try_from(high * 16 + low).ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_follow_the_documented_layout() {
        let user = UserId::try_new("u-1").unwrap();
        let list = ListId::try_new("l-1").unwrap();
        let household = HouseholdId::try_new("h-1").unwrap();
        let item_id = ItemId::try_new("i-1").unwrap();
        assert_eq!(list_meta(&list), Key::new("LIST#l-1".into(), "META"));
        assert_eq!(item(&list, &item_id).sk, "ITEM#i-1");
        assert_eq!(tombstone(&list, &item_id).sk, "TOMB#i-1");
        assert_eq!(
            user_list_ref(&user, &list),
            Key::new("USER#u-1".into(), "LIST#l-1")
        );
        assert_eq!(
            member(&household, &user),
            Key::new("HOUSEHOLD#h-1".into(), "MEMBER#u-1")
        );
        assert_eq!(profile(&user).sk, "PROFILE");
        assert_eq!(
            invitation(&InvitationTokenHash::from_bytes([0xab; 32])).pk,
            format!("INVITE#{}", "ab".repeat(32))
        );
    }

    #[test]
    fn singleton_sort_keys_never_match_prefix_queries() {
        for singleton in [META, PROFILE, HOUSEHOLD_COUNT, LIST_COUNT, INVITE_REV] {
            for prefix in [
                LIST_PREFIX,
                ITEM_PREFIX,
                TOMBSTONE_PREFIX,
                HOUSEHOLD_PREFIX,
                MEMBER_PREFIX,
                INVITE_PREFIX,
            ] {
                assert!(!singleton.starts_with(prefix), "{singleton} vs {prefix}");
            }
        }
    }

    #[test]
    fn identifiers_containing_separators_stay_inside_their_prefix() {
        let list = ListId::try_new("a#META").unwrap();
        let tricky = item(&list, &ItemId::try_new("META").unwrap());
        assert_eq!(tricky.pk, "LIST#a#META");
        assert_eq!(tricky.sk, "ITEM#META");
        assert_ne!(tricky.sk, META);
    }

    #[test]
    fn hash_hex_round_trips_and_rejects_malformed_input() {
        let hash = InvitationTokenHash::from_bytes([7; 32]);
        assert_eq!(parse_hash_hex(&hash_hex(&hash)), Some(hash));
        assert_eq!(parse_hash_hex("abc"), None);
        assert_eq!(parse_hash_hex(&"zz".repeat(32)), None);
        assert_eq!(parse_hash_hex(&"ab".repeat(31)), None);
    }
}

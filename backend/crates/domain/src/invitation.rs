//! Household invitations (roadmap C4, ADR 008, ADR 017).
//!
//! An invitation is a bearer secret: whoever holds the raw [`InvitationToken`] may accept it
//! after authenticating. Only its [`InvitationTokenHash`] is persisted. Neither type prints its
//! value through `Debug`, and neither implements `Display`, so they cannot leak into logs by
//! accident.

use std::fmt;

use crate::identifiers::{HouseholdId, InvitationId, UserId};

/// A point in time as whole seconds since the Unix epoch (UTC).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Timestamp(u64);

impl Timestamp {
    pub const fn from_unix_seconds(seconds: u64) -> Self {
        Self(seconds)
    }

    pub const fn as_unix_seconds(self) -> u64 {
        self.0
    }

    pub fn checked_add_seconds(self, seconds: u64) -> Option<Self> {
        self.0.checked_add(seconds).map(Self)
    }
}

/// The raw invitation secret. Returned to the creator exactly once and never persisted.
///
/// Construction from client input only checks the shape (URL-safe characters and a length
/// that can carry at least 256 bits); the entropy guarantee comes from the generator port.
#[derive(Clone, Eq, PartialEq)]
pub struct InvitationToken(String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InvitationTokenError {
    InvalidLength,
    InvalidCharacters,
}

impl InvitationToken {
    /// 256 bits in unpadded base64url need 43 characters; hex needs 64.
    pub const MIN_LENGTH: usize = 43;
    pub const MAX_LENGTH: usize = 128;

    pub fn try_new(value: impl Into<String>) -> Result<Self, InvitationTokenError> {
        let value = value.into();
        if !(Self::MIN_LENGTH..=Self::MAX_LENGTH).contains(&value.len()) {
            return Err(InvitationTokenError::InvalidLength);
        }
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err(InvitationTokenError::InvalidCharacters);
        }
        Ok(Self(value))
    }

    /// The secret value, for hashing and for the one-time response to the creator only.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for InvitationToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("InvitationToken(<redacted>)")
    }
}

/// SHA-256 digest of an [`InvitationToken`]; the only persisted form of the secret.
///
/// Token hashes must not be logged either (docs/security.md), so `Debug` is redacted.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct InvitationTokenHash([u8; 32]);

impl InvitationTokenHash {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for InvitationTokenHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("InvitationTokenHash(<redacted>)")
    }
}

/// How long an invitation stays acceptable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvitationLifetime(u64);

/// How many users may accept one invitation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvitationMaxUses(u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InvitationLimitError {
    TooSmall,
    TooLarge,
}

const DAY: u64 = 24 * 60 * 60;

impl InvitationLifetime {
    pub const MIN_SECONDS: u64 = 60 * 60;
    pub const MAX_SECONDS: u64 = 30 * DAY;
    pub const DEFAULT: Self = Self(7 * DAY);

    pub fn try_from_seconds(seconds: u64) -> Result<Self, InvitationLimitError> {
        if seconds < Self::MIN_SECONDS {
            return Err(InvitationLimitError::TooSmall);
        }
        if seconds > Self::MAX_SECONDS {
            return Err(InvitationLimitError::TooLarge);
        }
        Ok(Self(seconds))
    }

    pub const fn as_seconds(self) -> u64 {
        self.0
    }
}

impl InvitationMaxUses {
    pub const MAX: u32 = 10;
    pub const DEFAULT: Self = Self(5);

    pub fn try_new(value: u32) -> Result<Self, InvitationLimitError> {
        match value {
            0 => Err(InvitationLimitError::TooSmall),
            value if value > Self::MAX => Err(InvitationLimitError::TooLarge),
            value => Ok(Self(value)),
        }
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Why an invitation cannot be used. Callers must not reveal which reason applies to the
/// person presenting the token beyond "unavailable" (ADR 017).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InvitationUseError {
    Revoked,
    Expired,
    Exhausted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InvitationError {
    /// `issued_at + lifetime` does not fit the timestamp range.
    ExpiryOverflow,
}

/// Persisted invitation state that violates the invitation invariants.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InvitationRestoreError {
    /// More uses were recorded than the invitation allows.
    UsedCountExceedsMaxUses,
}

/// An invitation to join a household.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Invitation {
    id: InvitationId,
    household_id: HouseholdId,
    created_by: UserId,
    token_hash: InvitationTokenHash,
    expires_at: Timestamp,
    max_uses: InvitationMaxUses,
    used_count: u32,
    revoked: bool,
}

impl Invitation {
    /// Maximum simultaneously valid invitations per household (roadmap limits).
    pub const MAX_VALID_PER_HOUSEHOLD: usize = 5;

    pub fn new(
        id: InvitationId,
        household_id: HouseholdId,
        created_by: UserId,
        token_hash: InvitationTokenHash,
        issued_at: Timestamp,
        lifetime: InvitationLifetime,
        max_uses: InvitationMaxUses,
    ) -> Result<Self, InvitationError> {
        let expires_at = issued_at
            .checked_add_seconds(lifetime.as_seconds())
            .ok_or(InvitationError::ExpiryOverflow)?;
        Ok(Self {
            id,
            household_id,
            created_by,
            token_hash,
            expires_at,
            max_uses,
            used_count: 0,
            revoked: false,
        })
    }

    /// Reconstitutes an invitation from persisted state (persistence adapters only).
    ///
    /// Performs no business transition. Rejects state that violates the use-count invariant,
    /// so corrupted records fail closed instead of granting extra uses.
    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        id: InvitationId,
        household_id: HouseholdId,
        created_by: UserId,
        token_hash: InvitationTokenHash,
        expires_at: Timestamp,
        max_uses: InvitationMaxUses,
        used_count: u32,
        revoked: bool,
    ) -> Result<Self, InvitationRestoreError> {
        if used_count > max_uses.get() {
            return Err(InvitationRestoreError::UsedCountExceedsMaxUses);
        }
        Ok(Self {
            id,
            household_id,
            created_by,
            token_hash,
            expires_at,
            max_uses,
            used_count,
            revoked,
        })
    }

    pub fn id(&self) -> &InvitationId {
        &self.id
    }

    pub fn household_id(&self) -> &HouseholdId {
        &self.household_id
    }

    pub fn created_by(&self) -> &UserId {
        &self.created_by
    }

    pub fn token_hash(&self) -> &InvitationTokenHash {
        &self.token_hash
    }

    pub const fn expires_at(&self) -> Timestamp {
        self.expires_at
    }

    pub const fn max_uses(&self) -> InvitationMaxUses {
        self.max_uses
    }

    pub const fn used_count(&self) -> u32 {
        self.used_count
    }

    pub const fn is_revoked(&self) -> bool {
        self.revoked
    }

    /// Checks whether the invitation can be used at `now`. Expiry is exclusive: an
    /// invitation is no longer valid at its `expires_at` instant.
    pub fn check_usable(&self, now: Timestamp) -> Result<(), InvitationUseError> {
        if self.revoked {
            return Err(InvitationUseError::Revoked);
        }
        if now >= self.expires_at {
            return Err(InvitationUseError::Expired);
        }
        if self.used_count >= self.max_uses.get() {
            return Err(InvitationUseError::Exhausted);
        }
        Ok(())
    }

    pub fn is_valid_at(&self, now: Timestamp) -> bool {
        self.check_usable(now).is_ok()
    }

    /// Consumes one use. Adapters must apply this atomically with the membership creation.
    pub fn record_use(&mut self, now: Timestamp) -> Result<(), InvitationUseError> {
        self.check_usable(now)?;
        self.used_count += 1;
        Ok(())
    }

    /// Revokes the invitation; revoking twice is a no-op.
    pub fn revoke(&mut self) {
        self.revoked = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn invitation(max_uses: u32) -> Invitation {
        Invitation::new(
            InvitationId::try_new("invite-1").unwrap(),
            HouseholdId::try_new("household-1").unwrap(),
            UserId::try_new("owner").unwrap(),
            InvitationTokenHash::from_bytes([7; 32]),
            Timestamp::from_unix_seconds(1_000),
            InvitationLifetime::try_from_seconds(3_600).unwrap(),
            InvitationMaxUses::try_new(max_uses).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn token_shape_is_validated() {
        assert!(InvitationToken::try_new(SECRET).is_ok());
        assert_eq!(
            InvitationToken::try_new("short"),
            Err(InvitationTokenError::InvalidLength)
        );
        assert_eq!(
            InvitationToken::try_new("x".repeat(129)),
            Err(InvitationTokenError::InvalidLength)
        );
        assert_eq!(
            InvitationToken::try_new(format!("{}/", &SECRET[..50])),
            Err(InvitationTokenError::InvalidCharacters)
        );
    }

    #[test]
    fn secrets_and_hashes_are_redacted_in_debug_output() {
        let token = InvitationToken::try_new(SECRET).unwrap();
        assert!(!format!("{token:?}").contains(SECRET));
        let rendered = format!("{:?}", invitation(1));
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains("7, 7"));
    }

    #[test]
    fn limits_follow_the_roadmap() {
        assert_eq!(InvitationLifetime::DEFAULT.as_seconds(), 7 * DAY);
        assert!(InvitationLifetime::try_from_seconds(30 * DAY).is_ok());
        assert_eq!(
            InvitationLifetime::try_from_seconds(30 * DAY + 1),
            Err(InvitationLimitError::TooLarge)
        );
        assert_eq!(
            InvitationLifetime::try_from_seconds(59),
            Err(InvitationLimitError::TooSmall)
        );
        assert_eq!(InvitationMaxUses::DEFAULT.get(), 5);
        assert!(InvitationMaxUses::try_new(10).is_ok());
        assert_eq!(
            InvitationMaxUses::try_new(11),
            Err(InvitationLimitError::TooLarge)
        );
        assert_eq!(
            InvitationMaxUses::try_new(0),
            Err(InvitationLimitError::TooSmall)
        );
    }

    #[test]
    fn use_count_expiry_and_revocation_are_enforced() {
        let mut invitation = invitation(2);
        let now = Timestamp::from_unix_seconds(1_000);
        invitation.record_use(now).unwrap();
        invitation.record_use(now).unwrap();
        assert_eq!(invitation.used_count(), 2);
        assert_eq!(
            invitation.record_use(now),
            Err(InvitationUseError::Exhausted)
        );
        assert_eq!(invitation.used_count(), 2);

        let fresh = self::invitation(2);
        assert!(fresh.is_valid_at(Timestamp::from_unix_seconds(4_599)));
        assert_eq!(
            fresh.check_usable(Timestamp::from_unix_seconds(4_600)),
            Err(InvitationUseError::Expired)
        );

        let mut revoked = self::invitation(2);
        revoked.revoke();
        revoked.revoke();
        assert_eq!(revoked.record_use(now), Err(InvitationUseError::Revoked));
    }

    #[test]
    fn expiry_overflow_is_rejected() {
        assert_eq!(
            Invitation::new(
                InvitationId::try_new("invite-1").unwrap(),
                HouseholdId::try_new("household-1").unwrap(),
                UserId::try_new("owner").unwrap(),
                InvitationTokenHash::from_bytes([0; 32]),
                Timestamp::from_unix_seconds(u64::MAX),
                InvitationLifetime::DEFAULT,
                InvitationMaxUses::DEFAULT,
            ),
            Err(InvitationError::ExpiryOverflow)
        );
    }

    #[test]
    fn restore_reconstitutes_use_state_and_rejects_impossible_counts() {
        let restore = |used_count| {
            Invitation::restore(
                InvitationId::try_new("invite-1").unwrap(),
                HouseholdId::try_new("household-1").unwrap(),
                UserId::try_new("owner").unwrap(),
                InvitationTokenHash::from_bytes([7; 32]),
                Timestamp::from_unix_seconds(5_000),
                InvitationMaxUses::try_new(2).unwrap(),
                used_count,
                true,
            )
        };
        let restored = restore(2).unwrap();
        assert_eq!(restored.used_count(), 2);
        assert!(restored.is_revoked());
        assert_eq!(restored.expires_at(), Timestamp::from_unix_seconds(5_000));
        assert_eq!(
            restore(3),
            Err(InvitationRestoreError::UsedCountExceedsMaxUses)
        );
    }
}

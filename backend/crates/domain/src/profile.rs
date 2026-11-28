//! User profile aggregate (roadmap B7).

use crate::identifiers::UserId;
use crate::shopping_list::{AggregateVersion, ListMutationError};

/// A validated, trimmed display name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayName(String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisplayNameError {
    Empty,
    TooLong,
    ContainsControlCharacters,
}

impl DisplayName {
    pub const MAX_LENGTH: usize = 80;

    pub fn try_new(value: impl Into<String>) -> Result<Self, DisplayNameError> {
        let normalized = value.into().trim().to_owned();
        if normalized.is_empty() {
            return Err(DisplayNameError::Empty);
        }
        if normalized.chars().count() > Self::MAX_LENGTH {
            return Err(DisplayNameError::TooLong);
        }
        if normalized.chars().any(char::is_control) {
            return Err(DisplayNameError::ContainsControlCharacters);
        }
        Ok(Self(normalized))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The profile of one authenticated user. Its identity is the verified principal; a profile
/// is only ever read or changed by that principal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserProfile {
    user_id: UserId,
    display_name: DisplayName,
    version: AggregateVersion,
}

impl UserProfile {
    pub fn new(user_id: UserId, display_name: DisplayName) -> Self {
        Self {
            user_id,
            display_name,
            version: AggregateVersion::initial(),
        }
    }

    /// Reconstitutes a profile from persisted state (persistence adapters only).
    pub fn restore(user_id: UserId, display_name: DisplayName, version: AggregateVersion) -> Self {
        Self {
            user_id,
            display_name,
            version,
        }
    }

    pub fn user_id(&self) -> &UserId {
        &self.user_id
    }

    pub fn display_name(&self) -> &DisplayName {
        &self.display_name
    }

    pub const fn version(&self) -> AggregateVersion {
        self.version
    }

    /// Changes the display name, advancing the version only when it changes.
    ///
    /// Adapters must compare the expected version before calling this, so a stale no-op
    /// update still conflicts. On version exhaustion the profile is left unchanged.
    pub fn rename(&mut self, display_name: DisplayName) -> Result<bool, ListMutationError> {
        if self.display_name == display_name {
            return Ok(false);
        }
        self.version = self.version.checked_next()?;
        self.display_name = display_name;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> UserProfile {
        UserProfile::new(
            UserId::try_new("user-1").unwrap(),
            DisplayName::try_new(" Anna ").unwrap(),
        )
    }

    #[test]
    fn display_name_is_normalized_and_bounded() {
        assert_eq!(DisplayName::try_new(" Anna ").unwrap().as_str(), "Anna");
        assert_eq!(DisplayName::try_new("  "), Err(DisplayNameError::Empty));
        assert_eq!(
            DisplayName::try_new("x".repeat(81)),
            Err(DisplayNameError::TooLong)
        );
        assert_eq!(
            DisplayName::try_new("a\u{7}b"),
            Err(DisplayNameError::ContainsControlCharacters)
        );
    }

    #[test]
    fn rename_advances_version_only_on_change() {
        let mut profile = profile();
        assert_eq!(profile.version(), AggregateVersion::initial());
        assert_eq!(
            profile.rename(DisplayName::try_new("Anna").unwrap()),
            Ok(false)
        );
        assert_eq!(profile.version().as_u64(), 1);
        assert_eq!(
            profile.rename(DisplayName::try_new("Ben").unwrap()),
            Ok(true)
        );
        assert_eq!(profile.display_name().as_str(), "Ben");
        assert_eq!(profile.version().as_u64(), 2);
    }

    #[test]
    fn exhausted_version_leaves_profile_unchanged() {
        let mut profile = profile();
        profile.version = AggregateVersion::try_new(u64::MAX).unwrap();
        let before = profile.clone();
        assert_eq!(
            profile.rename(DisplayName::try_new("Ben").unwrap()),
            Err(ListMutationError::VersionExhausted)
        );
        assert_eq!(profile, before);
    }

    #[test]
    fn restore_reconstitutes_version() {
        let restored = UserProfile::restore(
            UserId::try_new("user-1").unwrap(),
            DisplayName::try_new("Anna").unwrap(),
            AggregateVersion::try_new(3).unwrap(),
        );
        assert_eq!(restored.version().as_u64(), 3);
        assert_eq!(restored.display_name().as_str(), "Anna");
    }
}

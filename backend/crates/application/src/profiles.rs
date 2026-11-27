//! Profile use cases for the authenticated principal (roadmap B7).

use einkaufio_domain::{AggregateVersion, DisplayName, DisplayNameError, UserId, UserProfile};

use crate::ports::{ProfileRepository, RepositoryError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProfileError {
    InvalidPrincipal,
    InvalidDisplayName(DisplayNameError),
    InvalidExpectedVersion,
    AlreadyExists,
    NotFound,
    Conflict,
    VersionExhausted,
    PersistenceUnavailable,
    RepositoryInvariantViolation,
}

fn map_repository_error(error: RepositoryError) -> ProfileError {
    match error {
        RepositoryError::AlreadyExists => ProfileError::AlreadyExists,
        RepositoryError::NotFound => ProfileError::NotFound,
        RepositoryError::Conflict => ProfileError::Conflict,
        RepositoryError::VersionExhausted => ProfileError::VersionExhausted,
        RepositoryError::Unavailable => ProfileError::PersistenceUnavailable,
        // Profiles are owned by their principal only; there is no permission or cursor.
        RepositoryError::Forbidden
        | RepositoryError::ItemLimitReached
        | RepositoryError::InvalidCursor => ProfileError::RepositoryInvariantViolation,
    }
}

fn principal(value: String) -> Result<UserId, ProfileError> {
    UserId::try_new(value).map_err(|_| ProfileError::InvalidPrincipal)
}

fn display_name(value: String) -> Result<DisplayName, ProfileError> {
    DisplayName::try_new(value).map_err(ProfileError::InvalidDisplayName)
}

/// Identity is always the verified principal; no command carries another user's id.
pub struct CreateProfile<'a, R> {
    repository: &'a R,
}

impl<'a, R: ProfileRepository> CreateProfile<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    pub async fn execute(
        &self,
        principal_id: String,
        name: String,
    ) -> Result<UserProfile, ProfileError> {
        let profile = UserProfile::new(principal(principal_id)?, display_name(name)?);
        self.repository
            .create_profile(profile.clone())
            .await
            .map_err(map_repository_error)?;
        Ok(profile)
    }
}

pub struct GetProfile<'a, R> {
    repository: &'a R,
}

impl<'a, R: ProfileRepository> GetProfile<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    pub async fn execute(&self, principal_id: String) -> Result<UserProfile, ProfileError> {
        let principal = principal(principal_id)?;
        let profile = self
            .repository
            .get_profile(&principal)
            .await
            .map_err(map_repository_error)?;
        if profile.user_id() != &principal {
            return Err(ProfileError::RepositoryInvariantViolation);
        }
        Ok(profile)
    }
}

pub struct UpdateProfile<'a, R> {
    repository: &'a R,
}

impl<'a, R: ProfileRepository> UpdateProfile<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    pub async fn execute(
        &self,
        principal_id: String,
        expected_version: u64,
        name: String,
    ) -> Result<UserProfile, ProfileError> {
        let principal = principal(principal_id)?;
        let expected = AggregateVersion::try_new(expected_version)
            .map_err(|_| ProfileError::InvalidExpectedVersion)?;
        let name = display_name(name)?;
        let profile = self
            .repository
            .update_profile(&principal, expected, name)
            .await
            .map_err(map_repository_error)?;
        if profile.user_id() != &principal {
            return Err(ProfileError::RepositoryInvariantViolation);
        }
        Ok(profile)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;
    use crate::test_support::BlockOn;

    #[derive(Default)]
    struct Fake {
        profiles: Mutex<HashMap<UserId, UserProfile>>,
        failure: Option<RepositoryError>,
        foreign: bool,
    }

    impl ProfileRepository for Fake {
        async fn create_profile(&self, profile: UserProfile) -> Result<(), RepositoryError> {
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            self.profiles
                .lock()
                .unwrap()
                .insert(profile.user_id().clone(), profile);
            Ok(())
        }
        async fn get_profile(&self, principal: &UserId) -> Result<UserProfile, RepositoryError> {
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            if self.foreign {
                return Ok(UserProfile::new(
                    UserId::try_new("someone-else").unwrap(),
                    DisplayName::try_new("X").unwrap(),
                ));
            }
            self.profiles
                .lock()
                .unwrap()
                .get(principal)
                .cloned()
                .ok_or(RepositoryError::NotFound)
        }
        async fn update_profile(
            &self,
            principal: &UserId,
            expected: AggregateVersion,
            name: DisplayName,
        ) -> Result<UserProfile, RepositoryError> {
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            let mut profiles = self.profiles.lock().unwrap();
            let profile = profiles
                .get_mut(principal)
                .ok_or(RepositoryError::NotFound)?;
            if profile.version() != expected {
                return Err(RepositoryError::Conflict);
            }
            profile.rename(name).unwrap();
            Ok(profile.clone())
        }
    }

    #[test]
    fn create_read_update_for_the_verified_principal() {
        let fake = Fake::default();
        let created = CreateProfile::new(&fake)
            .execute("user-1".into(), " Anna ".into())
            .block_on()
            .unwrap();
        assert_eq!(created.user_id().as_str(), "user-1");
        assert_eq!(created.display_name().as_str(), "Anna");
        assert_eq!(
            GetProfile::new(&fake).execute("user-1".into()).block_on(),
            Ok(created)
        );
        let updated = UpdateProfile::new(&fake)
            .execute("user-1".into(), 1, "Ben".into())
            .block_on()
            .unwrap();
        assert_eq!(updated.version().as_u64(), 2);
        assert_eq!(
            UpdateProfile::new(&fake)
                .execute("user-1".into(), 1, "Carl".into())
                .block_on(),
            Err(ProfileError::Conflict)
        );
        assert_eq!(
            GetProfile::new(&fake).execute("user-2".into()).block_on(),
            Err(ProfileError::NotFound)
        );
    }

    #[test]
    fn invalid_input_is_rejected_before_persistence() {
        let fake = Fake::default();
        assert_eq!(
            CreateProfile::new(&fake)
                .execute(String::new(), "Anna".into())
                .block_on(),
            Err(ProfileError::InvalidPrincipal)
        );
        assert_eq!(
            CreateProfile::new(&fake)
                .execute("user-1".into(), " ".into())
                .block_on(),
            Err(ProfileError::InvalidDisplayName(DisplayNameError::Empty))
        );
        assert_eq!(
            UpdateProfile::new(&fake)
                .execute("user-1".into(), 0, "Anna".into())
                .block_on(),
            Err(ProfileError::InvalidExpectedVersion)
        );
        assert!(fake.profiles.lock().unwrap().is_empty());
    }

    #[test]
    fn repository_failures_are_mapped_and_foreign_profiles_rejected() {
        for (failure, expected) in [
            (RepositoryError::AlreadyExists, ProfileError::AlreadyExists),
            (
                RepositoryError::Unavailable,
                ProfileError::PersistenceUnavailable,
            ),
            (
                RepositoryError::Forbidden,
                ProfileError::RepositoryInvariantViolation,
            ),
        ] {
            let fake = Fake {
                failure: Some(failure),
                ..Fake::default()
            };
            assert_eq!(
                CreateProfile::new(&fake)
                    .execute("user-1".into(), "Anna".into())
                    .block_on(),
                Err(expected)
            );
        }
        let fake = Fake {
            foreign: true,
            ..Fake::default()
        };
        assert_eq!(
            GetProfile::new(&fake).execute("user-1".into()).block_on(),
            Err(ProfileError::RepositoryInvariantViolation)
        );
    }
}

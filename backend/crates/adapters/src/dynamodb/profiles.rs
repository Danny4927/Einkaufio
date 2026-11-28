//! Profile port (roadmap B7, D2). Every operation addresses only `USER#<principal> / PROFILE`.

use einkaufio_application::{ProfileRepository, RepositoryError};
use einkaufio_domain::{AggregateVersion, DisplayName, UserId, UserProfile};

use super::DynamoDbRepository;
use super::keys;
use super::records::{self, VERSION};
use super::store::{Action, Commit, Condition, MAX_ATTEMPTS, number};

impl DynamoDbRepository {
    async fn read_profile(
        &self,
        principal: &UserId,
    ) -> Result<Option<UserProfile>, RepositoryError> {
        self.store
            .get(&keys::profile(principal))
            .await?
            .map(|item| records::decode_profile(&item))
            .transpose()
            .map_err(RepositoryError::from)
    }
}

fn unchanged(profile: &UserProfile) -> Condition {
    Condition::present().equals(VERSION, number(profile.version().as_u64()))
}

impl ProfileRepository for DynamoDbRepository {
    async fn create_profile(&self, profile: UserProfile) -> Result<(), RepositoryError> {
        for attempt in 0..MAX_ATTEMPTS {
            self.store.pause(attempt).await;
            if self.read_profile(profile.user_id()).await?.is_some() {
                return Err(RepositoryError::AlreadyExists);
            }
            let action = Action::Put {
                item: records::encode_profile(&profile),
                condition: Condition::missing(),
            };
            if self.store.commit(vec![action]).await? == Commit::Committed {
                return Ok(());
            }
        }
        Err(RepositoryError::Unavailable)
    }

    async fn get_profile(&self, principal: &UserId) -> Result<UserProfile, RepositoryError> {
        self.read_profile(principal)
            .await?
            .ok_or(RepositoryError::NotFound)
    }

    async fn update_profile(
        &self,
        principal: &UserId,
        expected_version: AggregateVersion,
        display_name: DisplayName,
    ) -> Result<UserProfile, RepositoryError> {
        for attempt in 0..MAX_ATTEMPTS {
            self.store.pause(attempt).await;
            let current = self
                .read_profile(principal)
                .await?
                .ok_or(RepositoryError::NotFound)?;
            // The expected version is compared before no-op detection.
            if current.version() != expected_version {
                return Err(RepositoryError::Conflict);
            }
            let mut next = current.clone();
            if !next
                .rename(display_name.clone())
                .map_err(|_| RepositoryError::VersionExhausted)?
            {
                return Ok(next);
            }
            let action = Action::Put {
                item: records::encode_profile(&next),
                condition: unchanged(&current),
            };
            if self.store.commit(vec![action]).await? == Commit::Committed {
                return Ok(next);
            }
        }
        Err(RepositoryError::Unavailable)
    }

    async fn delete_profile(&self, principal: &UserId) -> Result<(), RepositoryError> {
        for attempt in 0..MAX_ATTEMPTS {
            self.store.pause(attempt).await;
            let current = self
                .read_profile(principal)
                .await?
                .ok_or(RepositoryError::NotFound)?;
            let action = Action::Delete {
                key: keys::profile(principal),
                condition: unchanged(&current),
            };
            if self.store.commit(vec![action]).await? == Commit::Committed {
                return Ok(());
            }
        }
        Err(RepositoryError::Unavailable)
    }
}

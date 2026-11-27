//! Household invitation use cases (roadmap C4, ADR 008, ADR 017).
//!
//! The raw token exists only inside [`CreateHouseholdInvitation`] (returned once to the
//! creator) and [`AcceptHouseholdInvitation`] (hashed immediately). Repositories receive only
//! the hash. Commands and results that carry a token never print it through `Debug`.

use std::fmt;

use einkaufio_domain::{
    HouseholdOperation, Invitation, InvitationId, InvitationLifetime, InvitationMaxUses,
    InvitationToken,
};

use crate::household_ports::{
    Clock, HouseholdAccessRequest, HouseholdSummary, InvitationIdGenerator, InvitationRepository,
    InvitationTokenGenerator, InvitationTokenHasher,
};
use crate::households::{HouseholdError, actor, household_id};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateHouseholdInvitationCommand {
    pub actor_id: String,
    pub household_id: String,
    /// Defaults to `InvitationLifetime::DEFAULT` (7 days); at most 30 days.
    pub lifetime_seconds: Option<u64>,
    /// Defaults to `InvitationMaxUses::DEFAULT` (5); at most 10.
    pub max_uses: Option<u32>,
}

/// A new invitation and its raw token. The token is shown to the creator exactly once; it is
/// not stored and cannot be retrieved again.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatedInvitation {
    pub invitation: Invitation,
    pub token: InvitationToken,
}

/// Collaborators of [`CreateHouseholdInvitation`].
pub struct InvitationServices<'a> {
    pub tokens: &'a dyn InvitationTokenGenerator,
    pub hasher: &'a dyn InvitationTokenHasher,
    pub ids: &'a dyn InvitationIdGenerator,
    pub clock: &'a dyn Clock,
}

pub struct CreateHouseholdInvitation<'a, R> {
    repository: &'a R,
    services: InvitationServices<'a>,
}

impl<'a, R: InvitationRepository> CreateHouseholdInvitation<'a, R> {
    pub fn new(repository: &'a R, services: InvitationServices<'a>) -> Self {
        Self {
            repository,
            services,
        }
    }

    pub async fn execute(
        &self,
        command: CreateHouseholdInvitationCommand,
    ) -> Result<CreatedInvitation, HouseholdError> {
        let creator = actor(command.actor_id)?;
        let household_id = household_id(command.household_id)?;
        let lifetime = command
            .lifetime_seconds
            .map(InvitationLifetime::try_from_seconds)
            .transpose()
            .map_err(HouseholdError::InvalidLifetime)?
            .unwrap_or(InvitationLifetime::DEFAULT);
        let max_uses = command
            .max_uses
            .map(InvitationMaxUses::try_new)
            .transpose()
            .map_err(HouseholdError::InvalidMaxUses)?
            .unwrap_or(InvitationMaxUses::DEFAULT);
        let id = InvitationId::try_new(self.services.ids.generate())
            .map_err(HouseholdError::InvalidGeneratedId)?;
        let token = self
            .services
            .tokens
            .generate()
            .map_err(|_| HouseholdError::TokenGenerationFailed)?;
        // Re-validate the generator's output so that a faulty adapter cannot weaken tokens.
        if InvitationToken::try_new(token.expose_secret()).is_err() {
            return Err(HouseholdError::InvalidGeneratedToken);
        }
        let now = self.services.clock.now();
        let invitation = Invitation::new(
            id,
            household_id,
            creator.clone(),
            self.services.hasher.hash(&token),
            now,
            lifetime,
            max_uses,
        )
        .map_err(|_| HouseholdError::ExpiryOverflow)?;
        let access =
            HouseholdAccessRequest::for_operation(creator, HouseholdOperation::ManageInvitations);
        self.repository
            .create_invitation(&access, invitation.clone(), now)
            .await
            .map_err(HouseholdError::Repository)?;
        Ok(CreatedInvitation { invitation, token })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListHouseholdInvitationsQuery {
    pub actor_id: String,
    pub household_id: String,
}

pub struct ListHouseholdInvitations<'a, R> {
    repository: &'a R,
}

impl<'a, R: InvitationRepository> ListHouseholdInvitations<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    pub async fn execute(
        &self,
        query: ListHouseholdInvitationsQuery,
    ) -> Result<Vec<Invitation>, HouseholdError> {
        let access = HouseholdAccessRequest::for_operation(
            actor(query.actor_id)?,
            HouseholdOperation::ManageInvitations,
        );
        self.repository
            .list_invitations(&access, &household_id(query.household_id)?)
            .await
            .map_err(HouseholdError::Repository)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevokeHouseholdInvitationCommand {
    pub actor_id: String,
    pub household_id: String,
    pub invitation_id: String,
}

pub struct RevokeHouseholdInvitation<'a, R> {
    repository: &'a R,
}

impl<'a, R: InvitationRepository> RevokeHouseholdInvitation<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    pub async fn execute(
        &self,
        command: RevokeHouseholdInvitationCommand,
    ) -> Result<Invitation, HouseholdError> {
        let access = HouseholdAccessRequest::for_operation(
            actor(command.actor_id)?,
            HouseholdOperation::ManageInvitations,
        );
        let household_id = household_id(command.household_id)?;
        let invitation_id = InvitationId::try_new(command.invitation_id)
            .map_err(HouseholdError::InvalidInvitationId)?;
        self.repository
            .revoke_invitation(&access, &household_id, &invitation_id)
            .await
            .map_err(HouseholdError::Repository)
    }
}

/// Acceptance by an authenticated principal. `Debug` redacts the token.
#[derive(Clone, Eq, PartialEq)]
pub struct AcceptHouseholdInvitationCommand {
    /// Identity from verified authentication context; becomes the new member.
    pub actor_id: String,
    /// The raw bearer token from the invitation URL.
    pub token: String,
}

impl fmt::Debug for AcceptHouseholdInvitationCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AcceptHouseholdInvitationCommand")
            .field("actor_id", &self.actor_id)
            .field("token", &"<redacted>")
            .finish()
    }
}

pub struct AcceptHouseholdInvitation<'a, R> {
    repository: &'a R,
    hasher: &'a dyn InvitationTokenHasher,
    clock: &'a dyn Clock,
}

impl<'a, R: InvitationRepository> AcceptHouseholdInvitation<'a, R> {
    pub fn new(
        repository: &'a R,
        hasher: &'a dyn InvitationTokenHasher,
        clock: &'a dyn Clock,
    ) -> Self {
        Self {
            repository,
            hasher,
            clock,
        }
    }

    pub async fn execute(
        &self,
        command: AcceptHouseholdInvitationCommand,
    ) -> Result<HouseholdSummary, HouseholdError> {
        let principal = actor(command.actor_id)?;
        let token = InvitationToken::try_new(command.token)
            .map_err(|_| HouseholdError::InvalidInvitationToken)?;
        let token_hash = self.hasher.hash(&token);
        drop(token);
        self.repository
            .accept_invitation(&principal, &token_hash, self.clock.now())
            .await
            .map_err(HouseholdError::Repository)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use einkaufio_domain::{
        Household, HouseholdId, HouseholdName, HouseholdPermission, HouseholdRole,
        InvitationLimitError, InvitationTokenHash, Timestamp, UserId,
    };

    use super::*;
    use crate::household_ports::{HouseholdRepositoryError, TokenGenerationError};
    use crate::test_support::BlockOn;

    const SECRET: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[derive(Default)]
    struct FakeInvitations {
        created: Mutex<Vec<(HouseholdAccessRequest, Invitation, Timestamp)>>,
        accepted: Mutex<Vec<(UserId, InvitationTokenHash, Timestamp)>>,
    }

    impl InvitationRepository for FakeInvitations {
        async fn create_invitation(
            &self,
            access: &HouseholdAccessRequest,
            invitation: Invitation,
            now: Timestamp,
        ) -> Result<(), HouseholdRepositoryError> {
            self.created
                .lock()
                .unwrap()
                .push((access.clone(), invitation, now));
            Ok(())
        }

        async fn accept_invitation(
            &self,
            principal: &UserId,
            token_hash: &InvitationTokenHash,
            now: Timestamp,
        ) -> Result<HouseholdSummary, HouseholdRepositoryError> {
            self.accepted
                .lock()
                .unwrap()
                .push((principal.clone(), *token_hash, now));
            Ok(HouseholdSummary {
                household: Household::new(
                    HouseholdId::try_new("h-1").unwrap(),
                    UserId::try_new("owner").unwrap(),
                    HouseholdName::try_new("Familie").unwrap(),
                ),
                role: HouseholdRole::Member,
            })
        }
    }

    struct Tokens(Result<&'static str, TokenGenerationError>);

    impl InvitationTokenGenerator for Tokens {
        fn generate(&self) -> Result<InvitationToken, TokenGenerationError> {
            self.0
                .clone()
                .map(|secret| InvitationToken::try_new(secret).unwrap())
        }
    }

    /// Test hasher: first byte marks the input length so that tests see a hash, not the token.
    struct LengthHasher;

    impl InvitationTokenHasher for LengthHasher {
        fn hash(&self, token: &InvitationToken) -> InvitationTokenHash {
            let mut bytes = [0xab; 32];
            bytes[0] = u8::try_from(token.expose_secret().len()).unwrap();
            InvitationTokenHash::from_bytes(bytes)
        }
    }

    struct Ids;

    impl InvitationIdGenerator for Ids {
        fn generate(&self) -> String {
            "invite-1".to_owned()
        }
    }

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> Timestamp {
            Timestamp::from_unix_seconds(1_000_000)
        }
    }

    fn create(
        repository: &FakeInvitations,
        tokens: &Tokens,
        command: CreateHouseholdInvitationCommand,
    ) -> Result<CreatedInvitation, HouseholdError> {
        CreateHouseholdInvitation::new(
            repository,
            InvitationServices {
                tokens,
                hasher: &LengthHasher,
                ids: &Ids,
                clock: &FixedClock,
            },
        )
        .execute(command)
        .block_on()
    }

    fn command(
        lifetime_seconds: Option<u64>,
        max_uses: Option<u32>,
    ) -> CreateHouseholdInvitationCommand {
        CreateHouseholdInvitationCommand {
            actor_id: "owner".to_owned(),
            household_id: "h-1".to_owned(),
            lifetime_seconds,
            max_uses,
        }
    }

    #[test]
    fn creation_returns_the_raw_token_once_and_persists_only_its_hash() {
        let repository = FakeInvitations::default();
        let created = create(&repository, &Tokens(Ok(SECRET)), command(None, None)).unwrap();
        assert_eq!(created.token.expose_secret(), SECRET);

        let stored = repository.created.lock().unwrap();
        let (access, invitation, now) = &stored[0];
        assert_eq!(access.principal().as_str(), "owner");
        assert_eq!(access.permission(), HouseholdPermission::Administer);
        assert_eq!(invitation.token_hash(), &LengthHasher.hash(&created.token));
        assert_eq!(invitation.created_by().as_str(), "owner");
        assert_eq!(invitation.max_uses(), InvitationMaxUses::DEFAULT);
        assert_eq!(
            invitation.expires_at().as_unix_seconds(),
            1_000_000 + InvitationLifetime::DEFAULT.as_seconds()
        );
        assert_eq!(now.as_unix_seconds(), 1_000_000);
        // Neither the stored record nor the result prints the secret.
        assert!(!format!("{invitation:?}").contains(SECRET));
        assert!(!format!("{created:?}").contains(SECRET));
    }

    #[test]
    fn creation_enforces_lifetime_and_use_limits_before_persisting() {
        let repository = FakeInvitations::default();
        let tokens = Tokens(Ok(SECRET));
        assert_eq!(
            create(&repository, &tokens, command(Some(31 * 24 * 3600), None)),
            Err(HouseholdError::InvalidLifetime(
                InvitationLimitError::TooLarge
            ))
        );
        assert_eq!(
            create(&repository, &tokens, command(None, Some(11))),
            Err(HouseholdError::InvalidMaxUses(
                InvitationLimitError::TooLarge
            ))
        );
        assert_eq!(
            create(&repository, &tokens, command(None, Some(0))),
            Err(HouseholdError::InvalidMaxUses(
                InvitationLimitError::TooSmall
            ))
        );
        assert!(repository.created.lock().unwrap().is_empty());

        let created = create(
            &repository,
            &tokens,
            command(Some(30 * 24 * 3600), Some(10)),
        )
        .unwrap();
        assert_eq!(created.invitation.max_uses().get(), 10);
    }

    #[test]
    fn failing_random_source_fails_closed() {
        let repository = FakeInvitations::default();
        assert_eq!(
            create(
                &repository,
                &Tokens(Err(TokenGenerationError)),
                command(None, None)
            ),
            Err(HouseholdError::TokenGenerationFailed)
        );
        assert!(repository.created.lock().unwrap().is_empty());
    }

    #[test]
    fn acceptance_hashes_the_token_and_uses_the_verified_actor_and_server_time() {
        let repository = FakeInvitations::default();
        let use_case = AcceptHouseholdInvitation::new(&repository, &LengthHasher, &FixedClock);
        let summary = use_case
            .execute(AcceptHouseholdInvitationCommand {
                actor_id: "invitee".to_owned(),
                token: SECRET.to_owned(),
            })
            .block_on()
            .unwrap();
        assert_eq!(summary.role, HouseholdRole::Member);
        let accepted = repository.accepted.lock().unwrap();
        assert_eq!(accepted[0].0.as_str(), "invitee");
        assert_eq!(
            accepted[0].1,
            LengthHasher.hash(&InvitationToken::try_new(SECRET).unwrap())
        );
        assert_eq!(accepted[0].2.as_unix_seconds(), 1_000_000);
    }

    #[test]
    fn malformed_tokens_and_actors_are_rejected_without_a_lookup() {
        let repository = FakeInvitations::default();
        let use_case = AcceptHouseholdInvitation::new(&repository, &LengthHasher, &FixedClock);
        assert_eq!(
            use_case
                .execute(AcceptHouseholdInvitationCommand {
                    actor_id: "invitee".to_owned(),
                    token: "too-short".to_owned(),
                })
                .block_on(),
            Err(HouseholdError::InvalidInvitationToken)
        );
        assert!(matches!(
            use_case
                .execute(AcceptHouseholdInvitationCommand {
                    actor_id: "".to_owned(),
                    token: SECRET.to_owned(),
                })
                .block_on(),
            Err(HouseholdError::InvalidActor(_))
        ));
        assert!(repository.accepted.lock().unwrap().is_empty());
    }

    #[test]
    fn accept_command_debug_output_redacts_the_token() {
        let rendered = format!(
            "{:?}",
            AcceptHouseholdInvitationCommand {
                actor_id: "invitee".to_owned(),
                token: SECRET.to_owned(),
            }
        );
        assert!(rendered.contains("invitee"));
        assert!(!rendered.contains(SECRET));
    }

    #[test]
    fn listing_and_revoking_require_administration_and_fail_closed_by_default() {
        let repository = FakeInvitations::default();
        assert_eq!(
            ListHouseholdInvitations::new(&repository)
                .execute(ListHouseholdInvitationsQuery {
                    actor_id: "owner".to_owned(),
                    household_id: "h-1".to_owned(),
                })
                .block_on(),
            Err(HouseholdError::Repository(
                HouseholdRepositoryError::Unavailable
            ))
        );
        assert!(matches!(
            RevokeHouseholdInvitation::new(&repository)
                .execute(RevokeHouseholdInvitationCommand {
                    actor_id: "owner".to_owned(),
                    household_id: "h-1".to_owned(),
                    invitation_id: " ".to_owned(),
                })
                .block_on(),
            Err(HouseholdError::InvalidInvitationId(_))
        ));
    }
}

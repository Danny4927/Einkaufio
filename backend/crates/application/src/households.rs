//! Household use cases (roadmap C2) and membership lifecycle (roadmap C3).
//!
//! Actor identities come from verified authentication context only. Every use case builds its
//! [`HouseholdAccessRequest`] from the domain rule table ([`HouseholdOperation`]); adapters
//! enforce it atomically with the read or mutation.

use einkaufio_domain::{
    AggregateVersion, Household, HouseholdId, HouseholdName, HouseholdNameError,
    HouseholdOperation, IdentifierError, InvitationLimitError, ListId, ListName, ListNameError,
    ShoppingList, UserId,
};

use crate::household_ports::{
    HouseholdAccessRequest, HouseholdDetails, HouseholdIdGenerator, HouseholdRepository,
    HouseholdRepositoryError, HouseholdSummary,
};
use crate::ports::ListIdGenerator;

/// Errors of the household and invitation use cases.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HouseholdError {
    InvalidActor(IdentifierError),
    InvalidHouseholdId(IdentifierError),
    InvalidMemberId(IdentifierError),
    InvalidInvitationId(IdentifierError),
    InvalidName(HouseholdNameError),
    InvalidListName(ListNameError),
    InvalidGeneratedId(IdentifierError),
    InvalidLifetime(InvitationLimitError),
    InvalidMaxUses(InvitationLimitError),
    /// The presented token is malformed. The API must answer exactly as for
    /// `Repository(InvitationUnavailable)`.
    InvalidInvitationToken,
    /// The secure random source failed; no invitation was created (fail closed).
    TokenGenerationFailed,
    /// The generated token violated the token invariants; no invitation was created.
    InvalidGeneratedToken,
    /// The clock is beyond the representable invitation expiry.
    ExpiryOverflow,
    Repository(HouseholdRepositoryError),
}

pub(crate) fn actor(value: String) -> Result<UserId, HouseholdError> {
    UserId::try_new(value).map_err(HouseholdError::InvalidActor)
}

pub(crate) fn household_id(value: String) -> Result<HouseholdId, HouseholdError> {
    HouseholdId::try_new(value).map_err(HouseholdError::InvalidHouseholdId)
}

fn access(
    actor_id: String,
    operation: HouseholdOperation,
) -> Result<HouseholdAccessRequest, HouseholdError> {
    Ok(HouseholdAccessRequest::for_operation(
        actor(actor_id)?,
        operation,
    ))
}

fn member(value: String) -> Result<UserId, HouseholdError> {
    UserId::try_new(value).map_err(HouseholdError::InvalidMemberId)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateHouseholdCommand {
    /// Identity from verified authentication context; becomes the owner.
    pub actor_id: String,
    pub name: String,
}

pub struct CreateHousehold<'a, R> {
    repository: &'a R,
    ids: &'a dyn HouseholdIdGenerator,
}

impl<'a, R: HouseholdRepository> CreateHousehold<'a, R> {
    pub fn new(repository: &'a R, ids: &'a dyn HouseholdIdGenerator) -> Self {
        Self { repository, ids }
    }

    pub async fn execute(
        &self,
        command: CreateHouseholdCommand,
    ) -> Result<Household, HouseholdError> {
        let owner = actor(command.actor_id)?;
        let name = HouseholdName::try_new(command.name).map_err(HouseholdError::InvalidName)?;
        let id = HouseholdId::try_new(self.ids.generate())
            .map_err(HouseholdError::InvalidGeneratedId)?;
        let household = Household::new(id, owner, name);
        self.repository
            .create_household(household.clone())
            .await
            .map_err(HouseholdError::Repository)?;
        Ok(household)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListHouseholdsQuery {
    pub actor_id: String,
}

pub struct ListHouseholds<'a, R> {
    repository: &'a R,
}

impl<'a, R: HouseholdRepository> ListHouseholds<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    pub async fn execute(
        &self,
        query: ListHouseholdsQuery,
    ) -> Result<Vec<HouseholdSummary>, HouseholdError> {
        self.repository
            .list_households(&actor(query.actor_id)?)
            .await
            .map_err(HouseholdError::Repository)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GetHouseholdQuery {
    pub actor_id: String,
    pub household_id: String,
}

pub struct GetHousehold<'a, R> {
    repository: &'a R,
}

impl<'a, R: HouseholdRepository> GetHousehold<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    pub async fn execute(
        &self,
        query: GetHouseholdQuery,
    ) -> Result<HouseholdDetails, HouseholdError> {
        let access = access(query.actor_id, HouseholdOperation::View)?;
        let household_id = household_id(query.household_id)?;
        self.repository
            .get_household(&access, &household_id)
            .await
            .map_err(HouseholdError::Repository)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenameHouseholdCommand {
    pub actor_id: String,
    pub household_id: String,
    pub expected_version: AggregateVersion,
    pub name: String,
}

pub struct RenameHousehold<'a, R> {
    repository: &'a R,
}

impl<'a, R: HouseholdRepository> RenameHousehold<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    pub async fn execute(
        &self,
        command: RenameHouseholdCommand,
    ) -> Result<Household, HouseholdError> {
        let access = access(command.actor_id, HouseholdOperation::Rename)?;
        let household_id = household_id(command.household_id)?;
        let name = HouseholdName::try_new(command.name).map_err(HouseholdError::InvalidName)?;
        self.repository
            .rename_household(&access, &household_id, command.expected_version, name)
            .await
            .map_err(HouseholdError::Repository)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateHouseholdListCommand {
    pub actor_id: String,
    pub household_id: String,
    pub name: String,
}

/// Creates a household-owned list. Any active member may do so (ADR 017); the list limit
/// is enforced by the adapter.
pub struct CreateHouseholdList<'a, R> {
    repository: &'a R,
    ids: &'a dyn ListIdGenerator,
}

impl<'a, R: HouseholdRepository> CreateHouseholdList<'a, R> {
    pub fn new(repository: &'a R, ids: &'a dyn ListIdGenerator) -> Self {
        Self { repository, ids }
    }

    pub async fn execute(
        &self,
        command: CreateHouseholdListCommand,
    ) -> Result<ShoppingList, HouseholdError> {
        let access = access(command.actor_id, HouseholdOperation::CreateList)?;
        let household_id = household_id(command.household_id)?;
        let name = ListName::try_new(command.name).map_err(HouseholdError::InvalidListName)?;
        let id =
            ListId::try_new(self.ids.generate()).map_err(HouseholdError::InvalidGeneratedId)?;
        let list = ShoppingList::new_household(id, household_id, name);
        self.repository
            .create_household_list(&access, list.clone())
            .await
            .map_err(HouseholdError::Repository)?;
        Ok(list)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeaveHouseholdCommand {
    pub actor_id: String,
    pub household_id: String,
}

pub struct LeaveHousehold<'a, R> {
    repository: &'a R,
}

impl<'a, R: HouseholdRepository> LeaveHousehold<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    pub async fn execute(&self, command: LeaveHouseholdCommand) -> Result<(), HouseholdError> {
        let principal = actor(command.actor_id)?;
        let household_id = household_id(command.household_id)?;
        self.repository
            .leave_household(&principal, &household_id)
            .await
            .map(|_| ())
            .map_err(HouseholdError::Repository)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoveHouseholdMemberCommand {
    pub actor_id: String,
    pub household_id: String,
    pub expected_version: AggregateVersion,
    pub member_id: String,
}

pub struct RemoveHouseholdMember<'a, R> {
    repository: &'a R,
}

impl<'a, R: HouseholdRepository> RemoveHouseholdMember<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    pub async fn execute(
        &self,
        command: RemoveHouseholdMemberCommand,
    ) -> Result<Household, HouseholdError> {
        let access = access(command.actor_id, HouseholdOperation::RemoveMember)?;
        let household_id = household_id(command.household_id)?;
        let member = member(command.member_id)?;
        self.repository
            .remove_member(&access, &household_id, command.expected_version, &member)
            .await
            .map_err(HouseholdError::Repository)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferHouseholdOwnershipCommand {
    pub actor_id: String,
    pub household_id: String,
    pub expected_version: AggregateVersion,
    pub new_owner_id: String,
}

pub struct TransferHouseholdOwnership<'a, R> {
    repository: &'a R,
}

impl<'a, R: HouseholdRepository> TransferHouseholdOwnership<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    pub async fn execute(
        &self,
        command: TransferHouseholdOwnershipCommand,
    ) -> Result<Household, HouseholdError> {
        let access = access(command.actor_id, HouseholdOperation::TransferOwnership)?;
        let household_id = household_id(command.household_id)?;
        let new_owner = member(command.new_owner_id)?;
        self.repository
            .transfer_ownership(&access, &household_id, command.expected_version, &new_owner)
            .await
            .map_err(HouseholdError::Repository)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeleteHouseholdCommand {
    pub actor_id: String,
    pub household_id: String,
    pub expected_version: AggregateVersion,
}

pub struct DeleteHousehold<'a, R> {
    repository: &'a R,
}

impl<'a, R: HouseholdRepository> DeleteHousehold<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    pub async fn execute(&self, command: DeleteHouseholdCommand) -> Result<(), HouseholdError> {
        let access = access(command.actor_id, HouseholdOperation::Delete)?;
        let household_id = household_id(command.household_id)?;
        self.repository
            .delete_household(&access, &household_id, command.expected_version)
            .await
            .map_err(HouseholdError::Repository)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use einkaufio_domain::{HouseholdPermission, ListOwner};

    use super::*;
    use crate::test_support::BlockOn;

    #[derive(Default)]
    struct FakeHouseholds {
        created: Mutex<Vec<Household>>,
        lists: Mutex<Vec<ShoppingList>>,
        requests: Mutex<Vec<HouseholdAccessRequest>>,
        failure: Option<HouseholdRepositoryError>,
    }

    impl FakeHouseholds {
        fn record(&self, access: &HouseholdAccessRequest) -> Result<(), HouseholdRepositoryError> {
            self.requests.lock().unwrap().push(access.clone());
            self.failure.clone().map_or(Ok(()), Err)
        }

        fn last_permission(&self) -> HouseholdPermission {
            self.requests.lock().unwrap().last().unwrap().permission()
        }
    }

    fn household() -> Household {
        Household::new(
            HouseholdId::try_new("h-1").unwrap(),
            UserId::try_new("owner").unwrap(),
            HouseholdName::try_new("Familie").unwrap(),
        )
    }

    impl HouseholdRepository for FakeHouseholds {
        async fn create_household(
            &self,
            household: Household,
        ) -> Result<(), HouseholdRepositoryError> {
            self.failure.clone().map_or(Ok(()), Err)?;
            self.created.lock().unwrap().push(household);
            Ok(())
        }

        async fn get_household(
            &self,
            access: &HouseholdAccessRequest,
            _: &HouseholdId,
        ) -> Result<HouseholdDetails, HouseholdRepositoryError> {
            self.record(access)?;
            Ok(HouseholdDetails {
                household: household(),
                members: Vec::new(),
            })
        }

        async fn rename_household(
            &self,
            access: &HouseholdAccessRequest,
            _: &HouseholdId,
            _: AggregateVersion,
            _: HouseholdName,
        ) -> Result<Household, HouseholdRepositoryError> {
            self.record(access)?;
            Ok(household())
        }

        async fn create_household_list(
            &self,
            access: &HouseholdAccessRequest,
            list: ShoppingList,
        ) -> Result<(), HouseholdRepositoryError> {
            self.record(access)?;
            self.lists.lock().unwrap().push(list);
            Ok(())
        }

        async fn remove_member(
            &self,
            access: &HouseholdAccessRequest,
            _: &HouseholdId,
            _: AggregateVersion,
            _: &UserId,
        ) -> Result<Household, HouseholdRepositoryError> {
            self.record(access)?;
            Ok(household())
        }

        async fn transfer_ownership(
            &self,
            access: &HouseholdAccessRequest,
            _: &HouseholdId,
            _: AggregateVersion,
            _: &UserId,
        ) -> Result<Household, HouseholdRepositoryError> {
            self.record(access)?;
            Ok(household())
        }

        async fn delete_household(
            &self,
            access: &HouseholdAccessRequest,
            _: &HouseholdId,
            _: AggregateVersion,
        ) -> Result<(), HouseholdRepositoryError> {
            self.record(access)
        }
    }

    struct FixedIds(&'static str);

    impl HouseholdIdGenerator for FixedIds {
        fn generate(&self) -> String {
            self.0.to_owned()
        }
    }

    impl ListIdGenerator for FixedIds {
        fn generate(&self) -> String {
            self.0.to_owned()
        }
    }

    #[test]
    fn create_household_makes_the_verified_actor_owner() {
        let repository = FakeHouseholds::default();
        let created = CreateHousehold::new(&repository, &FixedIds("h-1"))
            .execute(CreateHouseholdCommand {
                actor_id: "verified-sub".to_owned(),
                name: "  Familie ".to_owned(),
            })
            .block_on()
            .unwrap();
        assert_eq!(created.owner_id().as_str(), "verified-sub");
        assert_eq!(created.name().as_str(), "Familie");
        assert_eq!(created.version().as_u64(), 1);
        assert_eq!(repository.created.lock().unwrap().as_slice(), &[created]);
    }

    #[test]
    fn create_household_validates_before_persisting() {
        let repository = FakeHouseholds::default();
        let use_case = CreateHousehold::new(&repository, &FixedIds("h-1"));
        assert!(matches!(
            use_case
                .execute(CreateHouseholdCommand {
                    actor_id: " ".to_owned(),
                    name: "Familie".to_owned(),
                })
                .block_on(),
            Err(HouseholdError::InvalidActor(_))
        ));
        assert!(matches!(
            use_case
                .execute(CreateHouseholdCommand {
                    actor_id: "owner".to_owned(),
                    name: " ".to_owned(),
                })
                .block_on(),
            Err(HouseholdError::InvalidName(HouseholdNameError::Empty))
        ));
        assert!(matches!(
            CreateHousehold::new(&repository, &FixedIds(""))
                .execute(CreateHouseholdCommand {
                    actor_id: "owner".to_owned(),
                    name: "Familie".to_owned(),
                })
                .block_on(),
            Err(HouseholdError::InvalidGeneratedId(_))
        ));
        assert!(repository.created.lock().unwrap().is_empty());
    }

    #[test]
    fn repository_failures_are_propagated() {
        let repository = FakeHouseholds {
            failure: Some(HouseholdRepositoryError::HouseholdLimitReached),
            ..FakeHouseholds::default()
        };
        assert_eq!(
            CreateHousehold::new(&repository, &FixedIds("h-1"))
                .execute(CreateHouseholdCommand {
                    actor_id: "owner".to_owned(),
                    name: "Familie".to_owned(),
                })
                .block_on(),
            Err(HouseholdError::Repository(
                HouseholdRepositoryError::HouseholdLimitReached
            ))
        );
        assert_eq!(
            DeleteHousehold::new(&repository)
                .execute(DeleteHouseholdCommand {
                    actor_id: "owner".to_owned(),
                    household_id: "h-1".to_owned(),
                    expected_version: AggregateVersion::initial(),
                })
                .block_on(),
            Err(HouseholdError::Repository(
                HouseholdRepositoryError::HouseholdLimitReached
            ))
        );
    }

    #[test]
    fn use_cases_request_permissions_from_the_rule_table() {
        let repository = FakeHouseholds::default();
        GetHousehold::new(&repository)
            .execute(GetHouseholdQuery {
                actor_id: "member".to_owned(),
                household_id: "h-1".to_owned(),
            })
            .block_on()
            .unwrap();
        assert_eq!(
            repository.last_permission(),
            HouseholdPermission::Participate
        );
        assert_eq!(
            repository.requests.lock().unwrap()[0].principal().as_str(),
            "member"
        );

        RenameHousehold::new(&repository)
            .execute(RenameHouseholdCommand {
                actor_id: "member".to_owned(),
                household_id: "h-1".to_owned(),
                expected_version: AggregateVersion::initial(),
                name: "WG".to_owned(),
            })
            .block_on()
            .unwrap();
        assert_eq!(
            repository.last_permission(),
            HouseholdPermission::Administer
        );

        RemoveHouseholdMember::new(&repository)
            .execute(RemoveHouseholdMemberCommand {
                actor_id: "owner".to_owned(),
                household_id: "h-1".to_owned(),
                expected_version: AggregateVersion::initial(),
                member_id: "member".to_owned(),
            })
            .block_on()
            .unwrap();
        assert_eq!(
            repository.last_permission(),
            HouseholdPermission::Administer
        );

        TransferHouseholdOwnership::new(&repository)
            .execute(TransferHouseholdOwnershipCommand {
                actor_id: "owner".to_owned(),
                household_id: "h-1".to_owned(),
                expected_version: AggregateVersion::initial(),
                new_owner_id: "member".to_owned(),
            })
            .block_on()
            .unwrap();
        assert_eq!(
            repository.last_permission(),
            HouseholdPermission::Administer
        );

        DeleteHousehold::new(&repository)
            .execute(DeleteHouseholdCommand {
                actor_id: "owner".to_owned(),
                household_id: "h-1".to_owned(),
                expected_version: AggregateVersion::initial(),
            })
            .block_on()
            .unwrap();
        assert_eq!(
            repository.last_permission(),
            HouseholdPermission::Administer
        );
    }

    #[test]
    fn create_household_list_creates_a_household_owned_list_for_members() {
        let repository = FakeHouseholds::default();
        let list = CreateHouseholdList::new(&repository, &FixedIds("list-1"))
            .execute(CreateHouseholdListCommand {
                actor_id: "member".to_owned(),
                household_id: "h-1".to_owned(),
                name: "Wocheneinkauf".to_owned(),
            })
            .block_on()
            .unwrap();
        assert_eq!(
            list.owner(),
            &ListOwner::Household(HouseholdId::try_new("h-1").unwrap())
        );
        assert_eq!(
            repository.last_permission(),
            HouseholdPermission::Participate
        );
        assert_eq!(repository.lists.lock().unwrap().as_slice(), &[list]);
    }

    #[test]
    fn invalid_identifiers_are_rejected_before_the_repository() {
        let repository = FakeHouseholds::default();
        assert!(matches!(
            TransferHouseholdOwnership::new(&repository)
                .execute(TransferHouseholdOwnershipCommand {
                    actor_id: "owner".to_owned(),
                    household_id: "h-1".to_owned(),
                    expected_version: AggregateVersion::initial(),
                    new_owner_id: "".to_owned(),
                })
                .block_on(),
            Err(HouseholdError::InvalidMemberId(_))
        ));
        assert!(matches!(
            LeaveHousehold::new(&repository)
                .execute(LeaveHouseholdCommand {
                    actor_id: "member".to_owned(),
                    household_id: " ".to_owned(),
                })
                .block_on(),
            Err(HouseholdError::InvalidHouseholdId(_))
        ));
        assert!(repository.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn leave_household_fails_closed_on_the_default_port() {
        let repository = FakeHouseholds::default();
        assert_eq!(
            LeaveHousehold::new(&repository)
                .execute(LeaveHouseholdCommand {
                    actor_id: "member".to_owned(),
                    household_id: "h-1".to_owned(),
                })
                .block_on(),
            Err(HouseholdError::Repository(
                HouseholdRepositoryError::Unavailable
            ))
        );
        assert_eq!(
            ListHouseholds::new(&repository)
                .execute(ListHouseholdsQuery {
                    actor_id: "member".to_owned(),
                })
                .block_on(),
            Err(HouseholdError::Repository(
                HouseholdRepositoryError::Unavailable
            ))
        );
    }
}

//! Account deletion for the verified principal (roadmap D2, ADR 018).
//!
//! Policy:
//! - A household the principal owns **together with other active members** blocks the
//!   deletion (`OwnerMustTransferOwnership`) before anything is removed. The owner first
//!   transfers ownership or deletes the household; other members never lose shared data as a
//!   side effect of someone else's account deletion.
//! - A household in which the principal is the only member is deleted with its lists, items
//!   and invitations.
//! - Memberships in other households end (the principal leaves).
//! - Personal lists are deleted with their items.
//! - Invitations the principal created are removed in every household.
//! - The profile is removed last, so an interrupted deletion stays visible and can be retried.
//!
//! Every step is one atomic port operation guarded by the current expected version; steps
//! are idempotent (`NotFound` for an already removed record counts as done), so a deletion
//! interrupted by an outage or a concurrent change is completed by repeating the request.
//! Items the principal added to household lists stay with the household (shared content).

use einkaufio_domain::{
    Household, HouseholdOperation, HouseholdRole, ListOperation, ShoppingList, UserId,
};

use crate::household_ports::{
    HouseholdAccessRequest, HouseholdRepository, HouseholdRepositoryError, HouseholdSummary,
    InvitationRepository,
};
use crate::ports::{
    ListAccessRequest, ProfileRepository, RepositoryError, ShoppingItemRepository,
    ShoppingListRepository,
};

/// Errors of the account deletion and data export use cases.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountDataError {
    InvalidPrincipal,
    /// The principal owns a household that has other active members. Nothing was deleted.
    OwnerMustTransferOwnership,
    /// A record changed concurrently; repeating the request completes the deletion.
    Conflict,
    PersistenceUnavailable,
    RepositoryInvariantViolation,
}

pub(crate) fn account_principal(value: String) -> Result<UserId, AccountDataError> {
    UserId::try_new(value).map_err(|_| AccountDataError::InvalidPrincipal)
}

pub(crate) fn map_list_error(error: RepositoryError) -> AccountDataError {
    match error {
        RepositoryError::Unavailable => AccountDataError::PersistenceUnavailable,
        RepositoryError::Conflict | RepositoryError::VersionExhausted => AccountDataError::Conflict,
        RepositoryError::NotFound
        | RepositoryError::Forbidden
        | RepositoryError::AlreadyExists
        | RepositoryError::ItemLimitReached
        | RepositoryError::InvalidCursor => AccountDataError::RepositoryInvariantViolation,
    }
}

pub(crate) fn map_household_error(error: HouseholdRepositoryError) -> AccountDataError {
    match error {
        HouseholdRepositoryError::Unavailable => AccountDataError::PersistenceUnavailable,
        // The principal's role or the household changed between reading and writing.
        HouseholdRepositoryError::Conflict
        | HouseholdRepositoryError::VersionExhausted
        | HouseholdRepositoryError::Forbidden
        | HouseholdRepositoryError::OwnerMustTransferOwnership => AccountDataError::Conflict,
        HouseholdRepositoryError::AlreadyExists
        | HouseholdRepositoryError::NotFound
        | HouseholdRepositoryError::InvalidRequest
        | HouseholdRepositoryError::HouseholdLimitReached
        | HouseholdRepositoryError::MemberLimitReached
        | HouseholdRepositoryError::ListLimitReached
        | HouseholdRepositoryError::InvitationLimitReached
        | HouseholdRepositoryError::InvitationUnavailable
        | HouseholdRepositoryError::AlreadyMember => AccountDataError::RepositoryInvariantViolation,
    }
}

/// Treats an already removed record as a completed step.
fn ignore_not_found<E: PartialEq>(result: Result<(), E>, not_found: E) -> Result<(), E> {
    match result {
        Err(error) if error == not_found => Ok(()),
        other => other,
    }
}

/// What a completed account deletion removed.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AccountDeletion {
    pub deleted_households: usize,
    pub left_households: usize,
    pub deleted_lists: usize,
}

/// Ports the account use cases need; satisfied by one persistence adapter.
pub trait AccountRepositories:
    ShoppingListRepository
    + ShoppingItemRepository
    + ProfileRepository
    + HouseholdRepository
    + InvitationRepository
{
}

impl<T> AccountRepositories for T where
    T: ShoppingListRepository
        + ShoppingItemRepository
        + ProfileRepository
        + HouseholdRepository
        + InvitationRepository
{
}

pub struct DeleteAccount<'a, R> {
    repository: &'a R,
}

impl<'a, R: AccountRepositories> DeleteAccount<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    pub async fn execute(&self, principal_id: String) -> Result<AccountDeletion, AccountDataError> {
        let principal = account_principal(principal_id)?;
        let households = self
            .repository
            .list_households(&principal)
            .await
            .map_err(map_household_error)?;
        let owned = self
            .owned_sole_member_households(&principal, &households)
            .await?;

        let mut deletion = AccountDeletion::default();
        let administer =
            HouseholdAccessRequest::for_operation(principal.clone(), HouseholdOperation::Delete);
        for household in &owned {
            ignore_not_found(
                self.repository
                    .delete_household(&administer, household.id(), household.version())
                    .await,
                HouseholdRepositoryError::NotFound,
            )
            .map_err(map_household_error)?;
            deletion.deleted_households += 1;
        }
        for summary in households
            .iter()
            .filter(|summary| summary.role == HouseholdRole::Member)
        {
            ignore_not_found(
                self.repository
                    .leave_household(&principal, summary.household.id())
                    .await
                    .map(|_| ()),
                HouseholdRepositoryError::NotFound,
            )
            .map_err(map_household_error)?;
            deletion.left_households += 1;
        }
        deletion.deleted_lists = self.delete_personal_lists(&principal).await?;
        self.repository
            .delete_invitations_created_by(&principal)
            .await
            .map_err(map_household_error)?;
        ignore_not_found(
            self.repository.delete_profile(&principal).await,
            RepositoryError::NotFound,
        )
        .map_err(map_list_error)?;
        Ok(deletion)
    }

    /// Checks every owned household before anything is deleted: any owned household with
    /// other active members blocks the deletion.
    async fn owned_sole_member_households(
        &self,
        principal: &UserId,
        households: &[HouseholdSummary],
    ) -> Result<Vec<Household>, AccountDataError> {
        let view =
            HouseholdAccessRequest::for_operation(principal.clone(), HouseholdOperation::View);
        let mut owned = Vec::new();
        for summary in households
            .iter()
            .filter(|summary| summary.role == HouseholdRole::Owner)
        {
            let details = self
                .repository
                .get_household(&view, summary.household.id())
                .await
                .map_err(map_household_error)?;
            if details.household.owner_id() != principal {
                return Err(AccountDataError::Conflict);
            }
            if details
                .members
                .iter()
                .any(|member| &member.user_id != principal)
            {
                return Err(AccountDataError::OwnerMustTransferOwnership);
            }
            owned.push(details.household);
        }
        Ok(owned)
    }

    async fn delete_personal_lists(&self, principal: &UserId) -> Result<usize, AccountDataError> {
        let lists: Vec<ShoppingList> = self
            .repository
            .list_accessible(principal)
            .await
            .map_err(map_list_error)?
            .into_iter()
            .filter(|list| list.personal_owner() == Some(principal))
            .collect();
        let access = ListAccessRequest::for_operation(principal.clone(), ListOperation::Delete);
        for list in &lists {
            ignore_not_found(
                self.repository
                    .delete_list(&access, list.id(), list.aggregate_version())
                    .await,
                RepositoryError::NotFound,
            )
            .map_err(map_list_error)?;
        }
        Ok(lists.len())
    }
}

#[cfg(test)]
mod tests {
    use einkaufio_domain::UserProfile;

    use super::*;
    use crate::test_support::BlockOn;

    struct Unimplemented;

    impl ShoppingListRepository for Unimplemented {
        async fn create_personal(&self, _: ShoppingList) -> Result<(), RepositoryError> {
            Err(RepositoryError::Unavailable)
        }
    }

    impl ShoppingItemRepository for Unimplemented {
        async fn add_item(
            &self,
            _: &ListAccessRequest,
            _: &einkaufio_domain::ListId,
            _: einkaufio_domain::AggregateVersion,
            _: einkaufio_domain::ShoppingItem,
        ) -> Result<einkaufio_domain::AggregateVersion, RepositoryError> {
            Err(RepositoryError::Unavailable)
        }
        async fn set_item_checked(
            &self,
            _: &ListAccessRequest,
            _: &einkaufio_domain::ListId,
            _: &einkaufio_domain::ItemId,
            _: u64,
            _: bool,
        ) -> Result<
            (
                einkaufio_domain::ShoppingItem,
                einkaufio_domain::AggregateVersion,
            ),
            RepositoryError,
        > {
            Err(RepositoryError::Unavailable)
        }
        async fn delete_item(
            &self,
            _: &ListAccessRequest,
            _: &einkaufio_domain::ListId,
            _: &einkaufio_domain::ItemId,
            _: u64,
        ) -> Result<einkaufio_domain::AggregateVersion, RepositoryError> {
            Err(RepositoryError::Unavailable)
        }
    }

    impl ProfileRepository for Unimplemented {
        async fn create_profile(&self, _: UserProfile) -> Result<(), RepositoryError> {
            Err(RepositoryError::Unavailable)
        }
        async fn get_profile(&self, _: &UserId) -> Result<UserProfile, RepositoryError> {
            Err(RepositoryError::Unavailable)
        }
        async fn update_profile(
            &self,
            _: &UserId,
            _: einkaufio_domain::AggregateVersion,
            _: einkaufio_domain::DisplayName,
        ) -> Result<UserProfile, RepositoryError> {
            Err(RepositoryError::Unavailable)
        }
    }

    impl HouseholdRepository for Unimplemented {}
    impl InvitationRepository for Unimplemented {}

    #[test]
    fn deletion_fails_closed_on_default_ports() {
        assert_eq!(
            DeleteAccount::new(&Unimplemented)
                .execute("user".to_owned())
                .block_on(),
            Err(AccountDataError::PersistenceUnavailable)
        );
    }

    #[test]
    fn deletion_rejects_an_invalid_principal_before_any_port_call() {
        assert_eq!(
            DeleteAccount::new(&Unimplemented)
                .execute(" ".to_owned())
                .block_on(),
            Err(AccountDataError::InvalidPrincipal)
        );
    }

    #[test]
    fn repository_errors_map_to_retryable_or_invariant_errors() {
        assert_eq!(
            map_household_error(HouseholdRepositoryError::OwnerMustTransferOwnership),
            AccountDataError::Conflict
        );
        assert_eq!(
            map_household_error(HouseholdRepositoryError::AlreadyMember),
            AccountDataError::RepositoryInvariantViolation
        );
        assert_eq!(
            map_list_error(RepositoryError::VersionExhausted),
            AccountDataError::Conflict
        );
        assert_eq!(
            map_list_error(RepositoryError::Forbidden),
            AccountDataError::RepositoryInvariantViolation
        );
        assert_eq!(ignore_not_found(Err(1), 1), Ok(()));
        assert_eq!(ignore_not_found(Err(2), 1), Err(2));
    }
}

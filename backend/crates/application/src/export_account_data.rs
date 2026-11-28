//! Personal data export for the verified principal (roadmap D2, ADR 018).
//!
//! The export contains the profile (if any), the principal's household memberships and every
//! list the principal can currently access (personal and household lists) with its items.
//! Lists and items are read through the same access-scoped ports as every other request, so
//! the export never contains data the principal could not read anyway. The export is not one
//! consistent snapshot: it is assembled from several reads, and a list whose access ends
//! between the reads is left out.

use einkaufio_domain::{ListOperation, ShoppingItem, ShoppingList, UserId, UserProfile};

use crate::delete_account::{
    AccountDataError, AccountRepositories, account_principal, map_household_error, map_list_error,
};
use crate::household_ports::HouseholdSummary;
use crate::ports::{ListAccessRequest, RepositoryError};

/// One accessible list with its items in position order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportedList {
    pub list: ShoppingList,
    pub items: Vec<ShoppingItem>,
}

/// Everything the service stores about or for the principal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountDataExport {
    pub principal: UserId,
    pub profile: Option<UserProfile>,
    /// Memberships, ordered by household id.
    pub households: Vec<HouseholdSummary>,
    /// Accessible lists, ordered by list id.
    pub lists: Vec<ExportedList>,
}

pub struct ExportAccountData<'a, R> {
    repository: &'a R,
}

impl<'a, R: AccountRepositories> ExportAccountData<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    pub async fn execute(
        &self,
        principal_id: String,
    ) -> Result<AccountDataExport, AccountDataError> {
        let principal = account_principal(principal_id)?;
        let profile = match self.repository.get_profile(&principal).await {
            Ok(profile) if profile.user_id() == &principal => Some(profile),
            Ok(_) => return Err(AccountDataError::RepositoryInvariantViolation),
            Err(RepositoryError::NotFound) => None,
            Err(error) => return Err(map_list_error(error)),
        };
        let mut households = self
            .repository
            .list_households(&principal)
            .await
            .map_err(map_household_error)?;
        households.sort_by(|left, right| {
            left.household
                .id()
                .as_str()
                .cmp(right.household.id().as_str())
        });
        let mut accessible = self
            .repository
            .list_accessible(&principal)
            .await
            .map_err(map_list_error)?;
        accessible.sort_by(|left, right| left.id().as_str().cmp(right.id().as_str()));

        let reader = ListAccessRequest::for_operation(principal.clone(), ListOperation::ReadItems);
        let mut lists = Vec::with_capacity(accessible.len());
        for list in accessible {
            match self.repository.list_items(&reader, list.id()).await {
                Ok(items) => lists.push(ExportedList { list, items }),
                // Access ended after the listing: the list is no longer the principal's data.
                Err(RepositoryError::NotFound) => {}
                Err(error) => return Err(map_list_error(error)),
            }
        }
        Ok(AccountDataExport {
            principal,
            profile,
            households,
            lists,
        })
    }
}

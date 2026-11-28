//! HTTP transport for account deletion and personal data export (roadmap D2, ADR 018).

use axum::{
    Json,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use einkaufio_application::{
    AccountDataError, AccountDataExport, DeleteAccount, ExportAccountData, ExportedList, ListOwner,
    ShoppingItem,
};
use serde::Serialize;

use super::households::{HouseholdResponse, no_store};
use super::timestamps::rfc3339;
use super::{
    ApiError, ApiState, ItemResponse, ProfileResponse, Repositories, VerifiedActor, principal,
};

/// Export format version; bump on incompatible changes to the document shape.
const EXPORT_FORMAT_VERSION: u32 = 1;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportedItemResponse {
    #[serde(flatten)]
    item: ItemResponse,
    created_by: String,
}

impl From<&ShoppingItem> for ExportedItemResponse {
    fn from(item: &ShoppingItem) -> Self {
        Self {
            item: ItemResponse::from(item),
            created_by: item.created_by().as_str().to_owned(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportedListResponse {
    id: String,
    name: String,
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    household_id: Option<String>,
    version: u64,
    items: Vec<ExportedItemResponse>,
}

impl From<&ExportedList> for ExportedListResponse {
    fn from(exported: &ExportedList) -> Self {
        let list = &exported.list;
        let (kind, household_id) = match list.owner() {
            ListOwner::Personal(_) => ("PERSONAL", None),
            ListOwner::Household(id) => ("HOUSEHOLD", Some(id.as_str().to_owned())),
        };
        Self {
            id: list.id().as_str().to_owned(),
            name: list.name().as_str().to_owned(),
            kind,
            household_id,
            version: list.version(),
            items: exported
                .items
                .iter()
                .map(ExportedItemResponse::from)
                .collect(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountExportResponse {
    format_version: u32,
    exported_at: String,
    user_id: String,
    profile: Option<ProfileResponse>,
    households: Vec<HouseholdResponse>,
    lists: Vec<ExportedListResponse>,
}

impl AccountExportResponse {
    fn new(export: &AccountDataExport, exported_at: String) -> Self {
        Self {
            format_version: EXPORT_FORMAT_VERSION,
            exported_at,
            user_id: export.principal.as_str().to_owned(),
            profile: export.profile.as_ref().map(ProfileResponse::from),
            households: export
                .households
                .iter()
                .map(HouseholdResponse::from)
                .collect(),
            lists: export
                .lists
                .iter()
                .map(ExportedListResponse::from)
                .collect(),
        }
    }
}

/// `DELETE /api/me`: deletes the caller's application data (ADR 018 policy).
pub(super) async fn delete_account<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
) -> Result<StatusCode, ApiError> {
    DeleteAccount::new(state.repositories.as_ref())
        .execute(principal(&actor))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/me/export`: the caller's personal data as a JSON download.
pub(super) async fn export_account<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
) -> Result<Response, ApiError> {
    let export = ExportAccountData::new(state.repositories.as_ref())
        .execute(principal(&actor))
        .await?;
    let body = AccountExportResponse::new(&export, rfc3339(state.households.clock.now()));
    let mut response = no_store(Json(body).into_response());
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"einkaufio-export.json\""),
    );
    Ok(response)
}

impl From<AccountDataError> for ApiError {
    fn from(error: AccountDataError) -> Self {
        match error {
            AccountDataError::InvalidPrincipal | AccountDataError::RepositoryInvariantViolation => {
                Self::Internal
            }
            AccountDataError::OwnerMustTransferOwnership => Self::OwnerMustTransferOwnership,
            AccountDataError::Conflict => Self::Conflict,
            AccountDataError::PersistenceUnavailable => Self::Unavailable,
        }
    }
}

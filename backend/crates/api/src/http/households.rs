//! HTTP transport for households, membership lifecycle and invitations (roadmap C2-C4).
//!
//! Handlers stay thin: parse transport input, take the verified actor, call one use case and
//! map its result. Authorization happens in the persistence adapter through the household
//! access request built by the use case (ADR 015, ADR 017).

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use einkaufio_application::{
    AcceptHouseholdInvitation, AcceptHouseholdInvitationCommand, CreateHousehold,
    CreateHouseholdCommand, CreateHouseholdInvitation, CreateHouseholdInvitationCommand,
    CreateHouseholdList, CreateHouseholdListCommand, DeleteHousehold, DeleteHouseholdCommand,
    GetHousehold, GetHouseholdQuery, Household, HouseholdDetails, HouseholdError, HouseholdMember,
    HouseholdRepositoryError, HouseholdRole, HouseholdSummary, Invitation, InvitationServices,
    InvitationUseError, LeaveHousehold, LeaveHouseholdCommand, ListHouseholdInvitations,
    ListHouseholdInvitationsQuery, ListHouseholds, ListHouseholdsQuery, RemoveHouseholdMember,
    RemoveHouseholdMemberCommand, RenameHousehold, RenameHouseholdCommand,
    RevokeHouseholdInvitation, RevokeHouseholdInvitationCommand, Timestamp,
    TransferHouseholdOwnership, TransferHouseholdOwnershipCommand, UserId,
};
use serde::{Deserialize, Serialize};

use super::timestamps::rfc3339;
use super::{
    ApiError, ApiState, ExpectedVersionQuery, ListResponse, Repositories, VerifiedActor,
    expected_version, principal,
};

const SECONDS_PER_DAY: u64 = 86_400;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CreateHouseholdRequest {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RenameHouseholdRequest {
    name: String,
    expected_version: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CreateHouseholdListRequest {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct TransferOwnershipRequest {
    new_owner_id: String,
    expected_version: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CreateInvitationRequest {
    max_uses: Option<u32>,
    expires_in_days: Option<u64>,
}

/// Carries the bearer secret. Deliberately not `Debug`, so it cannot be formatted into logs.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AcceptInvitationRequest {
    token: String,
}

fn role_name(role: HouseholdRole) -> &'static str {
    match role {
        HouseholdRole::Owner => "OWNER",
        HouseholdRole::Member => "MEMBER",
    }
}

/// A household from the caller's point of view.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HouseholdResponse {
    id: String,
    name: String,
    role: &'static str,
    version: u64,
}

impl HouseholdResponse {
    pub(super) fn new(household: &Household, role: HouseholdRole) -> Self {
        Self {
            id: household.id().as_str().to_owned(),
            name: household.name().as_str().to_owned(),
            role: role_name(role),
            version: household.version().as_u64(),
        }
    }

    /// The caller's role is derived from the household returned by the operation. Callers
    /// that reach this point are active members, which the adapter has just verified.
    fn for_actor(household: &Household, actor: &VerifiedActor) -> Result<Self, ApiError> {
        let user = UserId::try_new(actor.principal_id()).map_err(|_| ApiError::Internal)?;
        Ok(Self::new(household, household.role_of_member(&user)))
    }
}

impl From<&HouseholdSummary> for HouseholdResponse {
    fn from(summary: &HouseholdSummary) -> Self {
        Self::new(&summary.household, summary.role)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HouseholdListResponse {
    items: Vec<HouseholdResponse>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MemberResponse {
    user_id: String,
    role: &'static str,
}

impl From<&HouseholdMember> for MemberResponse {
    fn from(member: &HouseholdMember) -> Self {
        Self {
            user_id: member.user_id.as_str().to_owned(),
            role: role_name(member.role),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HouseholdDetailsResponse {
    #[serde(flatten)]
    household: HouseholdResponse,
    members: Vec<MemberResponse>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MemberListResponse {
    items: Vec<MemberResponse>,
}

/// The one-time creation response. Contains the bearer secret twice (token and URL).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CreatedInvitationResponse {
    invite_id: String,
    token: String,
    invite_url: String,
    expires_at: String,
    max_uses: u32,
}

/// An invitation as listed to the owner. Never contains the token or its hash.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct InvitationResponse {
    invite_id: String,
    created_by: String,
    expires_at: String,
    max_uses: u32,
    used_count: u32,
    status: &'static str,
}

impl InvitationResponse {
    fn new(invitation: &Invitation, now: Timestamp) -> Self {
        let status = match invitation.check_usable(now) {
            Ok(()) => "ACTIVE",
            Err(InvitationUseError::Revoked) => "REVOKED",
            Err(InvitationUseError::Expired) => "EXPIRED",
            Err(InvitationUseError::Exhausted) => "USED_UP",
        };
        Self {
            invite_id: invitation.id().as_str().to_owned(),
            created_by: invitation.created_by().as_str().to_owned(),
            expires_at: rfc3339(invitation.expires_at()),
            max_uses: invitation.max_uses().get(),
            used_count: invitation.used_count(),
            status,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct InvitationListResponse {
    items: Vec<InvitationResponse>,
}

/// Responses carrying secrets or personal data must not be stored by caches.
pub(super) fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

pub(super) async fn list_households<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
) -> Result<Json<HouseholdListResponse>, ApiError> {
    let summaries = ListHouseholds::new(state.repositories.as_ref())
        .execute(ListHouseholdsQuery {
            actor_id: principal(&actor),
        })
        .await?;
    Ok(Json(HouseholdListResponse {
        items: summaries.iter().map(HouseholdResponse::from).collect(),
    }))
}

pub(super) async fn create_household<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Json(payload): Json<CreateHouseholdRequest>,
) -> Result<(StatusCode, Json<HouseholdResponse>), ApiError> {
    let household = CreateHousehold::new(
        state.repositories.as_ref(),
        state.households.household_ids.as_ref(),
    )
    .execute(CreateHouseholdCommand {
        actor_id: principal(&actor),
        name: payload.name,
    })
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(HouseholdResponse::new(&household, HouseholdRole::Owner)),
    ))
}

async fn household_details<R: Repositories>(
    state: &ApiState<R>,
    actor: &VerifiedActor,
    household_id: String,
) -> Result<HouseholdDetails, ApiError> {
    Ok(GetHousehold::new(state.repositories.as_ref())
        .execute(GetHouseholdQuery {
            actor_id: principal(actor),
            household_id,
        })
        .await?)
}

pub(super) async fn get_household<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Path(household_id): Path<String>,
) -> Result<Json<HouseholdDetailsResponse>, ApiError> {
    let details = household_details(&state, &actor, household_id).await?;
    Ok(Json(HouseholdDetailsResponse {
        household: HouseholdResponse::for_actor(&details.household, &actor)?,
        members: details.members.iter().map(MemberResponse::from).collect(),
    }))
}

pub(super) async fn list_members<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Path(household_id): Path<String>,
) -> Result<Json<MemberListResponse>, ApiError> {
    let details = household_details(&state, &actor, household_id).await?;
    Ok(Json(MemberListResponse {
        items: details.members.iter().map(MemberResponse::from).collect(),
    }))
}

pub(super) async fn rename_household<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Path(household_id): Path<String>,
    Json(payload): Json<RenameHouseholdRequest>,
) -> Result<Json<HouseholdResponse>, ApiError> {
    let household = RenameHousehold::new(state.repositories.as_ref())
        .execute(RenameHouseholdCommand {
            actor_id: principal(&actor),
            household_id,
            expected_version: expected_version(payload.expected_version)?,
            name: payload.name,
        })
        .await?;
    Ok(Json(HouseholdResponse::for_actor(&household, &actor)?))
}

pub(super) async fn delete_household<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Path(household_id): Path<String>,
    Query(query): Query<ExpectedVersionQuery>,
) -> Result<StatusCode, ApiError> {
    DeleteHousehold::new(state.repositories.as_ref())
        .execute(DeleteHouseholdCommand {
            actor_id: principal(&actor),
            household_id,
            expected_version: expected_version(query.expected_version)?,
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn create_household_list<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Path(household_id): Path<String>,
    Json(payload): Json<CreateHouseholdListRequest>,
) -> Result<(StatusCode, Json<ListResponse>), ApiError> {
    let list = CreateHouseholdList::new(state.repositories.as_ref(), state.list_ids.as_ref())
        .execute(CreateHouseholdListCommand {
            actor_id: principal(&actor),
            household_id,
            name: payload.name,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(ListResponse::from(&list))))
}

pub(super) async fn remove_member<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Path((household_id, member_id)): Path<(String, String)>,
    Query(query): Query<ExpectedVersionQuery>,
) -> Result<Json<HouseholdResponse>, ApiError> {
    let household = RemoveHouseholdMember::new(state.repositories.as_ref())
        .execute(RemoveHouseholdMemberCommand {
            actor_id: principal(&actor),
            household_id,
            expected_version: expected_version(query.expected_version)?,
            member_id,
        })
        .await?;
    Ok(Json(HouseholdResponse::for_actor(&household, &actor)?))
}

pub(super) async fn leave_household<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Path(household_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    LeaveHousehold::new(state.repositories.as_ref())
        .execute(LeaveHouseholdCommand {
            actor_id: principal(&actor),
            household_id,
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn transfer_ownership<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Path(household_id): Path<String>,
    Json(payload): Json<TransferOwnershipRequest>,
) -> Result<Json<HouseholdResponse>, ApiError> {
    let household = TransferHouseholdOwnership::new(state.repositories.as_ref())
        .execute(TransferHouseholdOwnershipCommand {
            actor_id: principal(&actor),
            household_id,
            expected_version: expected_version(payload.expected_version)?,
            new_owner_id: payload.new_owner_id,
        })
        .await?;
    Ok(Json(HouseholdResponse::for_actor(&household, &actor)?))
}

pub(super) async fn create_invitation<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Path(household_id): Path<String>,
    Json(payload): Json<CreateInvitationRequest>,
) -> Result<Response, ApiError> {
    let lifetime_seconds = payload
        .expires_in_days
        .map(|days| {
            days.checked_mul(SECONDS_PER_DAY)
                .ok_or(ApiError::InvalidInput)
        })
        .transpose()?;
    let services = state.households.as_ref();
    let created = CreateHouseholdInvitation::new(
        state.repositories.as_ref(),
        InvitationServices {
            tokens: services.tokens.as_ref(),
            hasher: services.hasher.as_ref(),
            ids: services.invitation_ids.as_ref(),
            clock: services.clock.as_ref(),
        },
    )
    .execute(CreateHouseholdInvitationCommand {
        actor_id: principal(&actor),
        household_id,
        lifetime_seconds,
        max_uses: payload.max_uses,
    })
    .await?;
    let body = CreatedInvitationResponse {
        invite_id: created.invitation.id().as_str().to_owned(),
        token: created.token.expose_secret().to_owned(),
        invite_url: services.invitation_base_url.invitation_url(&created.token),
        expires_at: rfc3339(created.invitation.expires_at()),
        max_uses: created.invitation.max_uses().get(),
    };
    Ok(no_store((StatusCode::CREATED, Json(body)).into_response()))
}

pub(super) async fn list_invitations<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Path(household_id): Path<String>,
) -> Result<Json<InvitationListResponse>, ApiError> {
    let invitations = ListHouseholdInvitations::new(state.repositories.as_ref())
        .execute(ListHouseholdInvitationsQuery {
            actor_id: principal(&actor),
            household_id,
        })
        .await?;
    let now = state.households.clock.now();
    Ok(Json(InvitationListResponse {
        items: invitations
            .iter()
            .map(|invitation| InvitationResponse::new(invitation, now))
            .collect(),
    }))
}

pub(super) async fn revoke_invitation<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Path((household_id, invitation_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    RevokeHouseholdInvitation::new(state.repositories.as_ref())
        .execute(RevokeHouseholdInvitationCommand {
            actor_id: principal(&actor),
            household_id,
            invitation_id,
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn accept_invitation<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Json(payload): Json<AcceptInvitationRequest>,
) -> Result<Json<HouseholdResponse>, ApiError> {
    let services = state.households.as_ref();
    let summary = AcceptHouseholdInvitation::new(
        state.repositories.as_ref(),
        services.hasher.as_ref(),
        services.clock.as_ref(),
    )
    .execute(AcceptHouseholdInvitationCommand {
        actor_id: principal(&actor),
        token: payload.token,
    })
    .await?;
    Ok(Json(HouseholdResponse::from(&summary)))
}

impl From<HouseholdRepositoryError> for ApiError {
    fn from(error: HouseholdRepositoryError) -> Self {
        match error {
            HouseholdRepositoryError::NotFound => Self::NotFound,
            HouseholdRepositoryError::Forbidden => Self::Forbidden,
            HouseholdRepositoryError::Conflict | HouseholdRepositoryError::VersionExhausted => {
                Self::Conflict
            }
            HouseholdRepositoryError::Unavailable => Self::Unavailable,
            // Generated-id collisions and port precondition violations are server faults.
            HouseholdRepositoryError::AlreadyExists | HouseholdRepositoryError::InvalidRequest => {
                Self::Internal
            }
            HouseholdRepositoryError::HouseholdLimitReached => Self::HouseholdLimitReached,
            HouseholdRepositoryError::MemberLimitReached => Self::MemberLimitReached,
            HouseholdRepositoryError::ListLimitReached => Self::ListLimitReached,
            HouseholdRepositoryError::InvitationLimitReached => Self::InvitationLimitReached,
            HouseholdRepositoryError::InvitationUnavailable => Self::InvitationUnavailable,
            HouseholdRepositoryError::AlreadyMember => Self::AlreadyMember,
            HouseholdRepositoryError::OwnerMustTransferOwnership => {
                Self::OwnerMustTransferOwnership
            }
        }
    }
}

impl From<HouseholdError> for ApiError {
    fn from(error: HouseholdError) -> Self {
        match error {
            HouseholdError::InvalidActor(_)
            | HouseholdError::InvalidGeneratedId(_)
            | HouseholdError::InvalidGeneratedToken
            | HouseholdError::ExpiryOverflow => Self::Internal,
            // A malformed id cannot name a household, member or invitation the caller can see.
            HouseholdError::InvalidHouseholdId(_)
            | HouseholdError::InvalidMemberId(_)
            | HouseholdError::InvalidInvitationId(_) => Self::NotFound,
            HouseholdError::InvalidName(_)
            | HouseholdError::InvalidListName(_)
            | HouseholdError::InvalidLifetime(_)
            | HouseholdError::InvalidMaxUses(_) => Self::InvalidInput,
            // Indistinguishable from an unknown, revoked, expired or used-up token (ADR 017).
            HouseholdError::InvalidInvitationToken => Self::InvitationUnavailable,
            HouseholdError::TokenGenerationFailed => Self::Unavailable,
            HouseholdError::Repository(error) => error.into(),
        }
    }
}

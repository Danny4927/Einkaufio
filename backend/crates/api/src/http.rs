//! HTTP transport for shopping lists, items, sync and the caller's profile.

use std::sync::Arc;

use crate::auth::{AuthError, CognitoTokenVerifier};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, FromRequestParts, Path, Query, State},
    http::{StatusCode, request::Parts},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, patch, put},
};
use einkaufio_application::{
    AddShoppingItem, AddShoppingItemCommand, AggregateVersion, CreatePersonalList,
    CreatePersonalListCommand, CreatePersonalListError, CreateProfile, DeleteShoppingItem,
    DeleteShoppingItemCommand, DeleteShoppingList, DeleteShoppingListCommand,
    DeleteShoppingListError, GetProfile, ItemIdGenerator, ListIdGenerator, ListQueryError,
    ListSync, Page, PageAccessibleLists, PageParameters, PageShoppingItems, ProfileError,
    ProfileRepository, RateLimitDecision, RateLimiter, RenameShoppingList,
    RenameShoppingListCommand, RenameShoppingListError, ReorderShoppingItems,
    ReorderShoppingItemsCommand, RepositoryError, SetShoppingItemChecked,
    SetShoppingItemCheckedCommand, ShoppingItem, ShoppingItemError, ShoppingItemRepository,
    ShoppingList, ShoppingListRepository, SyncShoppingList, UpdateProfile, UpdateShoppingItem,
    UpdateShoppingItemCommand, UserId, UserProfile,
};
use serde::{Deserialize, Serialize};

/// Maximum accepted request body size. Larger bodies are rejected with 413 before parsing.
pub const MAX_REQUEST_BODY_BYTES: usize = 16 * 1024;

/// Repository ports the HTTP adapter needs, satisfied by one persistence adapter.
pub trait Repositories:
    ShoppingListRepository + ShoppingItemRepository + ProfileRepository + 'static
{
}

impl<T> Repositories for T where
    T: ShoppingListRepository + ShoppingItemRepository + ProfileRepository + 'static
{
}

/// Shared handler state, generic over the persistence adapter.
///
/// Repository ports use `async fn` in traits and are therefore statically dispatched
/// (ADR 014); the composition root picks the concrete adapter type.
pub struct ApiState<R> {
    repositories: Arc<R>,
    list_ids: Arc<dyn ListIdGenerator>,
    item_ids: Arc<dyn ItemIdGenerator>,
    rate_limiter: Arc<dyn RateLimiter>,
    verifier: Option<Arc<CognitoTokenVerifier>>,
}

impl<R> Clone for ApiState<R> {
    fn clone(&self) -> Self {
        Self {
            repositories: Arc::clone(&self.repositories),
            list_ids: Arc::clone(&self.list_ids),
            item_ids: Arc::clone(&self.item_ids),
            rate_limiter: Arc::clone(&self.rate_limiter),
            verifier: self.verifier.clone(),
        }
    }
}

impl<R: Repositories> ApiState<R> {
    pub fn new(
        repositories: Arc<R>,
        list_ids: Arc<dyn ListIdGenerator>,
        item_ids: Arc<dyn ItemIdGenerator>,
        rate_limiter: Arc<dyn RateLimiter>,
    ) -> Self {
        Self {
            repositories,
            list_ids,
            item_ids,
            rate_limiter,
            verifier: None,
        }
    }

    pub fn with_authenticator(mut self, verifier: CognitoTokenVerifier) -> Self {
        self.verifier = Some(Arc::new(verifier));
        self
    }

    /// Build an authenticated API state from trusted runtime environment variables.
    pub fn from_cognito_env(
        repositories: Arc<R>,
        list_ids: Arc<dyn ListIdGenerator>,
        item_ids: Arc<dyn ItemIdGenerator>,
        rate_limiter: Arc<dyn RateLimiter>,
    ) -> Result<Self, AuthError> {
        let config = crate::auth::CognitoConfig::from_env()?;
        let verifier = CognitoTokenVerifier::new(config)?;
        Ok(Self::new(repositories, list_ids, item_ids, rate_limiter).with_authenticator(verifier))
    }
}

/// Build the HTTP router. This function does not read the environment: the composition root
/// must construct the state with [`ApiState::from_cognito_env`] and abort startup on error.
/// A state without a verifier keeps protected routes fail-closed (503).
///
/// Protected routes authenticate first, then throttle per verified principal.
pub fn router<R: Repositories>(state: ApiState<R>) -> Router {
    let protected = protected_routes::<R>()
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            throttle_principal::<R>,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_verified_actor::<R>,
        ));
    build_router(state, protected)
}

fn protected_routes<R: Repositories>() -> Router<ApiState<R>> {
    Router::new()
        .route(
            "/lists",
            get(list_accessible_lists::<R>).post(create_personal_list::<R>),
        )
        .route(
            "/lists/{list_id}",
            patch(rename_list::<R>).delete(delete_list::<R>),
        )
        .route(
            "/lists/{list_id}/items",
            get(page_items::<R>).post(add_item::<R>),
        )
        .route(
            "/lists/{list_id}/items/{item_id}",
            patch(update_item::<R>).delete(delete_item::<R>),
        )
        .route("/lists/{list_id}/item-order", put(reorder_items::<R>))
        .route("/lists/{list_id}/sync", get(sync_list::<R>))
        .route("/me", get(get_profile::<R>).put(put_profile::<R>))
}

fn build_router<R: Repositories>(state: ApiState<R>, protected: Router<ApiState<R>>) -> Router {
    Router::new()
        .route("/health", get(health))
        .nest("/api", protected)
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

/// Constructible only inside this crate from a cryptographically verified Cognito subject.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VerifiedActor {
    principal_id: String,
}

impl VerifiedActor {
    pub(crate) fn from_verified_claims(principal_id: String) -> Self {
        Self { principal_id }
    }

    fn principal_id(&self) -> &str {
        &self.principal_id
    }
}

impl<S: Send + Sync> FromRequestParts<S> for VerifiedActor {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Self>()
            .cloned()
            // Reaching a handler without the auth middleware is a wiring bug, not a client error.
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

async fn require_verified_actor<R: Repositories>(
    State(state): State<ApiState<R>>,
    mut request: axum::extract::Request,
    next: Next,
) -> Response {
    let Some(verifier) = state.verifier else {
        return ApiError::AuthenticationUnavailable.into_response();
    };

    let token = match request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|header| header.to_str().ok())
        .and_then(|value| {
            value
                .get(..7)
                .filter(|scheme| scheme.eq_ignore_ascii_case("Bearer "))
                .map(|_| &value[7..])
        })
        .filter(|value| !value.is_empty())
    {
        Some(token) => token,
        None => return ApiError::Unauthorized.into_response(),
    };

    match verifier.verify(token).await {
        Ok(principal) => {
            request
                .extensions_mut()
                .insert(VerifiedActor::from_verified_claims(principal.subject));
            next.run(request).await
        }
        Err(AuthError::InvalidToken) => ApiError::Unauthorized.into_response(),
        Err(AuthError::Unavailable) => ApiError::AuthenticationUnavailable.into_response(),
    }
}

/// Per-principal throttling; runs after authentication so only verified principals are keyed.
async fn throttle_principal<R: Repositories>(
    State(state): State<ApiState<R>>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let Some(actor) = request.extensions().get::<VerifiedActor>() else {
        return ApiError::Internal.into_response();
    };
    let Ok(principal) = UserId::try_new(actor.principal_id()) else {
        return ApiError::Internal.into_response();
    };
    match state.rate_limiter.check(&principal) {
        Ok(RateLimitDecision::Allowed) => next.run(request).await,
        Ok(RateLimitDecision::Limited {
            retry_after_seconds,
        }) => ApiError::RateLimited {
            retry_after_seconds,
        }
        .into_response(),
        Err(_) => ApiError::Unavailable.into_response(),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateListRequest {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RenameListRequest {
    name: String,
    expected_version: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedVersionQuery {
    expected_version: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageQuery {
    limit: Option<u32>,
    cursor: Option<String>,
}

impl From<PageQuery> for PageParameters {
    fn from(query: PageQuery) -> Self {
        Self {
            page_size: query.limit,
            cursor: query.cursor,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncQuery {
    since: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateItemRequest {
    expected_list_version: u64,
    name: String,
    quantity: Option<String>,
    unit: Option<String>,
    note: Option<String>,
}

/// Either a check-state change (`checked` alone) or a details update (`name` plus optional
/// `quantity`/`unit`/`note`). Mixing both is rejected because they are separate atomic
/// mutations with separate version increments.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateItemRequest {
    expected_version: u64,
    checked: Option<bool>,
    name: Option<String>,
    quantity: Option<String>,
    unit: Option<String>,
    note: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReorderItemsRequest {
    expected_list_version: u64,
    item_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PutProfileRequest {
    display_name: String,
    expected_version: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ListResponse {
    id: String,
    name: String,
    version: u64,
}

impl From<&ShoppingList> for ListResponse {
    fn from(list: &ShoppingList) -> Self {
        Self {
            id: list.id().as_str().to_owned(),
            name: list.name().as_str().to_owned(),
            version: list.version(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ListPageResponse {
    items: Vec<ListResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ItemResponse {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    quantity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    checked: bool,
    position: u32,
    item_version: u64,
}

impl From<&ShoppingItem> for ItemResponse {
    fn from(item: &ShoppingItem) -> Self {
        Self {
            id: item.id().as_str().to_owned(),
            name: item.name().as_str().to_owned(),
            quantity: item.quantity().map(|value| value.as_str().to_owned()),
            unit: item.unit().map(|value| value.as_str().to_owned()),
            note: item.note().map(|value| value.as_str().to_owned()),
            checked: item.checked(),
            position: item.position(),
            item_version: item.version(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ItemPageResponse {
    items: Vec<ItemResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
}

impl From<Page<ShoppingItem>> for ItemPageResponse {
    fn from(page: Page<ShoppingItem>) -> Self {
        Self {
            items: page.items.iter().map(ItemResponse::from).collect(),
            next_cursor: page.next.map(|cursor| cursor.as_str().to_owned()),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ItemMutationResponse {
    item: ItemResponse,
    list_version: u64,
}

impl From<(ShoppingItem, u64)> for ItemMutationResponse {
    fn from((item, list_version): (ShoppingItem, u64)) -> Self {
        Self {
            item: ItemResponse::from(&item),
            list_version,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ListVersionResponse {
    list_version: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReorderResponse {
    items: Vec<ItemResponse>,
    list_version: u64,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum SyncResponse {
    #[serde(rename_all = "camelCase")]
    Snapshot {
        version: u64,
        list: ListResponse,
        items: Vec<ItemResponse>,
    },
    #[serde(rename_all = "camelCase")]
    Changes {
        version: u64,
        since: u64,
        list: ListResponse,
        changed: Vec<ItemResponse>,
        removed: Vec<String>,
    },
}

impl From<ListSync> for SyncResponse {
    fn from(sync: ListSync) -> Self {
        match sync {
            ListSync::Snapshot { list, items } => Self::Snapshot {
                version: list.version(),
                list: ListResponse::from(&list),
                items: items.iter().map(ItemResponse::from).collect(),
            },
            ListSync::Changes {
                list,
                since,
                changed,
                removed,
            } => Self::Changes {
                version: list.version(),
                since: since.as_u64(),
                list: ListResponse::from(&list),
                changed: changed.iter().map(ItemResponse::from).collect(),
                removed: removed.iter().map(|id| id.as_str().to_owned()).collect(),
            },
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileResponse {
    id: String,
    display_name: String,
    version: u64,
}

impl From<&UserProfile> for ProfileResponse {
    fn from(profile: &UserProfile) -> Self {
        Self {
            id: profile.user_id().as_str().to_owned(),
            display_name: profile.display_name().as_str().to_owned(),
            version: profile.version().as_u64(),
        }
    }
}

fn principal(actor: &VerifiedActor) -> String {
    actor.principal_id().to_owned()
}

async fn list_accessible_lists<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Query(query): Query<PageQuery>,
) -> Result<Json<ListPageResponse>, ApiError> {
    let page = PageAccessibleLists::new(state.repositories.as_ref())
        .execute(principal(&actor), query.into())
        .await?;
    Ok(Json(ListPageResponse {
        items: page.items.iter().map(ListResponse::from).collect(),
        next_cursor: page.next.map(|cursor| cursor.as_str().to_owned()),
    }))
}

async fn create_personal_list<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Json(payload): Json<CreateListRequest>,
) -> Result<(StatusCode, Json<ListResponse>), ApiError> {
    let use_case = CreatePersonalList::new(state.repositories.as_ref(), state.list_ids.as_ref());
    let list = use_case
        .execute(CreatePersonalListCommand {
            owner_id: principal(&actor),
            name: payload.name,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(ListResponse::from(&list))))
}

fn expected_version(value: u64) -> Result<AggregateVersion, ApiError> {
    AggregateVersion::try_new(value).map_err(|_| ApiError::InvalidInput)
}

async fn rename_list<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Path(list_id): Path<String>,
    Json(payload): Json<RenameListRequest>,
) -> Result<Json<ListResponse>, ApiError> {
    let list = RenameShoppingList::new(state.repositories.as_ref())
        .execute(RenameShoppingListCommand {
            actor_id: principal(&actor),
            list_id,
            expected_version: expected_version(payload.expected_version)?,
            name: payload.name,
        })
        .await?;
    Ok(Json(ListResponse::from(&list)))
}

async fn delete_list<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Path(list_id): Path<String>,
    Query(query): Query<ExpectedVersionQuery>,
) -> Result<StatusCode, ApiError> {
    DeleteShoppingList::new(state.repositories.as_ref())
        .execute(DeleteShoppingListCommand {
            actor_id: principal(&actor),
            list_id,
            expected_version: expected_version(query.expected_version)?,
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn page_items<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Path(list_id): Path<String>,
    Query(query): Query<PageQuery>,
) -> Result<Json<ItemPageResponse>, ApiError> {
    let page = PageShoppingItems::new(state.repositories.as_ref())
        .execute(principal(&actor), list_id, query.into())
        .await?;
    Ok(Json(ItemPageResponse::from(page)))
}

async fn add_item<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Path(list_id): Path<String>,
    Json(payload): Json<CreateItemRequest>,
) -> Result<(StatusCode, Json<ItemMutationResponse>), ApiError> {
    let result = AddShoppingItem::new(state.repositories.as_ref(), state.item_ids.as_ref())
        .execute(AddShoppingItemCommand {
            actor_id: principal(&actor),
            list_id,
            expected_list_version: payload.expected_list_version,
            name: payload.name,
            quantity: payload.quantity,
            unit: payload.unit,
            note: payload.note,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(result.into())))
}

async fn update_item<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Path((list_id, item_id)): Path<(String, String)>,
    Json(payload): Json<UpdateItemRequest>,
) -> Result<Json<ItemMutationResponse>, ApiError> {
    let repositories = state.repositories.as_ref();
    let details_given = payload.name.is_some()
        || payload.quantity.is_some()
        || payload.unit.is_some()
        || payload.note.is_some();
    let result = match (payload.checked, payload.name) {
        (Some(checked), None) if !details_given => {
            SetShoppingItemChecked::new(repositories)
                .execute(SetShoppingItemCheckedCommand {
                    actor_id: principal(&actor),
                    list_id,
                    item_id,
                    expected_item_version: payload.expected_version,
                    checked,
                })
                .await?
        }
        (None, Some(name)) => {
            UpdateShoppingItem::new(repositories)
                .execute(UpdateShoppingItemCommand {
                    actor_id: principal(&actor),
                    list_id,
                    item_id,
                    expected_item_version: payload.expected_version,
                    name,
                    quantity: payload.quantity,
                    unit: payload.unit,
                    note: payload.note,
                })
                .await?
        }
        _ => return Err(ApiError::InvalidInput),
    };
    Ok(Json(result.into()))
}

async fn delete_item<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Path((list_id, item_id)): Path<(String, String)>,
    Query(query): Query<ExpectedVersionQuery>,
) -> Result<Json<ListVersionResponse>, ApiError> {
    let list_version = DeleteShoppingItem::new(state.repositories.as_ref())
        .execute(DeleteShoppingItemCommand {
            actor_id: principal(&actor),
            list_id,
            item_id,
            expected_item_version: query.expected_version,
        })
        .await?;
    Ok(Json(ListVersionResponse { list_version }))
}

async fn reorder_items<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Path(list_id): Path<String>,
    Json(payload): Json<ReorderItemsRequest>,
) -> Result<Json<ReorderResponse>, ApiError> {
    let (items, list_version) = ReorderShoppingItems::new(state.repositories.as_ref())
        .execute(ReorderShoppingItemsCommand {
            actor_id: principal(&actor),
            list_id,
            expected_list_version: payload.expected_list_version,
            item_ids: payload.item_ids,
        })
        .await?;
    Ok(Json(ReorderResponse {
        items: items.iter().map(ItemResponse::from).collect(),
        list_version,
    }))
}

async fn sync_list<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Path(list_id): Path<String>,
    Query(query): Query<SyncQuery>,
) -> Result<Json<SyncResponse>, ApiError> {
    let sync = SyncShoppingList::new(state.repositories.as_ref())
        .execute(principal(&actor), list_id, query.since)
        .await?;
    Ok(Json(sync.into()))
}

async fn get_profile<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
) -> Result<Json<ProfileResponse>, ApiError> {
    let profile = GetProfile::new(state.repositories.as_ref())
        .execute(principal(&actor))
        .await?;
    Ok(Json(ProfileResponse::from(&profile)))
}

/// Create the caller's profile (no `expectedVersion`) or update it (with `expectedVersion`).
async fn put_profile<R: Repositories>(
    State(state): State<ApiState<R>>,
    actor: VerifiedActor,
    Json(payload): Json<PutProfileRequest>,
) -> Result<(StatusCode, Json<ProfileResponse>), ApiError> {
    let repositories = state.repositories.as_ref();
    let (status, profile) = match payload.expected_version {
        None => (
            StatusCode::CREATED,
            CreateProfile::new(repositories)
                .execute(principal(&actor), payload.display_name)
                .await?,
        ),
        Some(expected) => (
            StatusCode::OK,
            UpdateProfile::new(repositories)
                .execute(principal(&actor), expected, payload.display_name)
                .await?,
        ),
    };
    Ok((status, Json(ProfileResponse::from(&profile))))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Serialize)]
struct ErrorDetail {
    code: &'static str,
    message: &'static str,
}

impl ErrorResponse {
    fn new(code: &'static str, message: &'static str) -> Self {
        Self {
            error: ErrorDetail { code, message },
        }
    }
}

#[derive(Debug)]
enum ApiError {
    AuthenticationUnavailable,
    Unauthorized,
    InvalidInput,
    Forbidden,
    NotFound,
    Conflict,
    ItemLimitReached,
    RateLimited { retry_after_seconds: u64 },
    Unavailable,
    Internal,
}

impl From<RepositoryError> for ApiError {
    fn from(error: RepositoryError) -> Self {
        match error {
            RepositoryError::NotFound => Self::NotFound,
            RepositoryError::Forbidden => Self::Forbidden,
            RepositoryError::Conflict | RepositoryError::VersionExhausted => Self::Conflict,
            RepositoryError::ItemLimitReached => Self::ItemLimitReached,
            RepositoryError::InvalidCursor => Self::InvalidInput,
            RepositoryError::Unavailable => Self::Unavailable,
            RepositoryError::AlreadyExists => Self::Internal,
        }
    }
}

impl From<CreatePersonalListError> for ApiError {
    fn from(error: CreatePersonalListError) -> Self {
        match error {
            CreatePersonalListError::InvalidName(_) => Self::InvalidInput,
            CreatePersonalListError::InvalidOwner(_) | CreatePersonalListError::AlreadyExists => {
                Self::Internal
            }
            CreatePersonalListError::PersistenceUnavailable => Self::Unavailable,
            CreatePersonalListError::InvalidGeneratedId(_)
            | CreatePersonalListError::RepositoryInvariantViolation => Self::Internal,
        }
    }
}

impl From<RenameShoppingListError> for ApiError {
    fn from(error: RenameShoppingListError) -> Self {
        match error {
            RenameShoppingListError::InvalidActor(_) => Self::Internal,
            // A malformed id cannot name an accessible list; answer like an unknown one.
            RenameShoppingListError::InvalidListId(_) => Self::NotFound,
            RenameShoppingListError::InvalidName(_) => Self::InvalidInput,
            RenameShoppingListError::Repository(error) => error.into(),
        }
    }
}

impl From<DeleteShoppingListError> for ApiError {
    fn from(error: DeleteShoppingListError) -> Self {
        match error {
            DeleteShoppingListError::InvalidActor(_) => Self::Internal,
            DeleteShoppingListError::InvalidListId(_) => Self::NotFound,
            DeleteShoppingListError::Repository(error) => error.into(),
        }
    }
}

impl From<ListQueryError> for ApiError {
    fn from(error: ListQueryError) -> Self {
        match error {
            ListQueryError::InvalidPrincipal | ListQueryError::RepositoryInvariantViolation => {
                Self::Internal
            }
            ListQueryError::InvalidListId | ListQueryError::NotFound => Self::NotFound,
            ListQueryError::InvalidPageSize
            | ListQueryError::InvalidCursor
            | ListQueryError::InvalidSinceVersion => Self::InvalidInput,
            ListQueryError::Forbidden => Self::Forbidden,
            ListQueryError::PersistenceUnavailable => Self::Unavailable,
        }
    }
}

impl From<ShoppingItemError> for ApiError {
    fn from(error: ShoppingItemError) -> Self {
        match error {
            ShoppingItemError::InvalidActor => Self::Internal,
            ShoppingItemError::InvalidListId | ShoppingItemError::NotFound => Self::NotFound,
            ShoppingItemError::InvalidItemId
            | ShoppingItemError::InvalidName(_)
            | ShoppingItemError::InvalidQuantity(_)
            | ShoppingItemError::InvalidUnit(_)
            | ShoppingItemError::InvalidNote(_)
            | ShoppingItemError::InvalidExpectedVersion
            | ShoppingItemError::InvalidOrder(_)
            | ShoppingItemError::InvalidCursor => Self::InvalidInput,
            ShoppingItemError::ItemLimitReached => Self::ItemLimitReached,
            ShoppingItemError::VersionExhausted | ShoppingItemError::Conflict => Self::Conflict,
            ShoppingItemError::Forbidden => Self::Forbidden,
            ShoppingItemError::PersistenceUnavailable => Self::Unavailable,
        }
    }
}

impl From<ProfileError> for ApiError {
    fn from(error: ProfileError) -> Self {
        match error {
            ProfileError::InvalidPrincipal | ProfileError::RepositoryInvariantViolation => {
                Self::Internal
            }
            ProfileError::InvalidDisplayName(_) | ProfileError::InvalidExpectedVersion => {
                Self::InvalidInput
            }
            ProfileError::NotFound => Self::NotFound,
            ProfileError::AlreadyExists
            | ProfileError::Conflict
            | ProfileError::VersionExhausted => Self::Conflict,
            ProfileError::PersistenceUnavailable => Self::Unavailable,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let unauthorized = matches!(self, Self::Unauthorized);
        let retry_after = match self {
            Self::RateLimited {
                retry_after_seconds,
            } => Some(retry_after_seconds),
            _ => None,
        };
        let (status, code, message) = match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "A valid Cognito access token is required",
            ),
            Self::AuthenticationUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "authentication_unavailable",
                "Authentication is not configured",
            ),
            Self::InvalidInput => (
                StatusCode::BAD_REQUEST,
                "invalid_input",
                "The request is invalid",
            ),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                "forbidden",
                "The caller may not perform this operation",
            ),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "The resource was not found",
            ),
            Self::Conflict => (
                StatusCode::CONFLICT,
                "version_conflict",
                "The resource was changed concurrently; reload and retry",
            ),
            Self::ItemLimitReached => (
                StatusCode::CONFLICT,
                "item_limit_reached",
                "The list has reached its item limit",
            ),
            Self::RateLimited { .. } => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "Too many requests; retry later",
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "service_unavailable",
                "The service is temporarily unavailable",
            ),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "An internal error occurred",
            ),
        };
        let mut response = (status, Json(ErrorResponse::new(code, message))).into_response();
        if unauthorized {
            response.headers_mut().insert(
                axum::http::header::WWW_AUTHENTICATE,
                axum::http::HeaderValue::from_static("Bearer"),
            );
        }
        if let Some(seconds) = retry_after {
            response
                .headers_mut()
                .insert(axum::http::header::RETRY_AFTER, seconds.into());
        }
        response
    }
}

#[cfg(test)]
mod route_tests;

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request},
    };
    use tower::ServiceExt;

    /// Implements the item and profile ports as fail-closed stubs for list-only test doubles.
    macro_rules! unavailable_items_and_profiles {
        ($type:ty) => {
            impl ShoppingItemRepository for $type {
                async fn add_item(
                    &self,
                    _: &einkaufio_application::ListAccessRequest,
                    _: &einkaufio_application::ListId,
                    _: AggregateVersion,
                    _: ShoppingItem,
                ) -> Result<AggregateVersion, RepositoryError> {
                    Err(RepositoryError::Unavailable)
                }
                async fn set_item_checked(
                    &self,
                    _: &einkaufio_application::ListAccessRequest,
                    _: &einkaufio_application::ListId,
                    _: &einkaufio_application::ItemId,
                    _: u64,
                    _: bool,
                ) -> Result<(ShoppingItem, AggregateVersion), RepositoryError> {
                    Err(RepositoryError::Unavailable)
                }
                async fn delete_item(
                    &self,
                    _: &einkaufio_application::ListAccessRequest,
                    _: &einkaufio_application::ListId,
                    _: &einkaufio_application::ItemId,
                    _: u64,
                ) -> Result<AggregateVersion, RepositoryError> {
                    Err(RepositoryError::Unavailable)
                }
            }
            impl ProfileRepository for $type {
                async fn create_profile(&self, _: UserProfile) -> Result<(), RepositoryError> {
                    Err(RepositoryError::Unavailable)
                }
                async fn get_profile(&self, _: &UserId) -> Result<UserProfile, RepositoryError> {
                    Err(RepositoryError::Unavailable)
                }
                async fn update_profile(
                    &self,
                    _: &UserId,
                    _: AggregateVersion,
                    _: einkaufio_application::DisplayName,
                ) -> Result<UserProfile, RepositoryError> {
                    Err(RepositoryError::Unavailable)
                }
            }
        };
    }

    unavailable_items_and_profiles!(TestLists);
    unavailable_items_and_profiles!(RecordingLists);
    unavailable_items_and_profiles!(AccessibleLists);

    struct TestItemIds;

    impl ItemIdGenerator for TestItemIds {
        fn generate(&self) -> String {
            "test-item-id".to_owned()
        }
    }

    fn state<R: Repositories>(repositories: Arc<R>) -> ApiState<R> {
        ApiState::new(
            repositories,
            Arc::new(TestIds),
            Arc::new(TestItemIds),
            Arc::new(einkaufio_adapters::token_bucket::TokenBucketRateLimiter::with_defaults()),
        )
    }

    #[derive(Default)]
    struct TestLists {
        lists: Mutex<Vec<ShoppingList>>,
    }

    impl ShoppingListRepository for TestLists {
        async fn create_personal(
            &self,
            list: ShoppingList,
        ) -> Result<(), einkaufio_application::RepositoryError> {
            self.lists.lock().unwrap().push(list);
            Ok(())
        }
    }

    struct TestIds;

    impl ListIdGenerator for TestIds {
        fn generate(&self) -> String {
            "test-list-id".to_owned()
        }
    }

    fn test_router() -> Router {
        let protected =
            protected_routes::<TestLists>().route_layer(middleware::from_fn(inject_test_actor));
        build_router(state(Arc::new(TestLists::default())), protected)
    }

    async fn inject_test_actor(mut request: axum::extract::Request, next: Next) -> Response {
        request.extensions_mut().insert(VerifiedActor {
            principal_id: "verified-test-owner".to_owned(),
        });
        next.run(request).await
    }

    async fn send_json(app: Router, method: Method, body: &'static str) -> Response {
        app.oneshot(
            Request::builder()
                .method(method)
                .uri("/api/lists")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn health_is_public() {
        let app = router(state(Arc::new(TestLists::default())));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&body[..], br#"{"status":"ok"}"#);
    }

    #[tokio::test]
    async fn protected_routes_fail_closed_without_authentication_configuration() {
        let app = router(state(Arc::new(TestLists::default())));
        for (method, body) in [
            (Method::GET, Body::empty()),
            (
                Method::POST,
                Body::from(r#"{"name":"Groceries","ownerId":"attacker"}"#),
            ),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri("/api/lists")
                        .header("content-type", "application/json")
                        .body(body)
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["error"]["code"], "authentication_unavailable");
        }
    }

    #[tokio::test]
    async fn create_list_uses_verified_test_actor() {
        let response = send_json(test_router(), Method::POST, r#"{"name":"Groceries"}"#).await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["id"], "test-list-id");
        assert_eq!(value["name"], "Groceries");
    }

    #[tokio::test]
    async fn blank_list_name_is_rejected() {
        let response = send_json(test_router(), Method::POST, r#"{"name":"   "}"#).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn client_cannot_supply_owner_id() {
        let response = send_json(
            test_router(),
            Method::POST,
            r#"{"name":"Groceries","ownerId":"attacker"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn authentication_middleware_rejects_missing_and_malformed_bearer_tokens() {
        let verifier = CognitoTokenVerifier::new_with_jwks_url(
            crate::auth::CognitoConfig {
                issuer: "https://issuer.example.test/pool".to_owned(),
                app_client_id: "client".to_owned(),
            },
            "http://127.0.0.1:1/.well-known/jwks.json".to_owned(),
        )
        .unwrap();
        let state = state(Arc::new(TestLists::default())).with_authenticator(verifier);
        let app = router(state);

        for authorization in [None, Some("Bearer not-a-jwt")] {
            let mut builder = Request::builder().uri("/api/lists");
            if let Some(value) = authorization {
                builder = builder.header(axum::http::header::AUTHORIZATION, value);
            }
            let response = app
                .clone()
                .oneshot(builder.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(
                response
                    .headers()
                    .get(axum::http::header::WWW_AUTHENTICATE)
                    .unwrap(),
                "Bearer"
            );
        }
    }

    #[derive(Default)]
    struct RecordingLists {
        owners: Mutex<Vec<String>>,
    }

    impl ShoppingListRepository for RecordingLists {
        async fn create_personal(
            &self,
            list: ShoppingList,
        ) -> Result<(), einkaufio_application::RepositoryError> {
            self.owners
                .lock()
                .unwrap()
                .push(list.personal_owner().unwrap().as_str().to_owned());
            Ok(())
        }
    }

    #[tokio::test]
    async fn verified_subject_reaches_the_use_case_as_owner() {
        use crate::auth::jwks_integration_tests::{claims, fixture, signed_token};

        let f = fixture().await;
        let lists = Arc::new(RecordingLists::default());
        let state = state(lists.clone()).with_authenticator(f.verifier.clone());
        let token = signed_token(&f.private_pem, "key-1", claims(serde_json::json!({})));

        for scheme in ["Bearer", "bearer"] {
            let response = router(state.clone())
                .oneshot(
                    Request::builder()
                        .method(Method::POST)
                        .uri("/api/lists")
                        .header("content-type", "application/json")
                        .header(
                            axum::http::header::AUTHORIZATION,
                            format!("{scheme} {token}"),
                        )
                        .body(Body::from(r#"{"name":"Groceries"}"#))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::CREATED);
        }
        assert_eq!(
            *lists.owners.lock().unwrap(),
            vec!["user-123".to_owned(), "user-123".to_owned()]
        );
        f.server.abort();
    }

    #[tokio::test]
    async fn bearer_token_with_extra_whitespace_is_rejected() {
        let f = crate::auth::jwks_integration_tests::fixture().await;
        let token = crate::auth::jwks_integration_tests::signed_token(
            &f.private_pem,
            "key-1",
            crate::auth::jwks_integration_tests::claims(serde_json::json!({})),
        );
        let state = state(Arc::new(TestLists::default())).with_authenticator(f.verifier.clone());
        let response = router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/lists")
                    .header(
                        axum::http::header::AUTHORIZATION,
                        format!("Bearer  {token}"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        f.server.abort();
    }

    struct AccessibleLists {
        principals: Mutex<Vec<String>>,
    }

    impl ShoppingListRepository for AccessibleLists {
        async fn create_personal(
            &self,
            _list: ShoppingList,
        ) -> Result<(), einkaufio_application::RepositoryError> {
            Ok(())
        }

        async fn list_accessible_page(
            &self,
            principal: &einkaufio_application::UserId,
            _page: &einkaufio_application::PageRequest,
        ) -> Result<Page<ShoppingList>, einkaufio_application::RepositoryError> {
            self.principals
                .lock()
                .unwrap()
                .push(principal.as_str().to_owned());
            Ok(Page {
                next: None,
                items: vec![ShoppingList::new_personal(
                    einkaufio_application::ListId::try_new("accessible-list").unwrap(),
                    principal.clone(),
                    einkaufio_application::ListName::try_new("Wocheneinkauf").unwrap(),
                )],
            })
        }
    }

    #[tokio::test]
    async fn list_route_queries_lists_accessible_to_the_verified_principal() {
        let lists = Arc::new(AccessibleLists {
            principals: Mutex::new(Vec::new()),
        });
        let protected = protected_routes::<AccessibleLists>()
            .route_layer(middleware::from_fn(inject_test_actor));
        let app = build_router(state(lists.clone()), protected);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/lists?ownerId=attacker")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["items"][0]["id"], "accessible-list");
        assert_eq!(value["items"][0]["version"], 1);
        assert_eq!(
            *lists.principals.lock().unwrap(),
            vec!["verified-test-owner".to_owned()]
        );
    }

    #[tokio::test]
    async fn list_route_fails_closed_when_the_adapter_lacks_the_query() {
        let response = test_router()
            .oneshot(
                Request::builder()
                    .uri("/api/lists")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["error"]["code"], "service_unavailable");
    }
}

//! HTTP transport for personal shopping lists.
//!
//! This module deliberately has no authentication implementation yet. Every application
//! route is guarded by an authentication middleware that rejects requests until a verified
//! Cognito principal can be supplied by the future JWT-verification adapter.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use einkaufio_application::{
    CreatePersonalList, CreatePersonalListCommand, ListIdGenerator, ListPersonalLists,
    ListPersonalListsQuery, ShoppingListRepository,
};
use einkaufio_domain::ShoppingList;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct ApiState {
    lists: Arc<dyn ShoppingListRepository + Send + Sync>,
    ids: Arc<dyn ListIdGenerator + Send + Sync>,
}

impl ApiState {
    pub fn new(
        lists: Arc<dyn ShoppingListRepository + Send + Sync>,
        ids: Arc<dyn ListIdGenerator + Send + Sync>,
    ) -> Self {
        Self { lists, ids }
    }
}

/// Builds the application router. Health is public; all /api routes fail closed.
///
/// The authentication guard is intentionally not configurable to accept an arbitrary
/// user ID. It will be replaced by verified Cognito claims once JWT verification is added.
pub fn router(state: ApiState) -> Router {
    let protected = Router::new()
        .route("/lists", get(list_personal_lists).post(create_personal_list))
        .route_layer(middleware::from_fn(require_verified_actor));

    Router::new()
        .route("/health", get(health))
        .nest("/api", protected)
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

async fn require_verified_actor(
    _headers: HeaderMap,
    _request: axum::extract::Request,
    _next: Next,
) -> Response {
    // No token parsing or client-supplied identity is accepted until the Cognito verifier
    // is implemented. This prevents the HTTP surface from bypassing the trust boundary.
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorResponse::new(
            "authentication_unavailable",
            "Authentication is not configured",
        )),
    )
        .into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateListRequest {
    name: String,
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
}

async fn list_personal_lists(
    State(_state): State<ApiState>,
) -> Result<Json<ListPageResponse>, ApiError> {
    // The middleware currently blocks this handler. Keep the state extractor and use-case
    // wiring explicit so authentication can be added without changing transport contracts.
    let _ = ListPersonalListsQuery {
        owner_id: String::new(),
    };
    Err(ApiError::AuthenticationUnavailable)
}

async fn create_personal_list(
    State(_state): State<ApiState>,
    Json(_payload): Json<CreateListRequest>,
) -> Result<(StatusCode, Json<ListResponse>), ApiError> {
    // A request body must never provide owner/actor identity. This remains unavailable until
    // the middleware can pass a principal derived from verified Cognito claims.
    let _ = CreatePersonalListCommand {
        owner_id: String::new(),
        name: String::new(),
    };
    Err(ApiError::AuthenticationUnavailable)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorResponse {
    code: &'static str,
    message: &'static str,
}

impl ErrorResponse {
    const fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }
}

#[derive(Debug)]
enum ApiError {
    AuthenticationUnavailable,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::AuthenticationUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse::new(
                    "authentication_unavailable",
                    "Authentication is not configured",
                )),
            )
                .into_response(),
        }
    }
}

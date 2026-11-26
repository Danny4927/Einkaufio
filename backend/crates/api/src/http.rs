//! HTTP transport for personal shopping lists.
//!
//! Authentication is intentionally fail-closed until Cognito JWT verification is provided.
//! No caller-controlled identity is accepted by these handlers.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use einkaufio_application::{
    CreatePersonalList, CreatePersonalListCommand, CreatePersonalListError, ListIdGenerator,
    ListPersonalLists, ListPersonalListsError, ListPersonalListsQuery, ShoppingListRepository,
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
    _request: axum::extract::Request,
    _next: Next,
) -> Response {
    // This placeholder is intentionally not configurable to accept an arbitrary user ID.
    // Replace it only together with cryptographic Cognito token verification and a verified
    // principal extractor.
    ApiError::AuthenticationUnavailable.into_response()
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
    State(state): State<ApiState>,
) -> Result<Json<ListPageResponse>, ApiError> {
    let use_case = ListPersonalLists::new(state.lists.as_ref());
    let lists = use_case
        .execute(ListPersonalListsQuery {
            owner_id: String::new(),
        })
        .map_err(ApiError::from)?;
    Ok(Json(ListPageResponse {
        items: lists.iter().map(ListResponse::from).collect(),
    }))
}

async fn create_personal_list(
    State(state): State<ApiState>,
    Json(payload): Json<CreateListRequest>,
) -> Result<(StatusCode, Json<ListResponse>), ApiError> {
    let use_case = CreatePersonalList::new(state.lists.as_ref(), state.ids.as_ref());
    let list = use_case
        .execute(CreatePersonalListCommand {
            owner_id: String::new(),
            name: payload.name,
        })
        .map_err(ApiError::from)?;
    Ok((StatusCode::CREATED, Json(ListResponse::from(&list))))
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
    InvalidInput,
    Conflict,
    NotFound,
    Unavailable,
    Internal,
}

impl From<CreatePersonalListError> for ApiError {
    fn from(error: CreatePersonalListError) -> Self {
        match error {
            CreatePersonalListError::InvalidOwner(_) | CreatePersonalListError::InvalidName(_) => {
                Self::InvalidInput
            }
            CreatePersonalListError::AlreadyExists => Self::Conflict,
            CreatePersonalListError::PersistenceUnavailable => Self::Unavailable,
            CreatePersonalListError::InvalidGeneratedId(_)
            | CreatePersonalListError::RepositoryInvariantViolation => Self::Internal,
        }
    }
}

impl From<ListPersonalListsError> for ApiError {
    fn from(error: ListPersonalListsError) -> Self {
        match error {
            ListPersonalListsError::InvalidOwner(_) => Self::InvalidInput,
            ListPersonalListsError::PersistenceUnavailable => Self::Unavailable,
            ListPersonalListsError::RepositoryInvariantViolation => Self::Internal,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
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
            Self::Conflict => (
                StatusCode::CONFLICT,
                "conflict",
                "The resource conflicts with the current state",
            ),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "The resource was not found",
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
        (status, Json(ErrorResponse::new(code, message))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request},
    };
    use tower::ServiceExt;

    struct NoLists;
    impl ShoppingListRepository for NoLists {
        fn create_personal(&self, _list: ShoppingList) -> Result<(), einkaufio_application::RepositoryError> {
            Err(einkaufio_application::RepositoryError::Unavailable)
        }
    }

    struct NoIds;
    impl ListIdGenerator for NoIds {
        fn generate(&self) -> String {
            "unused".to_owned()
        }
    }

    #[tokio::test]
    async fn health_is_public() {
        let app = router(ApiState::new(Arc::new(NoLists), Arc::new(NoIds)));
        let response = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&body[..], br#"{"status":"ok"}"#);
    }

    #[tokio::test]
    async fn personal_list_routes_fail_closed_without_verified_authentication() {
        let app = router(ApiState::new(Arc::new(NoLists), Arc::new(NoIds)));
        for (method, uri, body) in [
            (Method::GET, "/api/lists", Body::empty()),
            (Method::POST, "/api/lists", Body::from(r#"{"name":"Einkauf","ownerId":"attacker"}"#)),
        ] {
            let response = app
                .clone()
                .oneshot(Request::builder().method(method).uri(uri).header("content-type", "application/json").body(body).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["error"]["code"], "authentication_unavailable");
        }
    }
}

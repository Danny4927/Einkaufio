//! HTTP transport for personal shopping lists.
//!
//! Authentication is intentionally fail-closed until Cognito JWT verification is provided.
//! No caller-controlled identity is accepted by these handlers.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{FromRequestParts, State},
    http::{StatusCode, request::Parts},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use einkaufio_application::{
    CreatePersonalList, CreatePersonalListCommand, CreatePersonalListError, ListIdGenerator,
    ListPersonalLists, ListPersonalListsError, ListPersonalListsQuery, ShoppingList,
    ShoppingListRepository,
};
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
    build_router(
        state,
        protected_routes().route_layer(middleware::from_fn(require_verified_actor)),
    )
}

fn protected_routes() -> Router<ApiState> {
    Router::new().route(
        "/lists",
        get(list_personal_lists).post(create_personal_list),
    )
}

fn build_router(state: ApiState, protected: Router<ApiState>) -> Router {
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

/// Identity that can only be created inside the API crate after authentication verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedActor {
    owner_id: String,
}

impl VerifiedActor {
    /// The only production constructor is crate-private so transport callers cannot forge identity.
    pub(crate) fn from_verified_claims(owner_id: String) -> Self {
        Self { owner_id }
    }

    fn owner_id(&self) -> &str {
        &self.owner_id
    }
}

impl FromRequestParts<ApiState> for VerifiedActor {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &ApiState,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Self>()
            .cloned()
            .ok_or(ApiError::AuthenticationUnavailable)
    }
}

async fn require_verified_actor(_request: axum::extract::Request, _next: Next) -> Response {
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
    actor: VerifiedActor,
) -> Result<Json<ListPageResponse>, ApiError> {
    let use_case = ListPersonalLists::new(state.lists.as_ref());
    let lists = use_case
        .execute(ListPersonalListsQuery {
            owner_id: actor.owner_id().to_owned(),
        })
        .map_err(ApiError::from)?;
    Ok(Json(ListPageResponse {
        items: lists.iter().map(ListResponse::from).collect(),
    }))
}

async fn create_personal_list(
    State(state): State<ApiState>,
    actor: VerifiedActor,
    Json(payload): Json<CreateListRequest>,
) -> Result<(StatusCode, Json<ListResponse>), ApiError> {
    let use_case = CreatePersonalList::new(state.lists.as_ref(), state.ids.as_ref());
    let list = use_case
        .execute(CreatePersonalListCommand {
            owner_id: actor.owner_id().to_owned(),
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
    Unavailable,
    Internal,
}

impl From<CreatePersonalListError> for ApiError {
    fn from(error: CreatePersonalListError) -> Self {
        match error {
            CreatePersonalListError::InvalidName(_) => Self::InvalidInput,
            // Owner identity and generated list IDs are server-controlled. Failures here
            // indicate an authentication/infrastructure invariant, not a client conflict.
            CreatePersonalListError::InvalidOwner(_)
            | CreatePersonalListError::AlreadyExists => Self::Internal,
            CreatePersonalListError::PersistenceUnavailable => Self::Unavailable,
            CreatePersonalListError::InvalidGeneratedId(_)
            | CreatePersonalListError::RepositoryInvariantViolation => Self::Internal,
        }
    }
}

impl From<ListPersonalListsError> for ApiError {
    fn from(error: ListPersonalListsError) -> Self {
        match error {
            ListPersonalListsError::InvalidOwner(_) => Self::Internal,
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
    use std::sync::Mutex;

    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request},
    };
    use tower::ServiceExt;

    #[derive(Default)]
    struct TestLists {
        lists: Mutex<Vec<ShoppingList>>,
    }

    impl ShoppingListRepository for TestLists {
        fn create_personal(
            &self,
            list: ShoppingList,
        ) -> Result<(), einkaufio_application::RepositoryError> {
            self.lists.lock().unwrap().push(list);
            Ok(())
        }

        fn list_personal_for_owner(
            &self,
            owner_id: &einkaufio_domain::UserId,
        ) -> Result<Vec<ShoppingList>, einkaufio_application::RepositoryError> {
            Ok(self
                .lists
                .lock()
                .unwrap()
                .iter()
                .filter(|list| list.owner_id() == owner_id)
                .cloned()
                .collect())
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
            protected_routes().route_layer(middleware::from_fn(inject_test_actor));
        build_router(
            ApiState::new(Arc::new(TestLists::default()), Arc::new(TestIds)),
            protected,
        )
    }

    async fn inject_test_actor(
        mut request: axum::extract::Request,
        next: Next,
    ) -> Response {
        request.extensions_mut().insert(VerifiedActor {
            owner_id: "verified-test-owner".to_owned(),
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
        let app = router(ApiState::new(
            Arc::new(TestLists::default()),
            Arc::new(TestIds),
        ));
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
    async fn personal_list_routes_fail_closed_without_verified_authentication() {
        let app = router(ApiState::new(
            Arc::new(TestLists::default()),
            Arc::new(TestIds),
        ));
        for (method, uri, body) in [
            (Method::GET, "/api/lists", Body::empty()),
            (
                Method::POST,
                "/api/lists",
                Body::from(r#"{"name":"Einkauf","ownerId":"attacker"}"#),
            ),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
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
    async fn create_list_returns_created_for_verified_actor() {
        let response = send_json(
            test_router(),
            Method::POST,
            r#"{"name":"Wocheneinkauf"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["id"], "test-list-id");
        assert_eq!(value["name"], "Wocheneinkauf");
    }

    #[tokio::test]
    async fn create_list_returns_bad_request_for_blank_name() {
        let response = send_json(test_router(), Method::POST, r#"{"name":"   "}"#).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_list_returns_unprocessable_entity_for_unknown_fields() {
        let response = send_json(
            test_router(),
            Method::POST,
            r#"{"name":"Wocheneinkauf","ownerId":"attacker"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}

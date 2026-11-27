//! HTTP transport for shopping lists.

use std::sync::Arc;

use crate::auth::{AuthError, CognitoTokenVerifier};
use axum::{
    Json, Router,
    extract::{FromRequestParts, State},
    http::{StatusCode, request::Parts},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use einkaufio_application::{
    CreatePersonalList, CreatePersonalListCommand, CreatePersonalListError, ListAccessibleLists,
    ListAccessibleListsError, ListAccessibleListsQuery, ListIdGenerator, ShoppingList,
    ShoppingListRepository,
};
use serde::{Deserialize, Serialize};

/// Shared handler state, generic over the list repository.
///
/// Repository ports use `async fn` in traits and are therefore statically dispatched
/// (ADR 014); the composition root picks the concrete adapter type.
pub struct ApiState<L> {
    lists: Arc<L>,
    ids: Arc<dyn ListIdGenerator>,
    verifier: Option<Arc<CognitoTokenVerifier>>,
}

impl<L> Clone for ApiState<L> {
    fn clone(&self) -> Self {
        Self {
            lists: Arc::clone(&self.lists),
            ids: Arc::clone(&self.ids),
            verifier: self.verifier.clone(),
        }
    }
}

impl<L: ShoppingListRepository + 'static> ApiState<L> {
    pub fn new(lists: Arc<L>, ids: Arc<dyn ListIdGenerator>) -> Self {
        Self {
            lists,
            ids,
            verifier: None,
        }
    }

    pub fn with_authenticator(mut self, verifier: CognitoTokenVerifier) -> Self {
        self.verifier = Some(Arc::new(verifier));
        self
    }

    /// Build an authenticated API state from trusted runtime environment variables.
    pub fn from_cognito_env(
        lists: Arc<L>,
        ids: Arc<dyn ListIdGenerator>,
    ) -> Result<Self, AuthError> {
        let config = crate::auth::CognitoConfig::from_env()?;
        let verifier = CognitoTokenVerifier::new(config)?;
        Ok(Self::new(lists, ids).with_authenticator(verifier))
    }
}

/// Build the HTTP router. This function does not read the environment: the composition root
/// must construct the state with [`ApiState::from_cognito_env`] and abort startup on error.
/// A state without a verifier keeps protected routes fail-closed (503).
pub fn router<L: ShoppingListRepository + 'static>(state: ApiState<L>) -> Router {
    let protected = protected_routes().route_layer(middleware::from_fn_with_state(
        state.clone(),
        require_verified_actor::<L>,
    ));
    build_router(state, protected)
}

fn protected_routes<L: ShoppingListRepository + 'static>() -> Router<ApiState<L>> {
    Router::new().route(
        "/lists",
        get(list_accessible_lists::<L>).post(create_personal_list::<L>),
    )
}

fn build_router<L: ShoppingListRepository + 'static>(
    state: ApiState<L>,
    protected: Router<ApiState<L>>,
) -> Router {
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

async fn require_verified_actor<L: ShoppingListRepository + 'static>(
    State(state): State<ApiState<L>>,
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

async fn list_accessible_lists<L: ShoppingListRepository + 'static>(
    State(state): State<ApiState<L>>,
    actor: VerifiedActor,
) -> Result<Json<ListPageResponse>, ApiError> {
    let use_case = ListAccessibleLists::new(state.lists.as_ref());
    let lists = use_case
        .execute(ListAccessibleListsQuery {
            principal_id: actor.principal_id().to_owned(),
        })
        .await
        .map_err(ApiError::from)?;

    Ok(Json(ListPageResponse {
        items: lists.iter().map(ListResponse::from).collect(),
    }))
}

async fn create_personal_list<L: ShoppingListRepository + 'static>(
    State(state): State<ApiState<L>>,
    actor: VerifiedActor,
    Json(payload): Json<CreateListRequest>,
) -> Result<(StatusCode, Json<ListResponse>), ApiError> {
    let use_case = CreatePersonalList::new(state.lists.as_ref(), state.ids.as_ref());
    let list = use_case
        .execute(CreatePersonalListCommand {
            owner_id: actor.principal_id().to_owned(),
            name: payload.name,
        })
        .await
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
    Unauthorized,
    InvalidInput,
    Unavailable,
    Internal,
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

impl From<ListAccessibleListsError> for ApiError {
    fn from(error: ListAccessibleListsError) -> Self {
        match error {
            ListAccessibleListsError::InvalidPrincipal(_)
            | ListAccessibleListsError::RepositoryInvariantViolation => Self::Internal,
            ListAccessibleListsError::PersistenceUnavailable => Self::Unavailable,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let unauthorized = matches!(self, Self::Unauthorized);
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
        response
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
        build_router(
            ApiState::new(Arc::new(TestLists::default()), Arc::new(TestIds)),
            protected,
        )
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
    async fn protected_routes_fail_closed_without_authentication_configuration() {
        let app = router(ApiState::new(
            Arc::new(TestLists::default()),
            Arc::new(TestIds),
        ));
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
        let state = ApiState::new(Arc::new(TestLists::default()), Arc::new(TestIds))
            .with_authenticator(verifier);
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
                .push(list.owner_id().as_str().to_owned());
            Ok(())
        }
    }

    #[tokio::test]
    async fn verified_subject_reaches_the_use_case_as_owner() {
        use crate::auth::jwks_integration_tests::{claims, fixture, signed_token};

        let f = fixture().await;
        let lists = Arc::new(RecordingLists::default());
        let state =
            ApiState::new(lists.clone(), Arc::new(TestIds)).with_authenticator(f.verifier.clone());
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
        let state = ApiState::new(Arc::new(TestLists::default()), Arc::new(TestIds))
            .with_authenticator(f.verifier.clone());
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

        async fn list_accessible(
            &self,
            principal: &einkaufio_application::UserId,
        ) -> Result<Vec<ShoppingList>, einkaufio_application::RepositoryError> {
            self.principals
                .lock()
                .unwrap()
                .push(principal.as_str().to_owned());
            Ok(vec![ShoppingList::new_personal(
                einkaufio_application::ListId::try_new("accessible-list").unwrap(),
                principal.clone(),
                einkaufio_application::ListName::try_new("Wocheneinkauf").unwrap(),
            )])
        }
    }

    #[tokio::test]
    async fn list_route_queries_lists_accessible_to_the_verified_principal() {
        let lists = Arc::new(AccessibleLists {
            principals: Mutex::new(Vec::new()),
        });
        let protected = protected_routes::<AccessibleLists>()
            .route_layer(middleware::from_fn(inject_test_actor));
        let app = build_router(ApiState::new(lists.clone(), Arc::new(TestIds)), protected);

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

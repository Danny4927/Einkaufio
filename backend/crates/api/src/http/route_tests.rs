//! Route tests for list mutation, item, pagination, sync, profile and abuse-control behavior.
//!
//! They drive the real router (with the test actor injected in place of Cognito verification)
//! against the in-memory reference adapter, so authorization and optimistic concurrency come
//! from the same access-scoped port semantics a production adapter must provide.

use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request};
use einkaufio_adapters::in_memory::InMemoryRepository;
use einkaufio_adapters::repository_contracts::provision_household;
use einkaufio_adapters::token_bucket::TokenBucketRateLimiter;
use einkaufio_application::{
    HouseholdAccessRequest, HouseholdOperation, HouseholdRepository, ListId, ListName,
};
use serde_json::{Value, json};
use tower::ServiceExt;

use super::*;

const OWNER: &str = "owner";
const CONTRIBUTOR: &str = "contributor";
const STRANGER: &str = "stranger";
const TEST_ACTOR_HEADER: &str = "x-test-actor";

#[derive(Default)]
struct Sequence(AtomicU64);

impl Sequence {
    fn next(&self, prefix: &str) -> String {
        format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

impl ListIdGenerator for Sequence {
    fn generate(&self) -> String {
        self.next("list")
    }
}

impl ItemIdGenerator for Sequence {
    fn generate(&self) -> String {
        self.next("item")
    }
}

/// Stands in for the Cognito middleware: the test names the verified principal.
async fn inject_header_actor(mut request: axum::extract::Request, next: Next) -> Response {
    let Some(principal) = request
        .headers()
        .get(TEST_ACTOR_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
    else {
        return ApiError::Unauthorized.into_response();
    };
    request
        .extensions_mut()
        .insert(VerifiedActor::from_verified_claims(principal));
    next.run(request).await
}

struct Harness {
    app: Router,
    repository: Arc<InMemoryRepository>,
}

fn harness_with_limiter(limiter: TokenBucketRateLimiter) -> Harness {
    let repository = Arc::new(InMemoryRepository::new());
    let state = ApiState::new(
        repository.clone(),
        Arc::new(Sequence::default()),
        Arc::new(Sequence::default()),
        Arc::new(limiter),
    );
    let protected = protected_routes::<InMemoryRepository>()
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            throttle_principal::<InMemoryRepository>,
        ))
        .route_layer(middleware::from_fn(inject_header_actor));
    Harness {
        app: build_router(state, protected),
        repository,
    }
}

fn harness() -> Harness {
    harness_with_limiter(TokenBucketRateLimiter::try_new(10_000, 10_000, 100).unwrap())
}

impl Harness {
    async fn call(
        &self,
        method: Method,
        uri: &str,
        actor: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let response = self.call_raw(method, uri, actor, body).await;
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, value)
    }

    async fn call_raw(
        &self,
        method: Method,
        uri: &str,
        actor: &str,
        body: Option<Value>,
    ) -> Response {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(TEST_ACTOR_HEADER, actor);
        let body = match body {
            Some(value) => {
                builder = builder.header("content-type", "application/json");
                Body::from(value.to_string())
            }
            None => Body::empty(),
        };
        self.app
            .clone()
            .oneshot(builder.body(body).unwrap())
            .await
            .unwrap()
    }

    async fn create_list(&self, name: &str) -> String {
        let (status, body) = self
            .call(
                Method::POST,
                "/api/lists",
                OWNER,
                Some(json!({ "name": name })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        body["id"].as_str().unwrap().to_owned()
    }

    /// A household list administered by OWNER (household owner) on which CONTRIBUTOR, an
    /// active member who joined through an invitation, may only contribute. Provisioned
    /// through the production household ports, not a test-only access path (ADR 015).
    async fn shared_list(&self) -> String {
        let household = provision_household(
            self.repository.as_ref(),
            "route-household",
            OWNER,
            &[CONTRIBUTOR],
        )
        .await;
        let list = ShoppingList::new_household(
            ListId::try_new("route-shared-list").unwrap(),
            household.id().clone(),
            ListName::try_new("Shared").unwrap(),
        );
        self.repository
            .create_household_list(
                &HouseholdAccessRequest::for_operation(
                    UserId::try_new(OWNER).unwrap(),
                    HouseholdOperation::CreateList,
                ),
                list.clone(),
            )
            .await
            .unwrap();
        list.id().as_str().to_owned()
    }

    async fn add_item(&self, list: &str, expected: u64, name: &str) -> (StatusCode, Value) {
        self.call(
            Method::POST,
            &format!("/api/lists/{list}/items"),
            OWNER,
            Some(json!({ "expectedListVersion": expected, "name": name })),
        )
        .await
    }
}

fn error_code(body: &Value) -> &str {
    body["error"]["code"].as_str().unwrap_or_default()
}

// --- A6: rename/delete lists ----------------------------------------------------------------

#[tokio::test]
async fn list_mutation_routes_require_authentication() {
    let verifier = CognitoTokenVerifier::new_with_jwks_url(
        crate::auth::CognitoConfig {
            issuer: "https://issuer.example.test/pool".to_owned(),
            app_client_id: "client".to_owned(),
        },
        "http://127.0.0.1:1/.well-known/jwks.json".to_owned(),
    )
    .unwrap();
    let state = ApiState::new(
        Arc::new(InMemoryRepository::new()),
        Arc::new(Sequence::default()),
        Arc::new(Sequence::default()),
        Arc::new(TokenBucketRateLimiter::with_defaults()),
    )
    .with_authenticator(verifier);
    let app = router(state);
    for (method, uri) in [
        (Method::PATCH, "/api/lists/l"),
        (Method::DELETE, "/api/lists/l?expectedVersion=1"),
        (Method::GET, "/api/lists/l/items"),
        (Method::POST, "/api/lists/l/items"),
        (Method::PATCH, "/api/lists/l/items/i"),
        (Method::DELETE, "/api/lists/l/items/i?expectedVersion=1"),
        (Method::PUT, "/api/lists/l/item-order"),
        (Method::GET, "/api/lists/l/sync"),
        (Method::GET, "/api/me"),
        (Method::PUT, "/api/me"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method.clone())
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {uri}"
        );
    }
}

#[tokio::test]
async fn owner_renames_list_with_expected_version() {
    let h = harness();
    let list = h.create_list("Groceries").await;
    let (status, body) = h
        .call(
            Method::PATCH,
            &format!("/api/lists/{list}"),
            OWNER,
            Some(json!({ "name": "Weekly", "expectedVersion": 1 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "Weekly");
    assert_eq!(body["version"], 2);

    let (status, body) = h
        .call(
            Method::PATCH,
            &format!("/api/lists/{list}"),
            OWNER,
            Some(json!({ "name": "Stale", "expectedVersion": 1 })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "version_conflict");
}

#[tokio::test]
async fn rename_rejects_invalid_input() {
    let h = harness();
    let list = h.create_list("Groceries").await;
    let uri = format!("/api/lists/{list}");
    for body in [
        json!({ "name": "   ", "expectedVersion": 1 }),
        json!({ "name": "x", "expectedVersion": 0 }),
    ] {
        let (status, _) = h.call(Method::PATCH, &uri, OWNER, Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let (status, _) = h
        .call(Method::PATCH, &uri, OWNER, Some(json!({ "name": "x" })))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn rename_and_delete_are_forbidden_for_contributors() {
    let h = harness();
    let list = h.shared_list().await;
    let (status, body) = h
        .call(
            Method::PATCH,
            &format!("/api/lists/{list}"),
            CONTRIBUTOR,
            Some(json!({ "name": "Mine", "expectedVersion": 1 })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&body), "forbidden");
    let (status, _) = h
        .call(
            Method::DELETE,
            &format!("/api/lists/{list}?expectedVersion=1"),
            CONTRIBUTOR,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn list_mutations_do_not_disclose_lists_to_strangers() {
    let h = harness();
    let list = h.create_list("Private").await;
    for target in [list.as_str(), "missing-list"] {
        let (status, _) = h
            .call(
                Method::PATCH,
                &format!("/api/lists/{target}"),
                STRANGER,
                Some(json!({ "name": "Mine", "expectedVersion": 1 })),
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = h
            .call(
                Method::DELETE,
                &format!("/api/lists/{target}?expectedVersion=1"),
                STRANGER,
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn owner_deletes_list_with_expected_version() {
    let h = harness();
    let list = h.create_list("Groceries").await;
    let uri = format!("/api/lists/{list}");
    let (status, _) = h.call(Method::DELETE, &uri, OWNER, None).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "expectedVersion is required"
    );
    let (status, _) = h
        .call(
            Method::DELETE,
            &format!("{uri}?expectedVersion=7"),
            OWNER,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = h
        .call(
            Method::DELETE,
            &format!("{uri}?expectedVersion=1"),
            OWNER,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = h
        .call(Method::GET, &format!("{uri}/items"), OWNER, None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// --- B2/B4: item routes ---------------------------------------------------------------------

#[tokio::test]
async fn item_lifecycle_with_optimistic_concurrency() {
    let h = harness();
    let list = h.create_list("Groceries").await;

    let (status, body) = h.add_item(&list, 1, "Milk").await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["listVersion"], 2);
    assert_eq!(body["item"]["name"], "Milk");
    assert_eq!(body["item"]["checked"], false);
    let item = body["item"]["id"].as_str().unwrap().to_owned();
    let item_version = body["item"]["itemVersion"].as_u64().unwrap();

    let (status, body) = h.add_item(&list, 1, "Stale").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "version_conflict");

    let item_uri = format!("/api/lists/{list}/items/{item}");
    let (status, body) = h
        .call(
            Method::PATCH,
            &item_uri,
            OWNER,
            Some(json!({
                "expectedVersion": item_version,
                "name": "Oat milk",
                "quantity": "2",
                "unit": "l"
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["item"]["name"], "Oat milk");
    assert_eq!(body["item"]["quantity"], "2");
    let item_version = body["item"]["itemVersion"].as_u64().unwrap();

    let (status, body) = h
        .call(
            Method::PATCH,
            &item_uri,
            OWNER,
            Some(json!({ "expectedVersion": item_version, "checked": true })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["item"]["checked"], true);
    let checked_version = body["item"]["itemVersion"].as_u64().unwrap();
    assert!(checked_version > item_version);

    let (status, _) = h
        .call(
            Method::PATCH,
            &item_uri,
            OWNER,
            Some(json!({ "expectedVersion": item_version, "checked": false })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, _) = h
        .call(
            Method::DELETE,
            &format!("{item_uri}?expectedVersion={item_version}"),
            OWNER,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, body) = h
        .call(
            Method::DELETE,
            &format!("{item_uri}?expectedVersion={checked_version}"),
            OWNER,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["listVersion"].as_u64().unwrap() > 2);

    let (status, _) = h
        .call(
            Method::PATCH,
            &item_uri,
            OWNER,
            Some(json!({ "expectedVersion": checked_version, "checked": false })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn item_update_rejects_mixed_or_empty_changes() {
    let h = harness();
    let list = h.create_list("Groceries").await;
    let (_, body) = h.add_item(&list, 1, "Milk").await;
    let item = body["item"]["id"].as_str().unwrap();
    let uri = format!("/api/lists/{list}/items/{item}");
    for payload in [
        json!({ "expectedVersion": 1 }),
        json!({ "expectedVersion": 1, "checked": true, "name": "Both" }),
        json!({ "expectedVersion": 1, "checked": true, "note": "Both" }),
        json!({ "expectedVersion": 1, "quantity": "2" }),
        json!({ "expectedVersion": 0, "checked": true }),
        json!({ "expectedVersion": 1, "name": "x".repeat(121) }),
    ] {
        let (status, _) = h.call(Method::PATCH, &uri, OWNER, Some(payload)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn contributors_edit_items_but_strangers_cannot_see_them() {
    let h = harness();
    let list = h.shared_list().await;
    let (status, body) = h
        .call(
            Method::POST,
            &format!("/api/lists/{list}/items"),
            CONTRIBUTOR,
            Some(json!({ "expectedListVersion": 1, "name": "Bread" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let item = body["item"]["id"].as_str().unwrap().to_owned();

    let (status, _) = h
        .call(
            Method::GET,
            &format!("/api/lists/{list}/items"),
            CONTRIBUTOR,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    for (method, uri, body) in [
        (
            Method::POST,
            format!("/api/lists/{list}/items"),
            Some(json!({ "expectedListVersion": 2, "name": "Intruder" })),
        ),
        (Method::GET, format!("/api/lists/{list}/items"), None),
        (
            Method::PATCH,
            format!("/api/lists/{list}/items/{item}"),
            Some(json!({ "expectedVersion": 1, "checked": true })),
        ),
        (
            Method::DELETE,
            format!("/api/lists/{list}/items/{item}?expectedVersion=1"),
            None,
        ),
        (
            Method::PUT,
            format!("/api/lists/{list}/item-order"),
            Some(json!({ "expectedListVersion": 2, "itemIds": [item] })),
        ),
        (Method::GET, format!("/api/lists/{list}/sync"), None),
    ] {
        let (status, _) = h.call(method.clone(), &uri, STRANGER, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_item_updates_with_the_same_version_admit_one_winner() {
    let h = Arc::new(harness());
    let list = h.create_list("Groceries").await;
    let (_, body) = h.add_item(&list, 1, "Milk").await;
    let item = body["item"]["id"].as_str().unwrap().to_owned();
    let version = body["item"]["itemVersion"].as_u64().unwrap();
    let uri = format!("/api/lists/{list}/items/{item}");
    let tasks: Vec<_> = (0..8)
        .map(|index| {
            let h = h.clone();
            let uri = uri.clone();
            tokio::spawn(async move {
                h.call(
                    Method::PATCH,
                    &uri,
                    OWNER,
                    Some(json!({ "expectedVersion": version, "checked": index % 2 == 0 })),
                )
                .await
                .0
            })
        })
        .collect();
    let mut winners = 0;
    for task in tasks {
        let status = task.await.unwrap();
        assert!(status == StatusCode::OK || status == StatusCode::CONFLICT);
        winners += usize::from(status == StatusCode::OK);
    }
    assert_eq!(winners, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_adds_with_the_same_list_version_admit_one_winner() {
    let h = Arc::new(harness());
    let list = h.create_list("Groceries").await;
    let tasks: Vec<_> = (0..8)
        .map(|index| {
            let h = h.clone();
            let list = list.clone();
            tokio::spawn(async move { h.add_item(&list, 1, &format!("Item {index}")).await.0 })
        })
        .collect();
    let mut created = 0;
    for task in tasks {
        let status = task.await.unwrap();
        assert!(status == StatusCode::CREATED || status == StatusCode::CONFLICT);
        created += usize::from(status == StatusCode::CREATED);
    }
    assert_eq!(created, 1);
}

#[tokio::test]
async fn reorder_items_with_expected_list_version() {
    let h = harness();
    let list = h.create_list("Groceries").await;
    let (_, first) = h.add_item(&list, 1, "A").await;
    let (_, second) = h.add_item(&list, 2, "B").await;
    let a = first["item"]["id"].as_str().unwrap().to_owned();
    let b = second["item"]["id"].as_str().unwrap().to_owned();
    let uri = format!("/api/lists/{list}/item-order");

    let (status, _) = h
        .call(
            Method::PUT,
            &uri,
            OWNER,
            Some(json!({ "expectedListVersion": 1, "itemIds": [b, a] })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, body) = h
        .call(
            Method::PUT,
            &uri,
            OWNER,
            Some(json!({ "expectedListVersion": 3, "itemIds": [b, a] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["listVersion"], 4);
    let positions: Vec<(String, u64)> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["id"].as_str().unwrap().to_owned(),
                item["position"].as_u64().unwrap(),
            )
        })
        .collect();
    assert!(positions.contains(&(b.clone(), 0)));
    assert!(positions.contains(&(a.clone(), 1)));

    let too_many: Vec<String> = (0..101).map(|i| format!("id-{i}")).collect();
    let (status, _) = h
        .call(
            Method::PUT,
            &uri,
            OWNER,
            Some(json!({ "expectedListVersion": 4, "itemIds": too_many })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// --- B3: pagination -------------------------------------------------------------------------

#[tokio::test]
async fn items_are_paginated_with_limit_and_cursor() {
    let h = harness();
    let list = h.create_list("Groceries").await;
    for (version, name) in [(1, "A"), (2, "B"), (3, "C")] {
        assert_eq!(
            h.add_item(&list, version, name).await.0,
            StatusCode::CREATED
        );
    }
    let base = format!("/api/lists/{list}/items");
    let (status, first) = h
        .call(Method::GET, &format!("{base}?limit=2"), OWNER, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["items"].as_array().unwrap().len(), 2);
    let cursor = first["nextCursor"].as_str().unwrap().to_owned();

    let (status, second) = h
        .call(
            Method::GET,
            &format!("{base}?limit=2&cursor={cursor}"),
            OWNER,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    assert!(second.get("nextCursor").is_none());

    for query in ["limit=0", "limit=101", "limit=abc", "cursor=%20bad"] {
        let (status, _) = h
            .call(Method::GET, &format!("{base}?{query}"), OWNER, None)
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
    }
}

#[tokio::test]
async fn lists_are_paginated_with_limit_and_cursor() {
    let h = harness();
    h.create_list("One").await;
    h.create_list("Two").await;
    let (status, first) = h.call(Method::GET, "/api/lists?limit=1", OWNER, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["items"].as_array().unwrap().len(), 1);
    let cursor = first["nextCursor"].as_str().unwrap();
    let (status, second) = h
        .call(
            Method::GET,
            &format!("/api/lists?limit=1&cursor={cursor}"),
            OWNER,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(first["items"][0]["id"], second["items"][0]["id"]);
    let (status, _) = h.call(Method::GET, "/api/lists?limit=0", OWNER, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, stranger) = h.call(Method::GET, "/api/lists", STRANGER, None).await;
    assert_eq!(stranger["items"].as_array().unwrap().len(), 0);
}

// --- B5: sync -------------------------------------------------------------------------------

#[tokio::test]
async fn sync_returns_snapshot_then_changes_since_version() {
    let h = harness();
    let list = h.create_list("Groceries").await;
    let (_, added) = h.add_item(&list, 1, "Milk").await;
    let item = added["item"]["id"].as_str().unwrap().to_owned();
    let uri = format!("/api/lists/{list}/sync");

    let (status, snapshot) = h.call(Method::GET, &uri, OWNER, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(snapshot["kind"], "snapshot");
    assert_eq!(snapshot["version"], 2);
    assert_eq!(snapshot["items"][0]["id"], item.as_str());

    let (status, unchanged) = h
        .call(Method::GET, &format!("{uri}?since=2"), OWNER, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unchanged["kind"], "changes");
    assert_eq!(unchanged["since"], 2);
    assert_eq!(unchanged["changed"].as_array().unwrap().len(), 0);
    assert_eq!(unchanged["removed"].as_array().unwrap().len(), 0);

    let item_version = added["item"]["itemVersion"].as_u64().unwrap();
    let (status, _) = h
        .call(
            Method::DELETE,
            &format!("/api/lists/{list}/items/{item}?expectedVersion={item_version}"),
            OWNER,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, changes) = h
        .call(Method::GET, &format!("{uri}?since=2"), OWNER, None)
        .await;
    assert_eq!(changes["kind"], "changes");
    assert_eq!(changes["removed"][0], item.as_str());

    let (status, _) = h
        .call(Method::GET, &format!("{uri}?since=0"), OWNER, None)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// --- B7: profile ----------------------------------------------------------------------------

#[tokio::test]
async fn profile_is_created_read_and_updated_for_the_verified_principal() {
    let h = harness();
    let (status, _) = h.call(Method::GET, "/api/me", OWNER, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, created) = h
        .call(
            Method::PUT,
            "/api/me",
            OWNER,
            Some(json!({ "displayName": "Anna" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["id"], OWNER);
    assert_eq!(created["version"], 1);

    let (status, _) = h
        .call(
            Method::PUT,
            "/api/me",
            OWNER,
            Some(json!({ "displayName": "Again" })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, updated) = h
        .call(
            Method::PUT,
            "/api/me",
            OWNER,
            Some(json!({ "displayName": "Anna B.", "expectedVersion": 1 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["version"], 2);

    let (status, _) = h
        .call(
            Method::PUT,
            "/api/me",
            OWNER,
            Some(json!({ "displayName": "Stale", "expectedVersion": 1 })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    for payload in [
        json!({ "displayName": "  ", "expectedVersion": 2 }),
        json!({ "displayName": "x".repeat(81), "expectedVersion": 2 }),
        json!({ "displayName": "Anna", "expectedVersion": 0 }),
    ] {
        let (status, _) = h.call(Method::PUT, "/api/me", OWNER, Some(payload)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let (status, _) = h
        .call(
            Method::PUT,
            "/api/me",
            OWNER,
            Some(json!({ "displayName": "Anna", "id": STRANGER })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, read) = h.call(Method::GET, "/api/me", OWNER, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read["displayName"], "Anna B.");
    let (status, _) = h.call(Method::GET, "/api/me", STRANGER, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// --- B8: abuse controls ---------------------------------------------------------------------

#[tokio::test]
async fn oversized_request_bodies_are_rejected() {
    let h = harness();
    let name = "x".repeat(MAX_REQUEST_BODY_BYTES);
    let (status, _) = h
        .call(
            Method::POST,
            "/api/lists",
            OWNER,
            Some(json!({ "name": name })),
        )
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn principals_are_throttled_independently_with_retry_after() {
    let h = harness_with_limiter(TokenBucketRateLimiter::try_new(2, 1, 100).unwrap());
    for _ in 0..2 {
        let (status, _) = h.call(Method::GET, "/api/lists", OWNER, None).await;
        assert_eq!(status, StatusCode::OK);
    }
    let response = h.call_raw(Method::GET, "/api/lists", OWNER, None).await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::RETRY_AFTER)
            .unwrap(),
        "1"
    );
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(error_code(&body), "rate_limited");

    let (status, _) = h.call(Method::GET, "/api/lists", STRANGER, None).await;
    assert_eq!(status, StatusCode::OK);
}

struct BrokenLimiter;

impl RateLimiter for BrokenLimiter {
    fn check(
        &self,
        _principal: &UserId,
    ) -> Result<RateLimitDecision, einkaufio_application::RateLimitError> {
        Err(einkaufio_application::RateLimitError::Unavailable)
    }
}

#[tokio::test]
async fn throttling_fails_closed_when_the_limiter_is_unavailable() {
    let state = ApiState::new(
        Arc::new(InMemoryRepository::new()),
        Arc::new(Sequence::default()),
        Arc::new(Sequence::default()),
        Arc::new(BrokenLimiter),
    );
    let protected = protected_routes::<InMemoryRepository>()
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            throttle_principal::<InMemoryRepository>,
        ))
        .route_layer(middleware::from_fn(inject_header_actor));
    let app = build_router(state, protected);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/lists")
                .header(TEST_ACTOR_HEADER, OWNER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

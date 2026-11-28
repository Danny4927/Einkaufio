//! Authorization matrix (roadmap Phase C exit criterion): every documented endpoint ×
//! {unauthenticated, outsider, member, owner}.
//!
//! The fixture is a household owned by `matrix-owner` with `matrix-member` as active member
//! (joined through an invitation over HTTP), a household list with one item and an open
//! invitation. `matrix-outsider` is authenticated but belongs to nothing. Each cell runs on a
//! fresh fixture so that one cell cannot influence another. Lists and items are the household
//! list, so "owner" is the list administrator and "member" a contributor (ADR 015).
//!
//! A guard test parses the OpenAPI contract and fails if an operation is documented but not
//! covered here, or covered here but not documented.

use std::collections::BTreeSet;

use super::*;

const MATRIX_OWNER: &str = "matrix-owner";
const MATRIX_MEMBER: &str = "matrix-member";
const MATRIX_OUTSIDER: &str = "matrix-outsider";

const OPENAPI: &str = include_str!("../../../../../../contracts/openapi/openapi.yaml");

/// Operations in the contract that are deliberately not implemented in v1 (ADR 015).
const DEFERRED_OPERATIONS: &[(&str, &str)] = &[("POST", "/api/lists/{}/invites")];

struct Fixture {
    h: Harness,
    values: Vec<(&'static str, String)>,
}

impl Fixture {
    fn render(&self, template: &str) -> String {
        self.values
            .iter()
            .fold(template.to_owned(), |text, (key, value)| {
                text.replace(key, value)
            })
    }

    /// Replaces every resource id with one that does not exist.
    fn render_missing(&self, template: &str) -> String {
        let missing = template
            .replace("{household}", "missing-household")
            .replace("{list}", "missing-list")
            .replace("{item}", "missing-item")
            .replace("{invite}", "missing-invite");
        self.render(&missing)
    }

    async fn send(
        &self,
        method: &Method,
        uri: &str,
        actor: &str,
        body: Option<String>,
    ) -> (StatusCode, Value) {
        let body = body.map(|text| serde_json::from_str(&text).expect("valid case body"));
        self.h.call(method.clone(), uri, actor, body).await
    }
}

async fn expect_status(h: &Harness, method: Method, uri: &str, actor: &str, body: Value) -> Value {
    let (status, value) = h.call(method.clone(), uri, actor, Some(body)).await;
    assert!(
        status.is_success(),
        "fixture {method} {uri}: {status} {value}"
    );
    value
}

async fn fixture() -> Fixture {
    let h = harness();
    for actor in [MATRIX_OWNER, MATRIX_MEMBER, MATRIX_OUTSIDER] {
        expect_status(
            &h,
            Method::PUT,
            "/api/me",
            actor,
            json!({ "displayName": actor }),
        )
        .await;
    }
    let household = expect_status(
        &h,
        Method::POST,
        "/api/households",
        MATRIX_OWNER,
        json!({ "name": "Matrix" }),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let invites = format!("/api/households/{household}/invites");
    let join = expect_status(
        &h,
        Method::POST,
        &invites,
        MATRIX_OWNER,
        json!({ "maxUses": 1 }),
    )
    .await["token"]
        .as_str()
        .unwrap()
        .to_owned();
    expect_status(
        &h,
        Method::POST,
        "/api/invites/accept",
        MATRIX_MEMBER,
        json!({ "token": join }),
    )
    .await;
    let open = expect_status(
        &h,
        Method::POST,
        &invites,
        MATRIX_OWNER,
        json!({ "maxUses": 5 }),
    )
    .await;
    let list = expect_status(
        &h,
        Method::POST,
        &format!("/api/households/{household}/lists"),
        MATRIX_OWNER,
        json!({ "name": "Gemeinsam" }),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let added = expect_status(
        &h,
        Method::POST,
        &format!("/api/lists/{list}/items"),
        MATRIX_OWNER,
        json!({ "expectedListVersion": 1, "name": "Milch" }),
    )
    .await;
    let (_, details) = h
        .call(
            Method::GET,
            &format!("/api/households/{household}"),
            MATRIX_OWNER,
            None,
        )
        .await;
    let values = vec![
        ("{household}", household),
        ("{list}", list),
        ("{item}", added["item"]["id"].as_str().unwrap().to_owned()),
        ("{invite}", open["inviteId"].as_str().unwrap().to_owned()),
        ("{token}", open["token"].as_str().unwrap().to_owned()),
        ("{member}", MATRIX_MEMBER.to_owned()),
        ("{hv}", details["version"].to_string()),
        ("{lv}", added["listVersion"].to_string()),
        ("{iv}", added["item"]["itemVersion"].to_string()),
    ];
    Fixture { h, values }
}

/// One endpoint with the expected status for `[outsider, member, owner]`.
struct Case {
    method: Method,
    path: &'static str,
    body: Option<&'static str>,
    expected: [StatusCode; 3],
}

const OK: StatusCode = StatusCode::OK;
const CREATED: StatusCode = StatusCode::CREATED;
const NO_CONTENT: StatusCode = StatusCode::NO_CONTENT;
const FORBIDDEN: StatusCode = StatusCode::FORBIDDEN;
const NOT_FOUND: StatusCode = StatusCode::NOT_FOUND;
const CONFLICT: StatusCode = StatusCode::CONFLICT;

fn case(
    method: Method,
    path: &'static str,
    body: Option<&'static str>,
    expected: [StatusCode; 3],
) -> Case {
    Case {
        method,
        path,
        body,
        expected,
    }
}

fn cases() -> Vec<Case> {
    vec![
        // Households (C2).
        case(Method::GET, "/api/households", None, [OK, OK, OK]),
        case(
            Method::POST,
            "/api/households",
            Some(r#"{"name":"Neu"}"#),
            [CREATED, CREATED, CREATED],
        ),
        case(
            Method::GET,
            "/api/households/{household}",
            None,
            [NOT_FOUND, OK, OK],
        ),
        case(
            Method::PATCH,
            "/api/households/{household}",
            Some(r#"{"name":"Umbenannt","expectedVersion":{hv}}"#),
            [NOT_FOUND, FORBIDDEN, OK],
        ),
        case(
            Method::DELETE,
            "/api/households/{household}?expectedVersion={hv}",
            None,
            [NOT_FOUND, FORBIDDEN, NO_CONTENT],
        ),
        case(
            Method::POST,
            "/api/households/{household}/lists",
            Some(r#"{"name":"Neu"}"#),
            [NOT_FOUND, CREATED, CREATED],
        ),
        // Membership lifecycle (C3).
        case(
            Method::GET,
            "/api/households/{household}/members",
            None,
            [NOT_FOUND, OK, OK],
        ),
        case(
            Method::DELETE,
            "/api/households/{household}/members/{member}?expectedVersion={hv}",
            None,
            [NOT_FOUND, FORBIDDEN, OK],
        ),
        case(
            Method::POST,
            "/api/households/{household}/leave",
            None,
            [NOT_FOUND, NO_CONTENT, CONFLICT],
        ),
        case(
            Method::POST,
            "/api/households/{household}/transfer-ownership",
            Some(r#"{"newOwnerId":"{member}","expectedVersion":{hv}}"#),
            [NOT_FOUND, FORBIDDEN, OK],
        ),
        // Invitations (C4).
        case(
            Method::GET,
            "/api/households/{household}/invites",
            None,
            [NOT_FOUND, FORBIDDEN, OK],
        ),
        case(
            Method::POST,
            "/api/households/{household}/invites",
            Some("{}"),
            [NOT_FOUND, FORBIDDEN, CREATED],
        ),
        case(
            Method::DELETE,
            "/api/households/{household}/invites/{invite}",
            None,
            [NOT_FOUND, FORBIDDEN, NO_CONTENT],
        ),
        case(
            Method::POST,
            "/api/invites/accept",
            Some(r#"{"token":"{token}"}"#),
            [OK, CONFLICT, CONFLICT],
        ),
        // Lists (A6) on the household list.
        case(Method::GET, "/api/lists", None, [OK, OK, OK]),
        case(
            Method::POST,
            "/api/lists",
            Some(r#"{"name":"Privat"}"#),
            [CREATED, CREATED, CREATED],
        ),
        case(
            Method::PATCH,
            "/api/lists/{list}",
            Some(r#"{"name":"Neu","expectedVersion":{lv}}"#),
            [NOT_FOUND, FORBIDDEN, OK],
        ),
        case(
            Method::DELETE,
            "/api/lists/{list}?expectedVersion={lv}",
            None,
            [NOT_FOUND, FORBIDDEN, NO_CONTENT],
        ),
        // Items and sync (B2-B5).
        case(
            Method::GET,
            "/api/lists/{list}/items",
            None,
            [NOT_FOUND, OK, OK],
        ),
        case(
            Method::POST,
            "/api/lists/{list}/items",
            Some(r#"{"expectedListVersion":{lv},"name":"Brot"}"#),
            [NOT_FOUND, CREATED, CREATED],
        ),
        case(
            Method::PATCH,
            "/api/lists/{list}/items/{item}",
            Some(r#"{"expectedVersion":{iv},"checked":true}"#),
            [NOT_FOUND, OK, OK],
        ),
        case(
            Method::DELETE,
            "/api/lists/{list}/items/{item}?expectedVersion={iv}",
            None,
            [NOT_FOUND, OK, OK],
        ),
        case(
            Method::PUT,
            "/api/lists/{list}/item-order",
            Some(r#"{"expectedListVersion":{lv},"itemIds":["{item}"]}"#),
            [NOT_FOUND, OK, OK],
        ),
        case(
            Method::GET,
            "/api/lists/{list}/sync",
            None,
            [NOT_FOUND, OK, OK],
        ),
        // The caller's own account (B7, D2): self-scoped, no foreign resource is addressable.
        case(Method::GET, "/api/me", None, [OK, OK, OK]),
        case(
            Method::PUT,
            "/api/me",
            Some(r#"{"displayName":"Neu","expectedVersion":1}"#),
            [OK, OK, OK],
        ),
        case(Method::GET, "/api/me/export", None, [OK, OK, OK]),
        // The owner of a household with other members must transfer first (ADR 018).
        case(
            Method::DELETE,
            "/api/me",
            None,
            [NO_CONTENT, NO_CONTENT, CONFLICT],
        ),
    ]
}

#[tokio::test]
async fn every_endpoint_enforces_the_authorization_matrix() {
    for case in cases() {
        for (actor, expected) in [MATRIX_OUTSIDER, MATRIX_MEMBER, MATRIX_OWNER]
            .into_iter()
            .zip(case.expected)
        {
            let f = fixture().await;
            let uri = f.render(case.path);
            let body = case.body.map(|text| f.render(text));
            let (status, value) = f.send(&case.method, &uri, actor, body.clone()).await;
            assert_eq!(
                status, expected,
                "{} {} as {actor}: {value}",
                case.method, case.path
            );
            if actor == MATRIX_OUTSIDER && expected == NOT_FOUND {
                // No existence leak: an inaccessible resource looks exactly like a missing one.
                let missing_uri = f.render_missing(case.path);
                let missing_body = case.body.map(|text| f.render_missing(text));
                let missing = f
                    .send(&case.method, &missing_uri, actor, missing_body)
                    .await;
                assert_eq!(
                    (status, value),
                    missing,
                    "{} {} leaks existence to outsiders",
                    case.method,
                    case.path
                );
            }
        }
    }
}

#[tokio::test]
async fn every_endpoint_requires_authentication() {
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
        test_household_services(),
    )
    .with_authenticator(verifier);
    let app = router(state);
    let f = fixture().await;
    for case in cases() {
        let uri = f.render(case.path);
        let mut builder = Request::builder().method(case.method.clone()).uri(&uri);
        let body = match case.body {
            Some(text) => {
                builder = builder.header("content-type", "application/json");
                Body::from(f.render(text))
            }
            None => Body::empty(),
        };
        let response = app
            .clone()
            .oneshot(builder.body(body).unwrap())
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{} {}",
            case.method,
            case.path
        );
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::WWW_AUTHENTICATE)
                .unwrap(),
            "Bearer"
        );
    }
}

/// `{name}` path parameters normalised to `{}` so contract and matrix spellings compare.
fn normalise(path: &str) -> String {
    let path = path.split('?').next().unwrap_or_default();
    let mut normalised = String::with_capacity(path.len());
    let mut in_parameter = false;
    for character in path.chars() {
        match character {
            '{' => {
                in_parameter = true;
                normalised.push_str("{}");
            }
            '}' => in_parameter = false,
            _ if in_parameter => {}
            _ => normalised.push(character),
        }
    }
    normalised
}

fn documented_operations() -> BTreeSet<(String, String)> {
    let mut operations = BTreeSet::new();
    let mut current: Option<String> = None;
    for line in OPENAPI.lines() {
        if !line.starts_with(' ') && !line.is_empty() && line != "paths:" {
            current = None;
        }
        if let Some(path) = line
            .strip_prefix("  /")
            .and_then(|rest| rest.strip_suffix(':'))
        {
            current = Some(format!("/{path}"));
            continue;
        }
        if let (Some(path), Some(method)) = (
            current.as_ref(),
            line.strip_prefix("    ")
                .and_then(|rest| rest.strip_suffix(':'))
                .filter(|method| ["get", "post", "put", "patch", "delete"].contains(method)),
        ) {
            operations.insert((method.to_uppercase(), normalise(path)));
        }
    }
    operations
}

#[test]
fn matrix_covers_exactly_the_documented_operations() {
    let mut documented = documented_operations();
    assert!(documented.remove(&("GET".to_owned(), "/health".to_owned())));
    for (method, path) in DEFERRED_OPERATIONS {
        assert!(
            documented.remove(&((*method).to_owned(), (*path).to_owned())),
            "deferred operation {method} {path} is no longer documented"
        );
    }
    let covered: BTreeSet<(String, String)> = cases()
        .iter()
        .map(|case| (case.method.to_string(), normalise(case.path)))
        .collect();
    assert_eq!(documented, covered);
}

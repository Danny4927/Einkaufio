//! Route tests for households (C2), the membership lifecycle (C3) and invitations (C4).

use super::*;

const MEMBER: &str = "hh-member";
const GUEST: &str = "hh-guest";

impl Harness {
    async fn create_household(&self, actor: &str, name: &str) -> Value {
        let (status, body) = self
            .call(
                Method::POST,
                "/api/households",
                actor,
                Some(json!({ "name": name })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body
    }

    async fn invite(&self, household: &str, body: Value) -> (StatusCode, Value) {
        self.call(
            Method::POST,
            &format!("/api/households/{household}/invites"),
            OWNER,
            Some(body),
        )
        .await
    }

    async fn accept(&self, actor: &str, token: &str) -> (StatusCode, Value) {
        self.call(
            Method::POST,
            "/api/invites/accept",
            actor,
            Some(json!({ "token": token })),
        )
        .await
    }

    /// A household owned by OWNER with MEMBER joined through an invitation.
    async fn household_with_member(&self) -> String {
        let household = self.create_household(OWNER, "Familie").await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let (_, invite) = self.invite(&household, json!({})).await;
        let (status, joined) = self.accept(MEMBER, invite["token"].as_str().unwrap()).await;
        assert_eq!(status, StatusCode::OK, "{joined}");
        household
    }

    async fn household_version(&self, household: &str) -> u64 {
        let (_, body) = self
            .call(
                Method::GET,
                &format!("/api/households/{household}"),
                OWNER,
                None,
            )
            .await;
        body["version"].as_u64().unwrap()
    }
}

#[tokio::test]
async fn households_are_created_listed_read_and_renamed_with_versions() {
    let h = harness();
    let created = h.create_household(OWNER, "  Familie ").await;
    assert_eq!(created["name"], "Familie");
    assert_eq!(created["role"], "OWNER");
    assert_eq!(created["version"], 1);
    let id = created["id"].as_str().unwrap().to_owned();

    let (status, listed) = h.call(Method::GET, "/api/households", OWNER, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["items"][0]["id"], id.as_str());
    let (_, foreign) = h.call(Method::GET, "/api/households", STRANGER, None).await;
    assert_eq!(foreign["items"].as_array().unwrap().len(), 0);

    let uri = format!("/api/households/{id}");
    let (status, details) = h.call(Method::GET, &uri, OWNER, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(details["members"][0]["userId"], OWNER);
    assert_eq!(details["members"][0]["role"], "OWNER");

    let (status, renamed) = h
        .call(
            Method::PATCH,
            &uri,
            OWNER,
            Some(json!({ "name": "WG", "expectedVersion": 1 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["name"], "WG");
    assert_eq!(renamed["version"], 2);
    let (status, body) = h
        .call(
            Method::PATCH,
            &uri,
            OWNER,
            Some(json!({ "name": "Stale", "expectedVersion": 1 })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "version_conflict");
}

#[tokio::test]
async fn household_input_is_validated_and_owner_cannot_be_supplied() {
    let h = harness();
    for name in ["   ", &"x".repeat(81)] {
        let (status, _) = h
            .call(
                Method::POST,
                "/api/households",
                OWNER,
                Some(json!({ "name": name })),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let (status, _) = h
        .call(
            Method::POST,
            "/api/households",
            OWNER,
            Some(json!({ "name": "Familie", "ownerId": STRANGER })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let id = h.create_household(OWNER, "Familie").await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, _) = h
        .call(
            Method::DELETE,
            &format!("/api/households/{id}"),
            OWNER,
            None,
        )
        .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "expectedVersion is required"
    );
}

#[tokio::test]
async fn household_limit_is_reported() {
    let h = harness();
    for index in 0..5 {
        h.create_household(OWNER, &format!("H{index}")).await;
    }
    let (status, body) = h
        .call(
            Method::POST,
            "/api/households",
            OWNER,
            Some(json!({ "name": "Zu viel" })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "household_limit_reached");
}

#[tokio::test]
async fn members_create_household_lists_that_appear_in_their_list_overview() {
    let h = harness();
    let household = h.household_with_member().await;
    let (status, list) = h
        .call(
            Method::POST,
            &format!("/api/households/{household}/lists"),
            MEMBER,
            Some(json!({ "name": "Wocheneinkauf" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(list["type"], "HOUSEHOLD");
    assert_eq!(list["householdId"], household.as_str());

    for actor in [OWNER, MEMBER] {
        let (_, lists) = h.call(Method::GET, "/api/lists", actor, None).await;
        assert_eq!(lists["items"][0]["id"], list["id"]);
        assert_eq!(lists["items"][0]["type"], "HOUSEHOLD");
    }
    let (_, personal) = h
        .call(
            Method::POST,
            "/api/lists",
            OWNER,
            Some(json!({ "name": "Privat" })),
        )
        .await;
    assert_eq!(personal["type"], "PERSONAL");
    assert!(personal.get("householdId").is_none());

    let (status, _) = h
        .call(
            Method::POST,
            &format!("/api/households/{household}/lists"),
            MEMBER,
            Some(json!({ "name": " " })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn household_list_limit_is_reported() {
    let h = harness();
    let household = h.create_household(OWNER, "Familie").await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let uri = format!("/api/households/{household}/lists");
    for index in 0..20 {
        let (status, _) = h
            .call(
                Method::POST,
                &uri,
                OWNER,
                Some(json!({ "name": format!("L{index}") })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let (status, body) = h
        .call(Method::POST, &uri, OWNER, Some(json!({ "name": "L20" })))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "list_limit_reached");
}

#[tokio::test]
async fn leaving_and_removal_end_list_access() {
    let h = harness();
    let household = h.household_with_member().await;
    let (_, list) = h
        .call(
            Method::POST,
            &format!("/api/households/{household}/lists"),
            OWNER,
            Some(json!({ "name": "Gemeinsam" })),
        )
        .await;
    let items = format!("/api/lists/{}/items", list["id"].as_str().unwrap());
    assert_eq!(
        h.call(Method::GET, &items, MEMBER, None).await.0,
        StatusCode::OK
    );

    let (status, _) = h
        .call(
            Method::POST,
            &format!("/api/households/{household}/leave"),
            MEMBER,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        h.call(Method::GET, &items, MEMBER, None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.call(
            Method::GET,
            &format!("/api/households/{household}"),
            MEMBER,
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    // Rejoin, then the owner removes the member with the expected version.
    let (_, invite) = h.invite(&household, json!({})).await;
    h.accept(MEMBER, invite["token"].as_str().unwrap()).await;
    let version = h.household_version(&household).await;
    let member_uri = format!("/api/households/{household}/members/{MEMBER}");
    let (status, _) = h
        .call(
            Method::DELETE,
            &format!("{member_uri}?expectedVersion={}", version - 1),
            OWNER,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, body) = h
        .call(
            Method::DELETE,
            &format!("{member_uri}?expectedVersion={version}"),
            OWNER,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["version"], version + 1);
    assert_eq!(
        h.call(Method::GET, &items, MEMBER, None).await.0,
        StatusCode::NOT_FOUND
    );
    let (_, members) = h
        .call(
            Method::GET,
            &format!("/api/households/{household}/members"),
            OWNER,
            None,
        )
        .await;
    assert_eq!(members["items"].as_array().unwrap().len(), 1);

    // Removing a non-member or the owner.
    let version = version + 1;
    let (status, _) = h
        .call(
            Method::DELETE,
            &format!("/api/households/{household}/members/{STRANGER}?expectedVersion={version}"),
            OWNER,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, body) = h
        .call(
            Method::DELETE,
            &format!("/api/households/{household}/members/{OWNER}?expectedVersion={version}"),
            OWNER,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "owner_must_transfer_ownership");
}

#[tokio::test]
async fn ownership_transfer_moves_administration() {
    let h = harness();
    let household = h.household_with_member().await;
    let version = h.household_version(&household).await;
    let uri = format!("/api/households/{household}/transfer-ownership");
    let (status, _) = h
        .call(
            Method::POST,
            &uri,
            OWNER,
            Some(json!({ "newOwnerId": STRANGER, "expectedVersion": version })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "target must be a member");
    let (status, body) = h
        .call(
            Method::POST,
            &uri,
            OWNER,
            Some(json!({ "newOwnerId": MEMBER, "expectedVersion": version })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["role"], "MEMBER",
        "the caller's role after the transfer"
    );
    assert_eq!(body["version"], version + 1);

    let rename = format!("/api/households/{household}");
    let payload = json!({ "name": "Neu", "expectedVersion": version + 1 });
    assert_eq!(
        h.call(Method::PATCH, &rename, OWNER, Some(payload.clone()))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        h.call(Method::PATCH, &rename, MEMBER, Some(payload))
            .await
            .0,
        StatusCode::OK
    );
    // The former owner may now leave.
    assert_eq!(
        h.call(
            Method::POST,
            &format!("/api/households/{household}/leave"),
            OWNER,
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn deleting_a_household_removes_its_lists_for_everyone() {
    let h = harness();
    let household = h.household_with_member().await;
    let (_, list) = h
        .call(
            Method::POST,
            &format!("/api/households/{household}/lists"),
            MEMBER,
            Some(json!({ "name": "Gemeinsam" })),
        )
        .await;
    let version = h.household_version(&household).await;
    let (status, _) = h
        .call(
            Method::DELETE,
            &format!("/api/households/{household}?expectedVersion={version}"),
            OWNER,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    for actor in [OWNER, MEMBER] {
        let (_, lists) = h.call(Method::GET, "/api/lists", actor, None).await;
        assert!(lists["items"].as_array().unwrap().is_empty());
        let (status, _) = h
            .call(
                Method::GET,
                &format!("/api/lists/{}/items", list["id"].as_str().unwrap()),
                actor,
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn invitation_creation_returns_the_token_once_with_a_stable_https_url() {
    let h = harness();
    let household = h.create_household(OWNER, "Familie").await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let response = h
        .call_raw(
            Method::POST,
            &format!("/api/households/{household}/invites"),
            OWNER,
            Some(json!({ "maxUses": 2, "expiresInDays": 3 })),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::CACHE_CONTROL)
            .unwrap(),
        "no-store"
    );
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let created: Value = serde_json::from_slice(&bytes).unwrap();
    let token = created["token"].as_str().unwrap().to_owned();
    assert_eq!(token.len(), 43, "256 bits as unpadded base64url");
    assert_eq!(
        created["inviteUrl"],
        format!("https://app.example.test/invite#token={token}")
    );
    assert_eq!(created["maxUses"], 2);
    let expires_at = created["expiresAt"].as_str().unwrap();
    assert!(
        expires_at.ends_with('Z') && expires_at.len() == 20,
        "{expires_at}"
    );

    // Listing never discloses the token, the URL or the hash.
    let (status, listed) = h
        .call(
            Method::GET,
            &format!("/api/households/{household}/invites"),
            OWNER,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!listed.to_string().contains(&token));
    assert_eq!(listed["items"][0]["inviteId"], created["inviteId"]);
    assert_eq!(listed["items"][0]["status"], "ACTIVE");
    assert_eq!(listed["items"][0]["usedCount"], 0);
    assert!(listed["items"][0].get("inviteUrl").is_none());
    assert!(listed["items"][0].get("token").is_none());

    for body in [
        json!({ "expiresInDays": 31 }),
        json!({ "expiresInDays": 0 }),
        json!({ "maxUses": 11 }),
        json!({ "maxUses": 0 }),
    ] {
        let (status, _) = h.invite(&household, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let (status, _) = h
        .invite(&household, json!({ "householdId": "other" }))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn acceptance_joins_once_and_unusable_tokens_look_the_same() {
    let h = harness();
    let household = h.create_household(OWNER, "Familie").await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, created) = h.invite(&household, json!({ "maxUses": 2 })).await;
    let token = created["token"].as_str().unwrap().to_owned();

    let (status, joined) = h.accept(MEMBER, &token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(joined["id"], household.as_str());
    assert_eq!(joined["role"], "MEMBER");
    // A repeated acceptance consumes no use: GUEST can still take the second one.
    let (status, body) = h.accept(MEMBER, &token).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "already_member");
    assert_eq!(h.accept(GUEST, &token).await.0, StatusCode::OK);

    // Used up, unknown and malformed tokens get one identical answer that echoes nothing.
    let (used_status, used) = h.accept("hh-late", &token).await;
    assert_eq!(used_status, StatusCode::GONE);
    assert_eq!(error_code(&used), "invitation_unavailable");
    assert!(!used.to_string().contains(&token));
    for other in ["A".repeat(43), "short".to_owned(), "x".repeat(400)] {
        assert_eq!(
            h.accept("hh-late", &other).await,
            (used_status, used.clone())
        );
    }

    // The token is only accepted in the JSON body.
    let (status, _) = h
        .call(
            Method::POST,
            &format!("/api/invites/accept?token={token}"),
            GUEST,
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn revoked_invitations_cannot_be_accepted() {
    let h = harness();
    let household = h.create_household(OWNER, "Familie").await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, created) = h.invite(&household, json!({})).await;
    let invite = created["inviteId"].as_str().unwrap();
    let uri = format!("/api/households/{household}/invites/{invite}");
    for _ in 0..2 {
        let (status, _) = h.call(Method::DELETE, &uri, OWNER, None).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "revoking is idempotent");
    }
    let (status, body) = h.accept(GUEST, created["token"].as_str().unwrap()).await;
    assert_eq!(status, StatusCode::GONE);
    assert_eq!(error_code(&body), "invitation_unavailable");
    let (_, listed) = h
        .call(
            Method::GET,
            &format!("/api/households/{household}/invites"),
            OWNER,
            None,
        )
        .await;
    assert_eq!(listed["items"][0]["status"], "REVOKED");
    let (status, _) = h
        .call(
            Method::DELETE,
            &format!("/api/households/{household}/invites/unknown"),
            OWNER,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn valid_invitation_limit_is_reported() {
    let h = harness();
    let household = h.create_household(OWNER, "Familie").await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    for _ in 0..5 {
        assert_eq!(h.invite(&household, json!({})).await.0, StatusCode::CREATED);
    }
    let (status, body) = h.invite(&household, json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "invitation_limit_reached");
}

#[tokio::test]
async fn member_limit_is_reported_on_acceptance() {
    let h = harness();
    let household = h.create_household(OWNER, "Familie").await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, created) = h.invite(&household, json!({ "maxUses": 10 })).await;
    let token = created["token"].as_str().unwrap().to_owned();
    for index in 0..9 {
        let (status, _) = h.accept(&format!("joiner-{index}"), &token).await;
        assert_eq!(status, StatusCode::OK);
    }
    let (status, body) = h.accept("joiner-9", &token).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "member_limit_reached");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_acceptances_never_exceed_max_uses() {
    let h = Arc::new(harness());
    let household = h.create_household(OWNER, "Familie").await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, created) = h.invite(&household, json!({ "maxUses": 2 })).await;
    let token = created["token"].as_str().unwrap().to_owned();
    let tasks: Vec<_> = (0..8)
        .map(|index| {
            let h = h.clone();
            let token = token.clone();
            tokio::spawn(async move { h.accept(&format!("racer-{index}"), &token).await.0 })
        })
        .collect();
    let mut joined = 0;
    for task in tasks {
        let status = task.await.unwrap();
        assert!(
            status == StatusCode::OK || status == StatusCode::GONE,
            "{status}"
        );
        joined += usize::from(status == StatusCode::OK);
    }
    assert_eq!(joined, 2);
}

//! Route tests for account deletion and data export (roadmap D2, ADR 018).

use super::*;

const ANNA: &str = "acct-anna";
const BEN: &str = "acct-ben";

impl Harness {
    async fn ok(&self, method: Method, uri: &str, actor: &str, body: Option<Value>) -> Value {
        let (status, value) = self.call(method.clone(), uri, actor, body).await;
        assert!(
            status.is_success(),
            "{method} {uri} as {actor}: {status} {value}"
        );
        value
    }

    async fn id_of(&self, method: Method, uri: &str, actor: &str, body: Value) -> String {
        self.ok(method, uri, actor, Some(body)).await["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

/// ANNA owns "shared" (with BEN as member and a household list holding items of both),
/// owns "solo" alone, has a personal list and a profile; BEN has a profile.
struct Scenario {
    h: Harness,
    shared: String,
    solo: String,
    shared_list: String,
    personal_list: String,
}

async fn scenario() -> Scenario {
    let h = harness();
    for actor in [ANNA, BEN] {
        h.ok(
            Method::PUT,
            "/api/me",
            actor,
            Some(json!({ "displayName": actor })),
        )
        .await;
    }
    let shared = h
        .id_of(
            Method::POST,
            "/api/households",
            ANNA,
            json!({ "name": "Shared" }),
        )
        .await;
    let solo = h
        .id_of(
            Method::POST,
            "/api/households",
            ANNA,
            json!({ "name": "Solo" }),
        )
        .await;
    let invite = h
        .ok(
            Method::POST,
            &format!("/api/households/{shared}/invites"),
            ANNA,
            Some(json!({ "maxUses": 1 })),
        )
        .await;
    h.ok(
        Method::POST,
        "/api/invites/accept",
        BEN,
        Some(json!({ "token": invite["token"] })),
    )
    .await;
    // A second, still open invitation created by ANNA.
    h.ok(
        Method::POST,
        &format!("/api/households/{shared}/invites"),
        ANNA,
        Some(json!({})),
    )
    .await;
    let shared_list = h
        .id_of(
            Method::POST,
            &format!("/api/households/{shared}/lists"),
            ANNA,
            json!({ "name": "Gemeinsam" }),
        )
        .await;
    for (version, (actor, name)) in [(ANNA, "Milch"), (BEN, "Brot")].into_iter().enumerate() {
        h.ok(
            Method::POST,
            &format!("/api/lists/{shared_list}/items"),
            actor,
            Some(json!({ "expectedListVersion": version + 1, "name": name })),
        )
        .await;
    }
    let personal_list = h
        .id_of(
            Method::POST,
            "/api/lists",
            ANNA,
            json!({ "name": "Privat" }),
        )
        .await;
    h.ok(
        Method::POST,
        &format!("/api/lists/{personal_list}/items"),
        ANNA,
        Some(json!({ "expectedListVersion": 1, "name": "Kaffee" })),
    )
    .await;
    Scenario {
        h,
        shared,
        solo,
        shared_list,
        personal_list,
    }
}

#[tokio::test]
async fn export_contains_profile_memberships_lists_and_items() {
    let s = scenario().await;
    let response =
        s.h.call_raw(Method::GET, "/api/me/export", ANNA, None)
            .await;
    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers();
    assert_eq!(
        headers.get(axum::http::header::CACHE_CONTROL).unwrap(),
        "no-store"
    );
    assert_eq!(
        headers
            .get(axum::http::header::CONTENT_DISPOSITION)
            .unwrap(),
        "attachment; filename=\"einkaufio-export.json\""
    );
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let export: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(export["formatVersion"], 1);
    assert_eq!(export["userId"], ANNA);
    assert_eq!(export["profile"]["displayName"], ANNA);
    assert!(export["exportedAt"].as_str().unwrap().ends_with('Z'));

    let households: Vec<&str> = export["households"]
        .as_array()
        .unwrap()
        .iter()
        .map(|household| household["id"].as_str().unwrap())
        .collect();
    assert_eq!(households.len(), 2);
    assert!(households.contains(&s.shared.as_str()) && households.contains(&s.solo.as_str()));
    assert!(export["households"][0]["role"] == "OWNER");

    let lists = export["lists"].as_array().unwrap();
    assert_eq!(lists.len(), 2);
    let shared = lists
        .iter()
        .find(|list| list["id"] == s.shared_list.as_str())
        .unwrap();
    assert_eq!(shared["type"], "HOUSEHOLD");
    assert_eq!(shared["householdId"], s.shared.as_str());
    assert_eq!(shared["items"][0]["name"], "Milch");
    assert_eq!(shared["items"][0]["createdBy"], ANNA);
    assert_eq!(shared["items"][1]["createdBy"], BEN);
    let personal = lists
        .iter()
        .find(|list| list["id"] == s.personal_list.as_str())
        .unwrap();
    assert_eq!(personal["type"], "PERSONAL");
    assert_eq!(personal["items"][0]["name"], "Kaffee");
    // Invitation secrets and hashes are never part of the export.
    assert!(!export.to_string().contains("token"));
}

#[tokio::test]
async fn export_only_contains_the_callers_data() {
    let s = scenario().await;
    let export = s.h.ok(Method::GET, "/api/me/export", BEN, None).await;
    assert_eq!(export["userId"], BEN);
    assert_eq!(export["households"].as_array().unwrap().len(), 1);
    assert_eq!(export["households"][0]["role"], "MEMBER");
    let lists = export["lists"].as_array().unwrap();
    assert_eq!(lists.len(), 1, "ANNA's personal list is not BEN's data");
    assert_eq!(lists[0]["id"], s.shared_list.as_str());

    let empty = s.h.ok(Method::GET, "/api/me/export", STRANGER, None).await;
    assert_eq!(empty["profile"], Value::Null);
    assert!(empty["households"].as_array().unwrap().is_empty());
    assert!(empty["lists"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn owner_with_members_must_transfer_before_deleting_the_account() {
    let s = scenario().await;
    let (status, body) = s.h.call(Method::DELETE, "/api/me", ANNA, None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "owner_must_transfer_ownership");

    // Nothing was deleted: the check runs before the first deletion.
    assert_eq!(
        s.h.call(Method::GET, "/api/me", ANNA, None).await.0,
        StatusCode::OK
    );
    let export = s.h.ok(Method::GET, "/api/me/export", ANNA, None).await;
    assert_eq!(export["households"].as_array().unwrap().len(), 2);
    assert_eq!(export["lists"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn account_deletion_removes_the_callers_data_and_keeps_shared_content() {
    let s = scenario().await;
    let (_, details) =
        s.h.call(
            Method::GET,
            &format!("/api/households/{}", s.shared),
            ANNA,
            None,
        )
        .await;
    s.h.ok(
        Method::POST,
        &format!("/api/households/{}/transfer-ownership", s.shared),
        ANNA,
        Some(json!({ "newOwnerId": BEN, "expectedVersion": details["version"] })),
    )
    .await;

    let (status, _) = s.h.call(Method::DELETE, "/api/me", ANNA, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Profile, personal list, sole-member household and memberships are gone.
    assert_eq!(
        s.h.call(Method::GET, "/api/me", ANNA, None).await.0,
        StatusCode::NOT_FOUND
    );
    let export = s.h.ok(Method::GET, "/api/me/export", ANNA, None).await;
    assert!(export["households"].as_array().unwrap().is_empty());
    assert!(export["lists"].as_array().unwrap().is_empty());
    assert_eq!(
        s.h.call(
            Method::GET,
            &format!("/api/lists/{}/items", s.personal_list),
            ANNA,
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (_, members) =
        s.h.call(
            Method::GET,
            &format!("/api/households/{}/members", s.shared),
            BEN,
            None,
        )
        .await;
    assert_eq!(members["items"].as_array().unwrap().len(), 1);
    assert_eq!(members["items"][0]["userId"], BEN);

    // Invitations ANNA created are removed; the shared list and its items stay with BEN.
    let (_, invitations) =
        s.h.call(
            Method::GET,
            &format!("/api/households/{}/invites", s.shared),
            BEN,
            None,
        )
        .await;
    assert!(invitations["items"].as_array().unwrap().is_empty());
    let (status, items) =
        s.h.call(
            Method::GET,
            &format!("/api/lists/{}/items", s.shared_list),
            BEN,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(items["items"].as_array().unwrap().len(), 2);
    assert_eq!(
        s.h.call(Method::GET, "/api/me", BEN, None).await.0,
        StatusCode::OK
    );

    // Repeating the deletion is safe.
    let (status, _) = s.h.call(Method::DELETE, "/api/me", ANNA, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn account_deletion_without_any_data_succeeds() {
    let h = harness();
    let (status, _) = h.call(Method::DELETE, "/api/me", STRANGER, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

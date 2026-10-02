use super::*;
use axum::http::StatusCode;
use axum::http::header as hdr;
use serde_json::{Value, json};

async fn serve_control(store: Store) -> (String, reqwest::Client) {
    let (base_url, _handle) = serve(control::router(&Server::new(
        store,
        "secret".into(),
        dummy_task_tx(),
    )))
    .await;

    (base_url, reqwest::Client::new())
}

#[tokio::test]
async fn control_gate_covers_unmatched_paths() {
    let store = Store::open_in_memory().unwrap();
    let (ctl, client) = serve_control(store).await;

    // an unmatched /api path must not answer 404 to an unauthenticated caller:
    // the 401/404 difference alone maps the control surface.
    let resp = client.get(format!("{ctl}/api/nope")).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    // the gate answers the status and its challenge; the error shape is the
    // response layer's, which has to reach the middleware's own responses.
    assert_eq!(resp.headers()[hdr::WWW_AUTHENTICATE], "Bearer");
    let v: Value = resp.json().await.unwrap();
    assert_eq!(v["error"]["code"], 401);
    assert_eq!(v["error"]["message"], "unauthorized");

    let resp = client
        .get(format!("{ctl}/api/nope"))
        .header(axum::http::header::AUTHORIZATION, "Bearer secret")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let v: Value = resp.json().await.unwrap();
    assert_eq!(v["error"]["code"], 404);
    assert_eq!(v["error"]["message"], "unknown path: /api/nope");
}

#[tokio::test]
async fn errors_off_the_handler_path_wear_the_envelope() {
    // a method axum itself refuses, and a body axum itself refuses: neither
    // reaches a handler, so only the response layer can give them the
    // control plane's error shape.
    let store = Store::open_in_memory().unwrap();
    let (ctl, client) = serve_control(store).await;
    let auth = |req: reqwest::RequestBuilder| req.header(hdr::AUTHORIZATION, "Bearer secret");

    let resp = auth(client.put(format!("{ctl}/api/health")))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(resp.headers()[hdr::ALLOW], "GET,HEAD");
    let v: Value = resp.json().await.unwrap();
    assert_eq!(v["error"]["code"], 405);
    assert_eq!(v["error"]["message"], "method not allowed");

    let resp = auth(client.post(format!("{ctl}/api/providers")))
        .header(hdr::CONTENT_TYPE, "text/plain")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let v: Value = resp.json().await.unwrap();
    assert_eq!(v["error"]["code"], 415);
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("application/json"),
        "{}",
        v["error"]["message"]
    );
}

#[tokio::test]
async fn pins_endpoint_round_trip() {
    // pins are per-app candidate sets (ADR-0009): the relation has its own URL
    // — add, remove, membership — and each end's pins read back as the other
    // end's representation.
    let store = store_with_provider("http://127.0.0.1:1").await;
    add_mock_provider(&store, "backup", "http://127.0.0.1:2").await;
    let (gw, client) = serve_control(store).await;
    let auth = |req: reqwest::RequestBuilder| req.header(hdr::AUTHORIZATION, "Bearer secret");
    let put = |app: &'static str, provider: &'static str| {
        let req = client.put(format!("{gw}/api/pins/{app}/{provider}"));
        async move { auth(req).send().await.unwrap() }
    };
    let app_pins = |app: &'static str| {
        let req = client.get(format!("{gw}/api/pins/{app}"));
        async move {
            auth(req)
                .send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap()
        }
    };

    // PUT member adds one candidate — idempotent, repeating is a no-op
    assert_eq!(put("claude", "mock").await.status(), StatusCode::NO_CONTENT);
    assert_eq!(put("claude", "mock").await.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        put("claude", "backup").await.status(),
        StatusCode::NO_CONTENT
    );

    // an app's pins are the providers themselves, in the order they were pinned
    // (not the id order: 'backup' sorts before 'mock')
    let v = app_pins("claude").await;
    assert_eq!(v["data"][0]["id"], "mock");
    assert_eq!(v["data"][1]["id"], "backup");
    // the gateway's observation rides along, as it does on /api/providers
    assert!(v["data"][0]["metrics"].is_object());

    // the relation answers for itself: 204 there, 404 gone — and a pin of
    // another app is not this app's
    assert_eq!(
        auth(client.get(format!("{gw}/api/pins/claude/mock")))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    let resp = auth(client.get(format!("{gw}/api/pins/claude/ghost")))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        resp.json::<Value>().await.unwrap()["error"]["code"],
        json!(404)
    );
    assert_eq!(
        auth(client.get(format!("{gw}/api/pins/codex/mock")))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );

    // the reverse direction hangs off the provider: the apps that pin it, in
    // the order they pinned it
    let v: Value = auth(client.get(format!("{gw}/api/providers/mock/pins")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["data"], json!(["claude"]));
    assert_eq!(put("codex", "mock").await.status(), StatusCode::NO_CONTENT);
    let v: Value = auth(client.get(format!("{gw}/api/providers/mock/pins")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["data"], json!(["claude", "codex"]));
    assert_eq!(
        auth(client.get(format!("{gw}/api/providers/ghost/pins")))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );

    // the cross-app view lists the members
    let v: Value = auth(client.get(format!("{gw}/api/pins")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["data"][0]["app"], "claude");
    assert_eq!(v["data"][0]["provider_id"], "mock");

    // a member must name a live provider — otherwise the write is a 404
    let resp = put("claude", "ghost").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(app_pins("claude").await["data"][0]["id"], "mock");

    // DELETE member removes it — and is idempotent: an absent member deletes
    // cleanly, and the relation then reports itself gone
    for _ in 0..2 {
        assert_eq!(
            auth(client.delete(format!("{gw}/api/pins/claude/mock")))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
    }
    assert_eq!(
        auth(client.get(format!("{gw}/api/pins/claude/mock")))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let v = app_pins("claude").await;
    assert_eq!(v["data"][0]["id"], "backup");
    assert_eq!(v["data"].as_array().unwrap().len(), 1);

    // DELETE /pins/{app} clears one app; DELETE /pins clears the whole
    // table — the same clear primitive behind both, 204 on any target
    assert_eq!(put("claude", "mock").await.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        auth(client.delete(format!("{gw}/api/pins/claude")))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert!(
        app_pins("claude").await["data"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // repin, then reset the whole table
    assert_eq!(put("claude", "mock").await.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        auth(client.delete(format!("{gw}/api/pins")))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    let v: Value = auth(client.get(format!("{gw}/api/pins")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(v["data"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn app_key_issue_returns_the_key_once_and_maps_back() {
    // POST /api/app-keys takes only the app name — the gateway issues a fresh
    // key (API-key convention): the response carries the public record plus
    // the key — returned once — which is exactly how the data plane maps it back.
    let store = store_with_provider("http://127.0.0.1:1").await;
    let (gw, client) = serve_control(store.clone()).await;

    let resp = client
        .post(format!("{gw}/api/app-keys"))
        .header(hdr::AUTHORIZATION, "Bearer secret")
        .json(&json!({"app": "claude"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let v: Value = resp.json().await.unwrap();
    let id = v["id"].as_str().expect("public id");
    let secret = v["key"].as_str().expect("one-time secret");
    assert!(id.starts_with("ak_"), "{id}");
    assert_eq!(v["app"], "claude");
    assert_eq!(secret.len(), 32);
    assert!(secret.chars().all(|c| c.is_ascii_hexdigit()), "{secret}");
    // last4 is the key's tail; created_at is ISO-8601
    assert_eq!(v["last4"], &secret[28..]);
    assert_eq!(v["created_at"].as_str().unwrap().chars().last(), Some('Z'));

    // the issued key is the credential the data plane maps back to the app
    assert_eq!(
        store.app_by_key(secret).await.unwrap().as_deref(),
        Some("claude")
    );
    assert_eq!(store.get_app_key(id).await.unwrap().unwrap().id, id);
}

#[tokio::test]
async fn data_plane_does_not_serve_control() {
    // /api/* must not be reachable on the agent router — the routers are split
    let store = store_with_provider("http://127.0.0.1:1").await;
    let (gw, _gh) = serve(data::router(&Server::new(
        store,
        "secret".into(),
        dummy_task_tx(),
    )))
    .await;
    let resp = reqwest::Client::new()
        .get(format!("{gw}/api/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn template_free_add_persists_the_served_protocols_and_meta() {
    // a template-free add (no provider template) carries its served protocols
    // (families claimed at their canonical endpoints) and metadata through to
    // the store, and the response is the stored record — the CLI needs the
    // final id and per-field values for the post-add probe.
    let store = Store::open_in_memory().unwrap();
    let (gw, client) = serve_control(store.clone()).await;
    let auth = |req: reqwest::RequestBuilder| req.header(hdr::AUTHORIZATION, "Bearer secret");

    let resp = auth(client.post(format!("{gw}/api/providers")))
        .json(&json!({
            "id": "custom",
            "key": "sk-custom",
            "base_url": "https://api.custom.com",
            "protocols": {"openai_chat": {}},
            "models_url": "https://api.custom.com/v1/models",
            "balance_url": null,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let provider: Value = resp.json().await.unwrap();
    assert_eq!(provider["id"], "custom");
    assert_eq!(provider["protocols"]["openai_chat"], json!({}));
    assert_eq!(provider["models_url"], "https://api.custom.com/v1/models");
    assert_eq!(provider["balance_url"], Value::Null);

    let stored = store.get_provider("custom").await.unwrap().unwrap();
    assert_eq!(stored.protocols.len(), 1);
    assert_eq!(
        stored.protocols.get(&crate::protocol::Protocol::OpenaiChat),
        Some(&crate::provider::Endpoint::Canonical)
    );
    assert_eq!(
        stored
            .url_for(crate::protocol::Protocol::OpenaiChat)
            .unwrap(),
        "https://api.custom.com/v1/chat/completions"
    );
    assert_eq!(stored.balance_url, None);
    assert_eq!(
        stored.models_url.as_deref(),
        Some("https://api.custom.com/v1/models")
    );
}

#[tokio::test]
async fn template_add_is_a_pure_snapshot_of_the_template() {
    // a built-in template add snapshots the template's current stored value
    // (ADR-0008 §3.1): a routing id and a key are the whole input — every field
    // of the stored record is the template's, wholesale
    let store = Store::open_in_memory().unwrap();
    let (gw, client) = serve_control(store.clone()).await;
    let auth = |req: reqwest::RequestBuilder| req.header(hdr::AUTHORIZATION, "Bearer secret");

    let resp = auth(client.post(format!("{gw}/api/provider-templates/deepseek/providers")))
        .json(&json!({
            "id": "deepseek-a",
            "key": "sk-test",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let provider: Value = resp.json().await.unwrap();
    assert_eq!(provider["id"], "deepseek-a");
    assert_eq!(provider["template_id"], "deepseek");
    assert_eq!(provider["base_url"], "https://api.deepseek.com");
    assert_eq!(provider["protocols"]["anthropic"], "/anthropic");
    assert_eq!(provider["protocols"]["openai_chat"], json!({}));
    assert_eq!(provider["models"], json!([]));

    // the stored record is exactly snapshot(template, id, key)
    let template = crate::provider::builtin_template("deepseek").unwrap();
    let want = crate::provider::snapshot(&template, "deepseek-a", "sk-test");
    assert_eq!(
        store.get_provider("deepseek-a").await.unwrap().unwrap(),
        want
    );
}

#[tokio::test]
async fn template_add_defaults_the_routing_id() {
    // no routing id → the smallest free `<template>-<n>` id
    let store = Store::open_in_memory().unwrap();
    let (gw, client) = serve_control(store.clone()).await;
    let auth = |req: reqwest::RequestBuilder| req.header(hdr::AUTHORIZATION, "Bearer secret");

    let resp = auth(client.post(format!("{gw}/api/provider-templates/deepseek/providers")))
        .json(&json!({ "key": "sk-test" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    assert_eq!(resp.json::<Value>().await.unwrap()["id"], "deepseek-1");
}

#[tokio::test]
async fn template_free_add_defaults_the_routing_id() {
    // a template-free add without a routing id → the smallest free `p<n>` id
    let store = Store::open_in_memory().unwrap();
    let (gw, client) = serve_control(store.clone()).await;
    let auth = |req: reqwest::RequestBuilder| req.header(hdr::AUTHORIZATION, "Bearer secret");

    let resp = auth(client.post(format!("{gw}/api/providers")))
        .json(&json!({
            "key": "sk-test", "base_url": "https://api.x.com/v1",
            "protocols": {"openai_chat": {}},
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    assert_eq!(resp.json::<Value>().await.unwrap()["id"], "p1");

    // the next template-free add without an id advances the counter
    let resp = auth(client.post(format!("{gw}/api/providers")))
        .json(&json!({
            "key": "sk-test-2", "base_url": "https://api.x.com/v1",
            "protocols": {"openai_chat": {}},
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    assert_eq!(resp.json::<Value>().await.unwrap()["id"], "p2");
}

#[tokio::test]
async fn instantiate_rejects_connection_fields() {
    // an instantiation carries only a routing id and a key — a connection field
    // alongside them is unknown to the request type and rejected by it (ADR-0008
    // §2/§3): the shape is unrepresentable
    let store = Store::open_in_memory().unwrap();
    let (gw, client) = serve_control(store.clone()).await;
    let auth = |req: reqwest::RequestBuilder| req.header(hdr::AUTHORIZATION, "Bearer secret");

    for (field, value) in [
        ("base_url", json!("https://proxy.example.com")),
        ("name", json!("Custom")),
        ("protocols", json!({"openai_chat": {}})),
        ("models_url", json!("https://x/models")),
    ] {
        let mut body = json!({"key": "sk-test"});
        body.as_object_mut()
            .unwrap()
            .insert(field.to_string(), value);
        let resp = auth(client.post(format!("{gw}/api/provider-templates/deepseek/providers")))
            .json(&body)
            .send()
            .await
            .unwrap();
        // the body parses as JSON and is rejected by the type: axum's 422,
        // carried through the control plane's envelope
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY, "{field}");
        let v: Value = resp.json().await.unwrap();
        assert!(
            v["error"]["message"]
                .as_str()
                .unwrap()
                .contains("unknown field"),
            "{field}: {}",
            v["error"]["message"]
        );
    }
    // nothing was written by any of the rejected instantiations
    assert_eq!(store.list_providers().await.unwrap().len(), 0);
}

#[tokio::test]
async fn unknown_template_is_a_not_found() {
    // the template is the URL's parent resource — instantiating under a
    // template that does not exist is a structural 404, not a 400
    let store = Store::open_in_memory().unwrap();
    let (gw, client) = serve_control(store.clone()).await;
    let auth = |req: reqwest::RequestBuilder| req.header(hdr::AUTHORIZATION, "Bearer secret");

    let resp = auth(client.post(format!("{gw}/api/provider-templates/ghost/providers")))
        .json(&json!({ "key": "sk-test" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let v: Value = resp.json().await.unwrap();
    assert!(v["error"]["message"].as_str().unwrap().contains("ghost"));
    assert_eq!(store.list_providers().await.unwrap().len(), 0);
}

#[tokio::test]
async fn provider_template_catalog_is_readable() {
    // GET /provider-templates is the built-in catalog — every shipped
    // template, id-keyed, in file order (ADR-0008 §3.1)
    let store = Store::open_in_memory().unwrap();
    let (gw, client) = serve_control(store).await;

    let resp = client
        .get(format!("{gw}/api/provider-templates"))
        .header(hdr::AUTHORIZATION, "Bearer secret")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v: Value = resp.json().await.unwrap();
    let ids: Vec<&str> = crate::provider::builtin_template_ids().collect();
    let data = v["data"].as_array().unwrap();
    let ids_in_order: Vec<&str> = data.iter().map(|t| t["id"].as_str().unwrap()).collect();
    assert_eq!(ids_in_order, ids);

    // the deepseek entry is the template itself — the snapshot source
    let want = crate::provider::builtin_template("deepseek").unwrap();
    let entry = data.iter().find(|t| t["id"] == "deepseek").unwrap();
    assert_eq!(entry["base_url"], json!(want.base_url));
    assert_eq!(
        entry["protocols"],
        serde_json::to_value(&want.protocols).unwrap()
    );
}

#[tokio::test]
async fn create_with_a_taken_id_conflicts() {
    // a duplicate provider id is a state conflict (409), not a malformed
    // request — on both create paths
    let store = Store::open_in_memory().unwrap();
    let (gw, client) = serve_control(store.clone()).await;
    let auth = |req: reqwest::RequestBuilder| req.header(hdr::AUTHORIZATION, "Bearer secret");

    // template-free add: first wins, the retry with the same id conflicts
    let body = json!({
        "id": "custom", "key": "k", "base_url": "https://api.x.com/v1",
        "protocols": {"openai_chat": {}},
    });
    let resp = auth(client.post(format!("{gw}/api/providers")))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let resp = auth(client.post(format!("{gw}/api/providers")))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let v: Value = resp.json().await.unwrap();
    assert!(v["error"]["message"].as_str().unwrap().contains("custom"));

    // instantiate: same id twice under the template conflicts too
    let body = json!({ "id": "deepseek-a", "key": "sk-test" });
    let post = || async {
        auth(client.post(format!("{gw}/api/provider-templates/deepseek/providers")))
            .json(&body)
            .send()
            .await
            .unwrap()
    };
    assert_eq!(post().await.status(), StatusCode::CREATED);
    assert_eq!(post().await.status(), StatusCode::CONFLICT);
    assert_eq!(store.list_providers().await.unwrap().len(), 2);
}

#[tokio::test]
async fn edit_rejects_unknown_fields() {
    // a typo'd patch field must not be silently accepted as a no-op — the
    // request type rejects unknown fields (422) before anything is written.
    let store = store_with_provider("http://127.0.0.1:1").await;
    let (gw, client) = serve_control(store).await;

    let resp = client
        .patch(format!("{gw}/api/providers/mock"))
        .header(hdr::AUTHORIZATION, "Bearer secret")
        .json(&json!({"baseurl": "https://typo.example.com"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let v: Value = resp.json().await.unwrap();
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unknown field")
    );
}

#[tokio::test]
async fn edit_restricts_a_template_backed_provider_to_its_key() {
    // a template-backed record's fields are the template's — only its API key
    // is editable; a template-free record stays fully editable (ADR-0008 §3.1)
    let store = Store::open_in_memory().unwrap();
    let (gw, client) = serve_control(store.clone()).await;
    let auth = |req: reqwest::RequestBuilder| req.header(hdr::AUTHORIZATION, "Bearer secret");

    auth(client.post(format!("{gw}/api/provider-templates/deepseek/providers")))
        .json(&json!({ "key": "sk-test" }))
        .send()
        .await
        .unwrap();

    // field patches are rejected with guidance
    for (field, value) in [
        ("base_url", json!("https://proxy.example.com")),
        ("name", json!("Custom")),
        ("protocols", json!({"openai_chat": {}})),
        ("models_url", json!("https://x/models")),
    ] {
        let mut body = json!({});
        body.as_object_mut()
            .unwrap()
            .insert(field.to_string(), value);
        let resp = auth(client.patch(format!("{gw}/api/providers/deepseek-1")))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{field}");
        let v: Value = resp.json().await.unwrap();
        assert!(
            v["error"]["message"]
                .as_str()
                .unwrap()
                .contains("diverged from its template"),
            "{field}: {}",
            v["error"]["message"]
        );
    }

    // the API key itself rotates, and the link survives
    let resp = auth(client.patch(format!("{gw}/api/providers/deepseek-1")))
        .json(&json!({"key": "rotated"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let stored = store.get_provider("deepseek-1").await.unwrap().unwrap();
    assert_eq!(stored.key, "rotated");
    assert_eq!(stored.template_id.as_deref(), Some("deepseek"));

    // a template-free provider is fully editable
    auth(client.post(format!("{gw}/api/providers")))
        .json(&json!({
            "id": "custom", "key": "k", "base_url": "https://api.custom.com/v1",
            "protocols": {"openai_chat": {}},
        }))
        .send()
        .await
        .unwrap();
    let resp = auth(client.patch(format!("{gw}/api/providers/custom")))
        .json(&json!({
            "name": "Custom Inc.",
            "base_url": "https://api.custom.com/v2",
            "models_url": "https://api.custom.com/v2/models",
            "balance_url": "https://api.custom.com/v2/balance",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let stored = store.get_provider("custom").await.unwrap().unwrap();
    assert_eq!(stored.name.as_deref(), Some("Custom Inc."));
    assert_eq!(stored.base_url, "https://api.custom.com/v2");
    assert_eq!(
        stored.models_url.as_deref(),
        Some("https://api.custom.com/v2/models")
    );
    assert_eq!(
        stored.balance_url.as_deref(),
        Some("https://api.custom.com/v2/balance")
    );
}

#[tokio::test]
async fn settings_document_patch_and_validation() {
    // settings are one document (config-as-document): GET reads it whole,
    // PATCH applies the named fields with schema-typed values and echoes the
    // full updated document; unknown keys and mistyped values are 400s.
    let store = Store::open_in_memory().unwrap();
    let (gw, client) = serve_control(store).await;
    let auth = |req: reqwest::RequestBuilder| req.header(hdr::AUTHORIZATION, "Bearer secret");

    // defaults on a fresh store
    let v: Value = auth(client.get(format!("{gw}/api/settings")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["routing.mode"], "eco");
    assert_eq!(v["routing.balance_threshold"], "1");

    // one enum field — the response echoes the full updated document
    let resp = patch_settings_req(&gw, &client, json!({ "routing.mode": "speed" })).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let v: Value = resp.json().await.unwrap();
    assert_eq!(v["routing.mode"], "speed");

    // an amount takes its numeric string form or a plain number
    let resp =
        patch_settings_req(&gw, &client, json!({ "routing.balance_threshold": "0.5" })).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = patch_settings_req(&gw, &client, json!({ "routing.balance_threshold": 2 })).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let v: Value = auth(client.get(format!("{gw}/api/settings")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["routing.balance_threshold"], "2");

    // several fields in one PATCH are a single all-or-nothing update
    let resp = patch_settings_req(&gw, &client, json!({ "routing.mode": "balanced" })).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let v: Value = resp.json().await.unwrap();
    assert_eq!(v["routing.mode"], "balanced");
    assert_eq!(v["routing.balance_threshold"], "2");

    // an unknown key is a structural 400, not a stored junk row
    let resp = patch_settings_req(&gw, &client, json!({ "nope": "x" })).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let v: Value = resp.json().await.unwrap();
    assert!(v["error"]["message"].as_str().unwrap().contains("nope"));

    // a mistyped value is a 400: bad enum, enum as number
    for body in [
        json!({ "routing.mode": "slow" }),
        json!({ "routing.mode": 3 }),
        json!({ "routing.balance_threshold": "not-an-amount" }),
    ] {
        let label = body.to_string();
        let resp = patch_settings_req(&gw, &client, body).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{label}");
    }

    // nothing above wrote a junk row — the document still holds the last patch
    let v: Value = auth(client.get(format!("{gw}/api/settings")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["routing.mode"], "balanced");
}

/// PATCH /settings for the round-trip tests.
async fn patch_settings_req(gw: &str, client: &reqwest::Client, body: Value) -> reqwest::Response {
    client
        .patch(format!("{gw}/api/settings"))
        .header(hdr::AUTHORIZATION, "Bearer secret")
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn issue_app_key_via(gw: &str, client: &reqwest::Client, app: &str) -> (String, String) {
    let resp = client
        .post(format!("{gw}/api/app-keys"))
        .header(hdr::AUTHORIZATION, "Bearer secret")
        .json(&json!({ "app": app }))
        .send()
        .await
        .unwrap();
    let v: Value = resp.json().await.unwrap();
    (
        v["id"].as_str().unwrap().to_string(),
        v["key"].as_str().unwrap().to_string(),
    )
}

#[tokio::test]
async fn app_key_delete_is_by_id_and_selective() {
    // DELETE /api/app-keys/{id} revokes exactly the key with that public id —
    // the app's other keys and other apps' keys stay live; an unknown id is a
    // 404 (the key no longer exists), never accepted by value.
    let store = store_with_provider("http://127.0.0.1:1").await;
    let (gw, client) = serve_control(store.clone()).await;
    let auth = |req: reqwest::RequestBuilder| req.header(hdr::AUTHORIZATION, "Bearer secret");

    let (id1, s1) = issue_app_key_via(&gw, &client, "claude").await;
    let (id2, s2) = issue_app_key_via(&gw, &client, "claude").await;
    let (_, ks) = issue_app_key_via(&gw, &client, "kimi").await;

    // management list shows public records only — never the key
    let v: Value = auth(client.get(format!("{gw}/api/app-keys")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    for row in v["data"].as_array().unwrap() {
        assert!(row.get("secret").is_none(), "secret must never be listed");
        assert!(row["id"].as_str().unwrap().starts_with("ak_"));
    }

    // ?app= narrows to one app
    let v: Value = auth(client.get(format!("{gw}/api/app-keys?app=claude")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["data"].as_array().unwrap().len(), 2);

    // delete one by id — the app's other key and the other app's key stay live
    let resp = auth(client.delete(format!("{gw}/api/app-keys/{id1}")))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert_eq!(store.app_by_key(&s1).await.unwrap(), None);
    assert_eq!(
        store.app_by_key(&s2).await.unwrap().as_deref(),
        Some("claude")
    );
    assert_eq!(
        store.app_by_key(&ks).await.unwrap().as_deref(),
        Some("kimi")
    );

    // an unknown (already deleted) id is a 404 — the key no longer exists
    let resp = auth(client.delete(format!("{gw}/api/app-keys/{id1}")))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(store.get_app_key(&id2).await.unwrap().unwrap().id, id2);
}

#[tokio::test]
async fn providers_carry_their_status() {
    let store = store_with_provider("http://127.0.0.1:1").await;
    let (gw, client) = serve_control(store).await;
    let auth = |req: reqwest::RequestBuilder| req.header(hdr::AUTHORIZATION, "Bearer secret");

    let v: Value = auth(client.get(format!("{gw}/api/providers")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let p = &v["data"][0];
    // The provider resource carries its configuration...
    assert_eq!(p["id"], "mock");
    // ...and the gateway's observation of it, as the facts they are.
    assert_eq!(p["metrics"]["consecutive_failures"], 0);
    assert_eq!(p["metrics"]["last_attempt_at"], 0);
    // Money keeps its parts instead of arriving as a rendered string, and no
    // no ejection verdict — the threshold and the cooldown are the caller's
    // from settings.
    assert_eq!(p["metrics"]["balance"]["amount"], "10");
    assert_eq!(p["metrics"]["balance"]["currency"], "USD");
    assert!(p["metrics"].get("health").is_none());
}

/// The reported status is the observation, never a verdict: a quiet failure
/// streak is reported exactly as observed, and the window it is read against
/// is a readable setting — routing is the only place the two meet.
#[tokio::test]
async fn quiet_failure_streak_is_reported_as_observed() {
    let store = store_with_provider("http://127.0.0.1:1").await;
    store.enqueue_attempt(crate::provider::Attempt {
        provider_id: "mock".into(),
        outcome: crate::provider::AttemptOutcome::Failed,
        latency: 0,
        at: crate::utils::now_unix_secs() - 3600,
    });
    let (gw, client) = serve_control(store).await;

    let v: Value = client
        .get(format!("{gw}/api/providers"))
        .header(hdr::AUTHORIZATION, "Bearer secret")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(v["data"][0]["metrics"]["consecutive_failures"], 1);
    assert!(v["data"][0]["metrics"]["last_attempt_at"].as_i64().unwrap() > 0);

    let s: Value = client
        .get(format!("{gw}/api/settings"))
        .header(hdr::AUTHORIZATION, "Bearer secret")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(s["routing.cooldown_secs"], 600);
}

#[tokio::test]
async fn there_is_no_status_endpoint() {
    // The gateway's status is not a resource: a provider's observation is part
    // of the provider, the app set is the CLI's pointing state, and the
    // gateway's own liveness is `/api/health`.
    let store = store_with_provider("http://127.0.0.1:1").await;
    let (gw, client) = serve_control(store).await;
    let auth = |req: reqwest::RequestBuilder| req.header(hdr::AUTHORIZATION, "Bearer secret");

    for path in [
        "/api/status",
        "/api/status/app/claude",
        "/api/status/provider/mock",
    ] {
        let resp = auth(client.get(format!("{gw}{path}")))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

/// One request of one token each way at $1/M — a cost that is neither zero
/// nor a rounding artifact, so the report's arithmetic is pinned.
fn priced_row(app: &str, provider_id: &str) -> crate::ledger::LedgerNew {
    crate::ledger::LedgerNew {
        app: app.into(),
        provider_id: provider_id.into(),
        response_id: None,
        conversation_id: None,
        requested_protocol: Protocol::OpenaiChat,
        requested_model: "m".into(),
        served_protocol: Protocol::OpenaiChat,
        served_model: "m".into(),
        usage: crate::protocol::Usage {
            input_tokens: 1,
            output_tokens: 1,
        },
        price: Some(crate::pricing::Price {
            input: "1".parse().unwrap(),
            output: "1".parse().unwrap(),
        }),
    }
}

#[tokio::test]
async fn usage_serves_a_window_covering_from_with_rows_in_the_api_shape() {
    let store = store_with_provider("http://127.0.0.1:1").await;
    store.enqueue_ledger(priced_row("claude", "mock"));
    let (gw, client) = serve_control(store).await;
    let auth = |req: reqwest::RequestBuilder| req.header(hdr::AUTHORIZATION, "Bearer secret");

    let now = crate::utils::now_unix_secs();
    let v: Value = auth(client.get(format!("{gw}/api/usage?from={now}&bucket_width=day")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // The window the caller asked from, up to the bucket that has not finished.
    let start = v["start"].as_i64().unwrap();
    let end = v["end"].as_i64().unwrap();
    assert!(start <= now && now < end, "{start} <= {now} < {end}");
    assert_eq!(v["bucket_width"], "day");

    // The row lands in the bucket its own timestamp falls in, and the empty
    // buckets of the window are still served.
    let filled: Vec<&Value> = v["buckets"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|b| !b["rows"].as_array().unwrap().is_empty())
        .collect();
    assert_eq!(filled.len(), 1);
    let bucket_start = filled[0]["start"].as_i64().unwrap();
    let bucket_end = filled[0]["end"].as_i64().unwrap();
    assert!(
        bucket_start <= now && now < bucket_end,
        "{bucket_start} <= {now} < {bucket_end}"
    );

    let row = &filled[0]["rows"][0];
    assert_eq!(row["app"], "claude");
    assert_eq!(row["provider_id"], "mock");
    assert_eq!(row["provider_name"], "mock");
    assert_eq!(row["served_model"], "m");
    assert_eq!(row["requests"], 1);
    assert_eq!(row["input_tokens"], 1);
    assert_eq!(row["output_tokens"], 1);
    // An amount + currency, not a pre-rounded string the caller cannot undo.
    assert_eq!(row["cost"]["amount"], "0.000002");
    assert_eq!(row["cost"]["currency"], "USD");
}

#[tokio::test]
async fn usage_scopes_to_one_app_or_provider() {
    let store = store_with_provider("http://127.0.0.1:1").await;
    store.enqueue_ledger(priced_row("claude", "mock"));
    store.enqueue_ledger(priced_row("codex", "mock"));
    let (gw, client) = serve_control(store).await;
    let auth = |req: reqwest::RequestBuilder| req.header(hdr::AUTHORIZATION, "Bearer secret");

    let now = crate::utils::now_unix_secs();
    let apps = |v: &Value| {
        v["buckets"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|b| b["rows"].as_array().unwrap().iter())
            .map(|r| r["app"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };

    let scoped: Value = auth(client.get(format!(
        "{gw}/api/usage/app/claude?from={now}&bucket_width=day"
    )))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(apps(&scoped), ["claude"]);

    let scoped: Value = auth(client.get(format!(
        "{gw}/api/usage/provider/mock?from={now}&bucket_width=day"
    )))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(apps(&scoped), ["claude", "codex"]);
}

#[tokio::test]
async fn usage_rejects_a_future_from_and_unknown_parameters() {
    let store = store_with_provider("http://127.0.0.1:1").await;
    let (gw, client) = serve_control(store).await;
    let bad = |path: String| {
        let client = client.clone();
        let gw = gw.clone();
        async move { control_get(&gw, &client, &path).await }
    };

    // Nothing to report before it happens, and a page whose shape the caller
    // cannot know is a bug, not a filter.
    let (status, v) = bad(format!(
        "/api/usage?from={}&bucket_width=day",
        crate::utils::now_unix_secs() + 3600
    ))
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(v["error"]["message"], "from is in the future");

    for path in [
        format!(
            "/api/usage?from={}&bucket=day",
            crate::utils::now_unix_secs()
        ),
        "/api/usage".to_string(),
    ] {
        let (status, _) = bad(path.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}");
    }
}

/// A control GET carrying the token, and its parsed body.
async fn control_get(gw: &str, client: &reqwest::Client, path: &str) -> (StatusCode, Value) {
    let resp = client
        .get(format!("{gw}{path}"))
        .header(hdr::AUTHORIZATION, "Bearer secret")
        .send()
        .await
        .unwrap();
    let status = resp.status();
    (status, resp.json().await.unwrap())
}

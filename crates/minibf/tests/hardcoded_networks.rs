use axum::http::Request;
use axum::body::Body;
use axum::body;
use tower::ServiceExt;
use serde_json::Value as JsonValue;

use dolos_core::config::{HardcodedNetwork, MinibfConfig};
use dolos_core::Domain;
use dolos_minibf::{build_router, build_router_for_test};
use dolos_testing::toy_domain::ToyDomain;

#[tokio::test]
async fn blocks_endpoint_respects_minibf_hardcoded_genesis_hash() {
    let domain = ToyDomain::new(None, None);

    let magic = domain.genesis().shelley.network_magic.unwrap_or_default() as u64;
    let custom_hash = "aa".repeat(32); // 64 hex chars

    let cfg = MinibfConfig {
        listen_address: "127.0.0.1:0".parse().unwrap(),
        permissive_cors: None,
        token_registry_url: None,
        url: None,
        base_path: None,
        hardcoded_network: Some(HardcodedNetwork { magic, genesis_hash: custom_hash.clone() }),
    };

    let app = build_router_for_test(cfg, domain);

    // Request by genesis hash (should be recognized via MinibfConfig)
    let req = Request::builder()
        .method("GET")
        .uri(format!("/blocks/{}", custom_hash))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.expect("router call");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let body_bytes = body::to_bytes(resp.into_body(), 16 * 1024 * 1024).await.unwrap();
    let json: JsonValue = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(json["hash"].as_str().unwrap(), custom_hash);
}

#[tokio::test]
async fn blocks_zero_returns_genesis_with_configured_hash() {
    let domain = ToyDomain::new(None, None);

    let magic = domain.genesis().shelley.network_magic.unwrap_or_default() as u64;
    let custom_hash = "bb".repeat(32); // 64 hex chars

    let cfg = MinibfConfig {
        listen_address: "127.0.0.1:0".parse().unwrap(),
        permissive_cors: None,
        token_registry_url: None,
        url: None,
        base_path: None,
        hardcoded_network: Some(HardcodedNetwork { magic, genesis_hash: custom_hash.clone() }),
    };

    let app = build_router_for_test(cfg, domain);

    let req = Request::builder()
        .method("GET")
        .uri("/blocks/0")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.expect("router call");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let body_bytes = body::to_bytes(resp.into_body(), 16 * 1024 * 1024).await.unwrap();
    let json: JsonValue = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(json["hash"].as_str().unwrap(), custom_hash);
}

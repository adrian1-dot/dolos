use axum::body::Body;
use axum::http::Request;
use tower::ServiceExt;
use serde_json::Value as JsonValue;

use dolos_core::config::MinibfConfig;
use dolos_core::{Domain, StateStore};
use dolos_testing::toy_domain::ToyDomain;
use dolos_cardano::{EraSummary, FixedNamespace};
use dolos_minibf::{build_router, build_router_for_test};

#[tokio::test]
async fn network_eras_derives_initial_parameters_from_state() {
    // Toy domain bootstraps genesis + era summary
    let domain = ToyDomain::new(None, None);

    // read the earliest EraSummary from state (authoritative source)
    let mut iter = domain
        .state()
        .iter_entities_typed::<EraSummary>(EraSummary::NS, None)
        .unwrap();

    let (_, era) = iter.next().expect("era summary present").unwrap();

    let cfg = MinibfConfig {
        listen_address: "127.0.0.1:0".parse().unwrap(),
        permissive_cors: None,
        token_registry_url: None,
        url: None,
        base_path: None,
        hardcoded_network: None,
    };

    let app = build_router_for_test(cfg, domain);

    let req = Request::builder()
        .method("GET")
        .uri("/network/eras")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.expect("router call");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let body_bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    let json: JsonValue = serde_json::from_slice(&body_bytes).unwrap();

    let arr = json.as_array().expect("expected array response");
    assert!(!arr.is_empty(), "expected at least one era entry");

    // first element contains the initial-era parameters (derived from state)
    let first = &arr[0];

    assert_eq!(
        first["parameters"]["epoch_length"].as_i64().unwrap(),
        era.epoch_length as i64
    );

    assert_eq!(
        first["parameters"]["slot_length"].as_i64().unwrap(),
        era.slot_length as i64
    );
}

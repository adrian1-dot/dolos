use axum::body::Body;
use axum::http::Request;
use tower::ServiceExt;
use serde_json::Value as JsonValue;

use dolos_core::config::MinibfConfig;
use dolos_core::{Domain, StateStore, StateWriter};
use dolos_testing::toy_domain::ToyDomain;
use dolos_cardano::{forks, PParamsSet, EpochValue, FixedNamespace};
use dolos_minibf::{build_router, build_router_for_test};

#[tokio::test]
async fn epochs_latest_parameters_reflect_pparams_values() {
    // Create a ToyDomain (bootstrapped genesis)
    let domain = ToyDomain::new(None, None);

    // Force-migrate pparams up to Alonzo so executionPrices/cost-models are present
    let genesis = domain.genesis();
    let migrated: PParamsSet = forks::force_pparams_version(&PParamsSet::default(), &genesis, 0, 5)
        .expect("force migrate failed");

    // Replace the current epoch state's pparams with the migrated value
    let writer = domain.state().start_writer().unwrap();

    let mut epoch = domain
        .state()
        .read_entity_typed::<dolos_cardano::EpochState>(dolos_cardano::EpochState::NS, &dolos_core::EntityKey::from(dolos_cardano::CURRENT_EPOCH_KEY))
        .unwrap()
        .unwrap();

    epoch.pparams = EpochValue::with_live(epoch.number, migrated.clone());

    writer.write_entity_typed(&dolos_core::EntityKey::from(dolos_cardano::CURRENT_EPOCH_KEY), &epoch).unwrap();
    writer.commit().unwrap();

    // verify the epoch was updated in state
    let epoch_after = domain
        .state()
        .read_entity_typed::<dolos_cardano::EpochState>(dolos_cardano::EpochState::NS, &dolos_core::EntityKey::from(dolos_cardano::CURRENT_EPOCH_KEY))
        .unwrap()
        .unwrap();

    assert!(epoch_after.pparams.live().as_ref().and_then(|p| p.execution_costs()).is_some(), "pparams migration not present in state");

    // sanity-check handler preconditions (replicate parts of the handler)
    // ensure load_epoch succeeds against the ToyDomain state
    let epoch_loaded = dolos_cardano::load_epoch::<ToyDomain>(domain.state()).expect("load_epoch should succeed");
    assert!(epoch_loaded.pparams.live().as_ref().and_then(|p| p.execution_costs()).is_some());

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
        .uri("/epochs/latest/parameters")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.expect("router call");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let body_bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    let json: JsonValue = serde_json::from_slice(&body_bytes).unwrap();

    // executionPrices -> price_mem/price_step should be present (Alonzo)
    assert!(json["price_mem"] != JsonValue::Null, "price_mem is null");
    assert!(json["price_step"] != JsonValue::Null, "price_step is null");

    // cost_models should be present (PlutusV1 from Alonzo)
    assert!(json["cost_models"] != JsonValue::Null, "cost_models is null");
}

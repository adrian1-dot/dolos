use axum::{
    extract::{Path, State},
    Json,
};
use blockfrost_openapi::models::script_datum::ScriptDatum;
use dolos_cardano::indexes::AsyncCardanoQueryExt;
use dolos_core::{Domain, IndexStore, StateStore};
use pallas::crypto::hash::Hash;
use reqwest::StatusCode;

use crate::{
    error::Error,
    mapping::{IntoModel, PlutusDataWrapper},
    Facade,
};

pub async fn by_datum_hash<D>(
    Path(datum_hash): Path<String>,
    State(domain): State<Facade<D>>,
) -> Result<Json<ScriptDatum>, Error>
where
    D: Domain + Clone + Send + Sync + 'static,
{
    if datum_hash.len() != 64 {
        // Oficial blockfrost returns this instead of bad request.
        return Err(StatusCode::NOT_FOUND.into());
    }
    let datum_hash = Hash::<32>::from(
        hex::decode(&datum_hash)
            .map_err(|_| StatusCode::NOT_FOUND)?
            .as_slice(),
    );

    let datum = domain
        .query()
        .plutus_data(&datum_hash)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(ScriptDatum {
        json_value: PlutusDataWrapper(datum).into_model()?,
    }))
}


pub async fn by_script_hash<D>(
    Path(script_hash): Path<String>,
    State(domain): State<Facade<D>>,
) -> Result<Json<serde_json::Value>, Error>
where
    D: Domain + Clone + Send + Sync + 'static,
{
    // script hashes are blake2b-224 (28 bytes => 56 hex chars)
    if script_hash.len() != 56 {
        return Err(StatusCode::NOT_FOUND.into());
    }

    let key = hex::decode(&script_hash).map_err(|_| StatusCode::NOT_FOUND)?;

    // find any UTxO that references this script hash
    let utxos = domain
        .indexes()
        .utxos_by_tag(dolos_cardano::indexes::utxo_dimensions::REFERENCE_SCRIPT, &key)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let txoref = utxos.into_iter().next().ok_or(StatusCode::NOT_FOUND)?;

    // load the UTxO and extract the script_ref
    let utxos_map = domain
        .state()
        .get_utxos(vec![txoref.clone()])
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let era_cbor = utxos_map.get(&txoref).ok_or(StatusCode::NOT_FOUND)?;

    let output = pallas::ledger::traverse::MultiEraOutput::try_from(era_cbor.as_ref())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let script_ref = output.script_ref().ok_or(StatusCode::NOT_FOUND)?;

    // determine type & serialised size
    use pallas::codec::minicbor;
    use pallas::ledger::primitives::conway::ScriptRef as PScriptRef;

    let (typ, size) = match script_ref {
        PScriptRef::NativeScript(ns) => ("native", ns.raw_cbor().len()),
        PScriptRef::PlutusV1Script(p) => (
            "plutusV1",
            minicbor::to_vec(&p).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.len(),
        ),
        PScriptRef::PlutusV2Script(p) => (
            "plutusV2",
            minicbor::to_vec(&p).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.len(),
        ),
        PScriptRef::PlutusV3Script(p) => (
            "plutusV3",
            minicbor::to_vec(&p).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.len(),
        ),
    };

    let resp = serde_json::json!({
        "script_hash": script_hash,
        "type": typ,
        "serialised_size": size,
    });

    Ok(Json(resp))
}


#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use axum::body::Body;
    use dolos_core::{config::MinibfConfig, EraCbor, TxoRef, UtxoSetDelta};
    use dolos_testing::{tx_sequence_to_hash, toy_domain::ToyDomain};
    use pallas::ledger::primitives::conway::ScriptRef;
    use pallas::ledger::traverse::{Era, MultiEraTx, ComputeHash, OriginalHash};
    use serde_json::Value as JsonValue;
    use std::sync::Arc;
    use tower::ServiceExt;
    use axum::body;

    #[tokio::test]
    async fn test_by_script_hash_returns_plutus_v1() {
        // Load pre-encoded output CBOR (contains a reference script) from test_data
        let hex = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/test_data/reference_script_utxo.hex")).trim();
        let cbor_bytes = hex::decode(hex).expect("invalid hex in test data");

        // the fixture contains a full Conway transaction; decode the tx and
        // extract the output that contains a reference script
        let tx = MultiEraTx::decode_for_era(Era::Conway, &cbor_bytes)
            .expect("test CBOR must decode to a Conway tx");
        let output = tx
            .outputs()
            .into_iter()
            .find(|o| o.script_ref().is_some())
            .expect("test tx must contain an output with a script_ref");

        let output_cbor = output.encode();
        let era_cbor = EraCbor(Era::Conway.into(), output_cbor);

        // extract the referenced script and compute its canonical hash
        let script_ref = output.script_ref().expect("test CBOR must contain a script_ref");
        let script_hash = match &script_ref {
            ScriptRef::NativeScript(ns) => ns.original_hash().to_vec(),
            ScriptRef::PlutusV1Script(p) => p.compute_hash().to_vec(),
            ScriptRef::PlutusV2Script(p) => p.compute_hash().to_vec(),
            ScriptRef::PlutusV3Script(p) => p.compute_hash().to_vec(),
        };
        let script_hash_hex = hex::encode(&script_hash);

        // Put into a UTxO delta and create a ToyDomain (which will index the UTxO)
        let mut delta = UtxoSetDelta::default();
        let tx_hash = tx_sequence_to_hash(42);
        let txoref = TxoRef(tx_hash, 0);
        delta.produced_utxo.insert(txoref.clone(), Arc::new(era_cbor));

        let domain = ToyDomain::new(Some(delta), None);

        // sanity-check: indexes should contain the produced UTxO tagged with the
        // reference script hash and the state should contain the UTxO
        let key = script_hash.clone();
        let utxos_indexed = domain
            .indexes()
            .utxos_by_tag(dolos_cardano::indexes::utxo_dimensions::REFERENCE_SCRIPT, &key)
            .expect("indexes query failed");
        assert!(utxos_indexed.into_iter().next().is_some(), "UTxO was not indexed with REFERENCE_SCRIPT tag");

        let utxos_map = domain
            .state()
            .get_utxos(vec![txoref.clone()])
            .expect("state.get_utxos failed");
        assert!(utxos_map.get(&txoref).is_some(), "UTxO not present in state");

        let cfg = MinibfConfig {
            listen_address: "[::]:0".parse().unwrap(),
            permissive_cors: None,
            token_registry_url: None,
            url: None,
            base_path: None,
        };

        let app = crate::build_router(cfg, domain);

        let req = Request::builder()
            .method("GET")
            .uri(format!("/scripts/{}", script_hash_hex))
            .body(Body::empty())
            .unwrap();

        let resp = app.clone().oneshot(req).await.expect("router call");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        let body_bytes = body::to_bytes(resp.into_body(), 16 * 1024 * 1024).await.unwrap();
        let json: JsonValue = serde_json::from_slice(&body_bytes).unwrap();

        let expected_type = match script_ref {
            ScriptRef::NativeScript(_) => "native",
            ScriptRef::PlutusV1Script(_) => "plutusV1",
            ScriptRef::PlutusV2Script(_) => "plutusV2",
            ScriptRef::PlutusV3Script(_) => "plutusV3",
        };

        assert_eq!(json["script_hash"].as_str().unwrap(), script_hash_hex);
        assert_eq!(json["type"].as_str().unwrap(), expected_type);
        assert!(json["serialised_size"].as_u64().unwrap() > 0);
    }
}

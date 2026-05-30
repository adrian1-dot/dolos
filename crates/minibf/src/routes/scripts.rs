#![allow(dead_code)]
use axum::{
    extract::{Path, State},
    Json,
};
use blockfrost_openapi::models::script_datum::ScriptDatum;
use dolos_cardano::indexes::AsyncCardanoQueryExt;
use dolos_core::{Domain, IndexStore, StateStore, ArchiveStore};
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

    use axum::{response::IntoResponse, response::Response};
    use axum::body::Body;

        /// Blockfrost: GET /scripts/{script_hash}/cbor
        pub async fn script_cbor_by_hash<D>(
            Path(script_hash): Path<String>,
            State(domain): State<Facade<D>>,
        ) -> Result<Response, Error>
        where
            D: Domain + Clone + Send + Sync + 'static,
        {
            // script hashes are blake2b-224 (28 bytes => 56 hex chars)
            if script_hash.len() != 56 {
                return Ok(StatusCode::NOT_FOUND.into_response());
            }

            let key = hex::decode(&script_hash).map_err(|_| StatusCode::NOT_FOUND)?;

            // Try to find pointer(s) for this script hash (preferred). If the pointer index is empty, fall back to UTxO-filter index.
            let pointers = domain
                .indexes()
                .reference_script_pointers(&key)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let maybe_pointer = pointers.into_iter().next();

            let (txoref, utxos_map, pointer_for_archive): (
                dolos_core::TxoRef,
                std::collections::HashMap<dolos_core::TxoRef, std::sync::Arc<dolos_core::EraCbor>>,
                Option<dolos_core::indexes::ReferenceScriptPointer>,
            ) = if let Some(pointer) = maybe_pointer.clone() {
                let tx_hash = pallas::crypto::hash::Hash::<32>::from(pointer.tx_hash.as_slice());
                let txoref = dolos_core::TxoRef(tx_hash, pointer.output_index);
                let utxos_map = domain
                    .state()
                    .get_utxos(vec![txoref.clone()])
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                (txoref, utxos_map, Some(pointer))
            } else {
                let utxos_indexed = domain
                    .indexes()
                    .utxos_by_tag(dolos_cardano::indexes::utxo_dimensions::REFERENCE_SCRIPT, &key)
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                let txoref = utxos_indexed
                    .into_iter()
                    .next()
                    .ok_or(StatusCode::NOT_FOUND)?;
                let utxos_map = domain
                    .state()
                    .get_utxos(vec![txoref.clone()])
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                (txoref, utxos_map, None)
            };

            use pallas::codec::minicbor;
            use pallas::ledger::primitives::conway::ScriptRef as PScriptRef;

            // Try state first
            if let Some(era_cbor) = utxos_map.get(&txoref) {
                let output = pallas::ledger::traverse::MultiEraOutput::try_from(era_cbor.as_ref())
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                if let Some(script_ref) = output.script_ref() {
                    let cbor_bytes = match script_ref {
                        PScriptRef::NativeScript(ns) => ns.raw_cbor().to_vec(),
                        PScriptRef::PlutusV1Script(p) => minicbor::to_vec(&p).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                        PScriptRef::PlutusV2Script(p) => minicbor::to_vec(&p).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                        PScriptRef::PlutusV3Script(p) => minicbor::to_vec(&p).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                    };
                    let json = serde_json::json!({ "cbor": hex::encode(cbor_bytes) });
                    return Ok(Response::builder()
                        .status(StatusCode::OK)
                        .header("Content-Type", "application/json")
                        .body(Body::from(json.to_string()))
                        .unwrap());
                }
            }

            // Archive fallback
            let pointer = pointer_for_archive.as_ref().ok_or(StatusCode::NOT_FOUND)?;
            let raw_block = domain
                .archive()
                .get_block_by_slot(&pointer.slot)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .ok_or(StatusCode::NOT_FOUND)?;
            let block = pallas::ledger::traverse::MultiEraBlock::decode(&raw_block)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let txs = block.txs();
            let tx = txs
                .iter()
                .find(|tx: &&pallas::ledger::traverse::MultiEraTx| tx.hash().to_vec() == pointer.tx_hash)
                .ok_or(StatusCode::NOT_FOUND)?;
            let outputs = tx.outputs();
            let output = outputs
                .get(pointer.output_index as usize)
                .ok_or(StatusCode::NOT_FOUND)?;
            if let Some(script_ref) = output.script_ref() {
                let cbor_bytes = match script_ref {
                    PScriptRef::NativeScript(ns) => ns.raw_cbor().to_vec(),
                    PScriptRef::PlutusV1Script(p) => minicbor::to_vec(&p).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                    PScriptRef::PlutusV2Script(p) => minicbor::to_vec(&p).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                    PScriptRef::PlutusV3Script(p) => minicbor::to_vec(&p).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                };
                let json = serde_json::json!({ "cbor": hex::encode(cbor_bytes) });
                return Ok(Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "application/json")
                    .body(Body::from(json.to_string()))
                    .unwrap());
            }
            Ok(StatusCode::NOT_FOUND.into_response())
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

    // Try to find pointer(s) for this script hash (preferred). If the
    // pointer index is empty, fall back to the UTxO-filter index so minibf
    // works for domains that only have UTxO-filter data (e.g. ToyDomain).
    let pointers = domain
        .indexes()
        .reference_script_pointers(&key)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let maybe_pointer = pointers.into_iter().next();

    // Resolve a txoref + utxos_map using either the pointer (fast path) or the
    // utxos_by_tag fallback (when pointer index is not populated).
    let (txoref, utxos_map, pointer_for_archive): (
        dolos_core::TxoRef,
        std::collections::HashMap<dolos_core::TxoRef, std::sync::Arc<dolos_core::EraCbor>>,
        Option<dolos_core::indexes::ReferenceScriptPointer>,
    ) = if let Some(pointer) = maybe_pointer.clone() {
        let tx_hash = Hash::<32>::from(pointer.tx_hash.as_slice());
        let txoref = dolos_core::TxoRef(tx_hash, pointer.output_index);
        let utxos_map = domain
            .state()
            .get_utxos(vec![txoref.clone()])
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        (txoref, utxos_map, Some(pointer))
    } else {
        // fallback: find a live UTxO indexed by the reference script hash
        let utxos_indexed = domain
            .indexes()
            .utxos_by_tag(dolos_cardano::indexes::utxo_dimensions::REFERENCE_SCRIPT, &key)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let txoref = utxos_indexed
            .into_iter()
            .next()
            .ok_or(StatusCode::NOT_FOUND)?;

        let utxos_map = domain
            .state()
            .get_utxos(vec![txoref.clone()])
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        (txoref, utxos_map, None)
    };

// determine type & serialised size by inspecting either the state UTxO or
    // the archived block (decoded). We compute `typ` and `size` here so we
    // avoid returning references into temporaries.
    use pallas::codec::minicbor;
    use pallas::ledger::primitives::conway::ScriptRef as PScriptRef;

    let (typ, size) = if let Some(era_cbor) = utxos_map.get(&txoref) {
        let output = pallas::ledger::traverse::MultiEraOutput::try_from(era_cbor.as_ref())
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        match output.script_ref() {
            Some(PScriptRef::NativeScript(ns)) => ("native", ns.raw_cbor().len()),
            Some(PScriptRef::PlutusV1Script(p)) => (
                "plutusV1",
                minicbor::to_vec(&p).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.len(),
            ),
            Some(PScriptRef::PlutusV2Script(p)) => (
                "plutusV2",
                minicbor::to_vec(&p).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.len(),
            ),
            Some(PScriptRef::PlutusV3Script(p)) => (
                "plutusV3",
                minicbor::to_vec(&p).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.len(),
            ),
            None => return Err(StatusCode::NOT_FOUND.into()),
        }
    } else {
        // Archive fallback only possible when we resolved a pointer earlier.
        let pointer = pointer_for_archive
            .as_ref()
            .ok_or(StatusCode::NOT_FOUND)?;

        let raw_block = domain
            .archive()
            .get_block_by_slot(&pointer.slot)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::NOT_FOUND)?;
        let block = pallas::ledger::traverse::MultiEraBlock::decode(&raw_block)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let txs = block.txs();
        let tx = txs
            .iter()
            .find(|tx: &&pallas::ledger::traverse::MultiEraTx| tx.hash().to_vec() == pointer.tx_hash)
            .ok_or(StatusCode::NOT_FOUND)?;
        let outputs = tx.outputs();
        let output = outputs
            .get(pointer.output_index as usize)
            .ok_or(StatusCode::NOT_FOUND)?;

        match output.script_ref() {
            Some(PScriptRef::NativeScript(ns)) => ("native", ns.raw_cbor().len()),
            Some(PScriptRef::PlutusV1Script(p)) => (
                "plutusV1",
                minicbor::to_vec(&p).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.len(),
            ),
            Some(PScriptRef::PlutusV2Script(p)) => (
                "plutusV2",
                minicbor::to_vec(&p).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.len(),
            ),
            Some(PScriptRef::PlutusV3Script(p)) => (
                "plutusV3",
                minicbor::to_vec(&p).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.len(),
            ),
            None => return Err(StatusCode::NOT_FOUND.into()),
        }
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
        #[tokio::test]
        async fn test_script_cbor_by_hash_returns_cbor() {
            // Load pre-encoded output CBOR (contains a reference script) from test_data
            let hex = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/test_data/reference_script_utxo.hex")).trim();
            let cbor_bytes = hex::decode(hex).expect("invalid hex in test data");

            let tx = MultiEraTx::decode_for_era(Era::Conway, &cbor_bytes)
                .expect("test CBOR must decode to a Conway tx");
            let output = tx
                .outputs()
                .into_iter()
                .find(|o| o.script_ref().is_some())
                .expect("test tx must contain an output with a script_ref");

            let output_cbor = output.encode();
            let era_cbor = EraCbor(Era::Conway.into(), output_cbor);

            let script_ref = output.script_ref().expect("test CBOR must contain a script_ref");
            let script_hash = match &script_ref {
                ScriptRef::NativeScript(ns) => ns.original_hash().to_vec(),
                ScriptRef::PlutusV1Script(p) => p.compute_hash().to_vec(),
                ScriptRef::PlutusV2Script(p) => p.compute_hash().to_vec(),
                ScriptRef::PlutusV3Script(p) => p.compute_hash().to_vec(),
            };
            let script_hash_hex = hex::encode(&script_hash);

            let mut delta = UtxoSetDelta::default();
            let tx_hash = tx_sequence_to_hash(42);
            let txoref = TxoRef(tx_hash, 0);
            delta.produced_utxo.insert(txoref.clone(), Arc::new(era_cbor));

            let domain = ToyDomain::new(Some(delta), None);
            let cfg = MinibfConfig {
                listen_address: "[::]:0".parse().unwrap(),
                permissive_cors: None,
                token_registry_url: None,
                url: None,
                base_path: None,
                hardcoded_network: None,
            };
            let app = crate::build_router(cfg, domain);
            let app = app.into_inner();

            let req = Request::builder()
                .method("GET")
                .uri(format!("/scripts/{}/cbor", script_hash_hex))
                .body(Body::empty())
                .unwrap();

            let resp = ServiceExt::oneshot(app.clone(), req).await.expect("router call");
            assert_eq!(resp.status(), reqwest::StatusCode::OK);

            let body_bytes = body::to_bytes(resp.into_body(), 16 * 1024 * 1024).await.unwrap();
            // The returned body should be a JSON object with a hex string under "cbor"
            let json: serde_json::Value = serde_json::from_slice(&body_bytes).expect("valid JSON");
            let expected_cbor = match script_ref {
                ScriptRef::NativeScript(ns) => ns.raw_cbor().to_vec(),
                ScriptRef::PlutusV1Script(p) => pallas::codec::minicbor::to_vec(&p).unwrap(),
                ScriptRef::PlutusV2Script(p) => pallas::codec::minicbor::to_vec(&p).unwrap(),
                ScriptRef::PlutusV3Script(p) => pallas::codec::minicbor::to_vec(&p).unwrap(),
            };
            let expected_hex = hex::encode(expected_cbor);
            assert_eq!(json["cbor"].as_str().unwrap(), expected_hex);
        }
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

        // We expect the UTxO to be indexed by `REFERENCE_SCRIPT` (utxos_by_tag)
        // but a dedicated pointer entry may not exist for this test fixture.
        let utxos_indexed = domain
            .indexes()
            .utxos_by_tag(dolos_cardano::indexes::utxo_dimensions::REFERENCE_SCRIPT, &key)
            .expect("indexes query failed");
        assert!(utxos_indexed.into_iter().next().is_some(), "UTxO was not indexed with REFERENCE_SCRIPT tag");

        let utxos_map = domain
            .state()
            .get_utxos(vec![txoref.clone()])
            .expect("state.get_utxos failed");
        assert!(utxos_map.contains_key(&txoref), "UTxO not present in state");

        let cfg = MinibfConfig {
            listen_address: "[::]:0".parse().unwrap(),
            permissive_cors: None,
            token_registry_url: None,
            url: None,
            base_path: None,
            hardcoded_network: None,
        };

        let app = crate::build_router(cfg, domain);
        let app = app.into_inner();

        let req = Request::builder()
            .method("GET")
            .uri(format!("/scripts/{}", script_hash_hex))
            .body(Body::empty())
            .unwrap();

        let resp = ServiceExt::oneshot(app.clone(), req).await.expect("router call");
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

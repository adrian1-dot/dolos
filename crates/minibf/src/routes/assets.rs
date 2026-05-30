use std::{collections::HashMap, time::Duration};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use blockfrost_openapi::models::{
    asset::{Asset},
    asset_addresses_inner::AssetAddressesInner,
    asset_metadata::AssetMetadata as OffchainMetadata,
    asset_transactions_inner::AssetTransactionsInner,
};
use dolos_cardano::{
    cip68::{cip_68_reference_asset, Cip68TokenStandard},
    indexes::{AsyncCardanoQueryExt, CardanoIndexExt, SlotOrder},
    model::AssetState,
    ChainSummary,
};
use dolos_core::{BlockSlot, Domain, EraCbor, IndexStore as _, StateStore as _};
use futures_util::StreamExt;
use itertools::Itertools;
use pallas::{
    codec::minicbor,
    crypto::hash::Hash,
    ledger::{
        primitives::{Metadatum, PlutusData},
        traverse::{MultiEraBlock, MultiEraOutput, MultiEraTx},
    },
};
use serde::Deserialize;

use crate::{
    error::Error,
    mapping::{asset_fingerprint, IntoModel},
    pagination::{Order, Pagination, PaginationParameters},
    Facade,
};


#[derive(Debug, Deserialize)]
pub struct TokenRegistryValue<T> {
    pub value: T,
}
#[derive(Debug, Deserialize)]
pub struct TokenRegistryMetadata {
    pub name: Option<TokenRegistryValue<String>>,
    pub description: Option<TokenRegistryValue<String>>,
    pub ticker: Option<TokenRegistryValue<String>>,
    pub url: Option<TokenRegistryValue<String>>,
    pub logo: Option<TokenRegistryValue<String>>,
    pub decimals: Option<TokenRegistryValue<i32>>,
}
impl From<TokenRegistryMetadata> for OffchainMetadata {
    fn from(token_registry_asset: TokenRegistryMetadata) -> Self {
        Self {
            name: token_registry_asset.name.as_ref().unwrap().value.clone(),
            description: token_registry_asset
                .description
                .as_ref()
                .unwrap()
                .value
                .clone(),
            ticker: token_registry_asset
                .ticker
                .as_ref()
                .map(|v| v.value.clone()),
            url: token_registry_asset.url.as_ref().map(|v| v.value.clone()),
            logo: token_registry_asset.logo.as_ref().map(|v| v.value.clone()),
            decimals: token_registry_asset.decimals.as_ref().map(|v| v.value),
        }
    }
}

#[allow(dead_code)]
struct CIP25Metadata(Metadatum);
impl IntoModel<serde_json::Value> for CIP25Metadata {
    type SortKey = ();

    fn into_model(self) -> Result<serde_json::Value, StatusCode> {
        Ok(match self.0 {
            Metadatum::Int(x) => serde_json::Number::from_i128(x.into())
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::String(x.to_string())),

            Metadatum::Text(x) => serde_json::Value::String(x),

            Metadatum::Bytes(x) => match String::from_utf8(x.to_vec().clone()) {
                Ok(s) => serde_json::Value::String(s),
                Err(_) => serde_json::Value::String(hex::encode(x.to_vec())),
            },

            Metadatum::Array(x) => {
                let values = x
                    .into_iter()
                    .map(|d| CIP25Metadata(d).into_model())
                    .collect::<Result<Vec<_>, _>>()?;
                serde_json::Value::Array(values)
            }

            Metadatum::Map(x) => {
                let mut map = serde_json::Map::new();
                for (k, v) in x.iter() {
                    if let Some(key) = CIP25Metadata(k.clone()).into_model()?.as_str() {
                        map.insert(key.to_string(), CIP25Metadata(v.clone()).into_model()?);
                    }
                }
                serde_json::Value::Object(map)
            }
        })
    }
}

#[allow(dead_code)]
async fn datum_from_hash<D>(
    domain: &Facade<D>,
    hash: Hash<32>,
) -> Result<Option<PlutusData>, StatusCode>
where
    D: Domain + Clone + Send + Sync + 'static,
{
    let Some(bytes) = domain
        .query()
        .get_datum(&hash)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    else {
        return Ok(None);
    };

    let datum = minicbor::decode(&bytes).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Some(datum))
}

#[allow(dead_code)]
fn cip68_reference_from_unit(
    unit: &str,
) -> Result<Option<(String, Cip68TokenStandard)>, StatusCode> {
    if unit.len() < 56 {
        return Ok(None);
    }

    let policy_id = &unit[..56];
    let asset_name = &unit[56..];
    cip_68_reference_asset(policy_id, asset_name).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[allow(dead_code)]
fn decode_era_tx(era: u16, cbor: &[u8]) -> Result<MultiEraTx<'_>, StatusCode> {
    let era = pallas::ledger::traverse::Era::try_from(era)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    MultiEraTx::decode_for_era(era, cbor).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}


#[allow(dead_code)]
struct AssetModelBuilder {
    subject: Vec<u8>,
    unit: String,
    asset_state: dolos_cardano::model::AssetState,
    initial_tx: Option<EraCbor>,
    registry_url: Option<String>,
}

impl AssetModelBuilder {
    #[allow(dead_code)]
    async fn offchain_metadata(&self, asset: &str) -> Result<Option<OffchainMetadata>, StatusCode> {
        // TODO: apply memory cache
        let Some(url) = &self.registry_url else {
            return Ok(None);
        };

        let url = format!("{url}/metadata/{asset}");

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("Dolos MiniBF")
            .build()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let res = client
            .get(&url)
            .send()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        if res.status() != StatusCode::OK {
            return Ok(None);
        }

        let metadata: TokenRegistryMetadata = res
            .json()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        if metadata.name.is_none() || metadata.description.is_none() {
            return Ok(None);
        }

        Ok(Some(metadata.into()))
    }

    #[allow(dead_code)]
    async fn into_model<D>(self, _domain: &Facade<D>) -> Result<Asset, StatusCode>
    where
        D: Domain + Clone + Send + Sync + 'static,
        Option<AssetState>: From<D::Entity>,
    {
        let policy = self.subject[..28].to_vec();
        let asset = self.subject[28..].to_vec();

        let metadata = self.offchain_metadata(&self.unit).await?.map(Box::new);

        let asset_name = hex::encode(&asset);
        let asset_name = (!asset_name.is_empty()).then_some(asset_name);

        let out = Asset {
            asset: hex::encode(&self.subject),
            policy_id: hex::encode(policy),
            asset_name,
            fingerprint: asset_fingerprint(&self.subject)?,
            quantity: self.asset_state.quantity().to_string(),
            initial_mint_tx_hash: self
                .asset_state
                .initial_tx
                .map(|h| h.to_string())
                .unwrap_or_default(),
            mint_or_burn_count: self.asset_state.mint_tx_count as i32,
            metadata,
            onchain_metadata: None,
            onchain_metadata_extra: None,
            onchain_metadata_standard: None,
        };

        Ok(out)
    }
}

#[allow(dead_code)]
pub async fn by_subject<D>(
    Path(unit): Path<String>,
    State(domain): State<Facade<D>>,
) -> Result<Json<Asset>, StatusCode>
where
    Option<AssetState>: From<D::Entity>,
    D: Domain + Clone + Send + Sync + 'static,
{
    let subject = hex::decode(&unit).map_err(|_| StatusCode::BAD_REQUEST)?;
    let entity_key = pallas::crypto::hash::Hasher::<256>::hash(subject.as_slice());

    let registry_url = domain.config.token_registry_url.clone();

    let asset_state = domain
        .read_cardano_entity::<AssetState>(entity_key.as_slice())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let initial_tx = if let Some(initial_tx) = asset_state.initial_tx {
        domain
            .query()
            .tx_cbor(initial_tx.as_slice().to_vec())
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        None
    };

    let model = AssetModelBuilder {
        subject,
        unit,
        asset_state,
        initial_tx,
        registry_url,
    };

    Ok(Json(model.into_model(&domain).await?))
}

#[allow(dead_code)]
pub async fn by_subject_addresses<D>(
    Path(subject): Path<String>,
    Query(params): Query<PaginationParameters>,
    State(domain): State<Facade<D>>,
) -> Result<Json<Vec<AssetAddressesInner>>, Error>
where
    D: Domain + Clone + Send + Sync + 'static,
{
    let pagination = Pagination::try_from(params)?;
    let asset = hex::decode(&subject).map_err(|_| Error::InvalidAsset)?;
    let utxoset = domain
        .indexes()
        .utxos_by_asset(&asset)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .into_iter()
        .collect_vec();

    let utxos = domain
        .state()
        .get_utxos(utxoset)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut addresses: HashMap<String, ((BlockSlot, u32), u128)> = HashMap::new();
    for (txoref, eracbor) in utxos {
        let sort = (
            domain
                .indexes()
                .slot_by_tx_hash(txoref.0.as_slice())
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?,
            txoref.1,
        );

        let utxo = MultiEraOutput::decode(eracbor.0.try_into().unwrap(), &eracbor.1)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let amount = utxo
            .value()
            .assets()
            .iter()
            .flat_map(|x| {
                let subject = x.policy().to_vec();
                x.assets()
                    .iter()
                    .find(|x| [subject.as_slice(), x.name()].concat() == asset.as_slice())
                    .map(|x| x.any_coin() as u128)
            })
            .sum();

        addresses
            .entry(
                utxo.address()
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                    .to_string(),
            )
            .and_modify(|entry| {
                entry.0 = entry.0.min(sort);
                entry.1 += amount;
            })
            .or_insert((sort, amount));
    }

    let mut items = addresses
        .into_iter()
        .sorted_by_key(|(_, (sort, _))| *sort)
        .map(|(address, (_, amount))| AssetAddressesInner {
            address,
            quantity: amount.to_string(),
        })
        .collect_vec();

    if matches!(pagination.order, Order::Desc) {
        items.reverse();
    }

    let sorted = items
        .into_iter()
        .skip(pagination.skip())
        .take(pagination.count)
        .collect_vec();

    Ok(Json(sorted))
}

#[allow(dead_code)]
fn subject_matches(subject: &[u8], policy: &[u8], name: &[u8]) -> bool {
    [policy, name].concat() == subject
}

#[allow(dead_code)]
fn output_has_subject(subject: &[u8], output: &MultiEraOutput) -> bool {
    for pa in output.value().assets() {
        for asset in pa.assets() {
            if subject_matches(subject, pa.policy().as_slice(), asset.name()) {
                return true;
            }
        }
    }
    false
}

#[allow(dead_code)]
async fn tx_has_subject<D>(
    domain: &Facade<D>,
    subject: &[u8],
    tx: &MultiEraTx<'_>,
) -> Result<bool, StatusCode>
where
    D: Domain + Clone + Send + Sync + 'static,
{
    for (_, output) in tx.produces() {
        if output_has_subject(subject, &output) {
            return Ok(true);
        }
    }

    for input in tx.consumes() {
        if let Some(EraCbor(era, cbor)) = domain
            .query()
            .tx_cbor(input.hash().as_slice().to_vec())
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        {
            let parsed = MultiEraTx::decode_for_era(
                era.try_into()
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                &cbor,
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            if let Some(output) = parsed.produces_at(input.index() as usize) {
                if output_has_subject(subject, &output) {
                    return Ok(true);
                }
            }
        }
    }

    Ok(false)
}

#[allow(dead_code)]
async fn find_txs<D>(
    domain: &Facade<D>,
    subject: &[u8],
    chain: &ChainSummary,
    pagination: &Pagination,
    block: &[u8],
) -> Result<Vec<AssetTransactionsInner>, StatusCode>
where
    D: Domain + Clone + Send + Sync + 'static,
{
    let block = MultiEraBlock::decode(block).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut matches = vec![];

    for (idx, tx) in block.txs().iter().enumerate() {
        if !pagination.should_skip(block.number(), idx)
            && tx_has_subject(domain, subject, tx).await?
        {
            let model = AssetTransactionsInner {
                tx_hash: hex::encode(tx.hash().as_slice()),
                tx_index: idx as i32,
                block_height: block.number() as i32,
                block_time: chain.slot_time(block.slot()) as i32,
            };

            matches.push(model);
        }
    }

    if matches!(pagination.order, Order::Desc) {
        matches = matches.into_iter().rev().collect();
    }

    Ok(matches)
}

#[allow(dead_code)]
pub async fn by_subject_transactions<D>(
    Path(subject): Path<String>,
    Query(params): Query<PaginationParameters>,
    State(domain): State<Facade<D>>,
) -> Result<Json<Vec<AssetTransactionsInner>>, Error>
where
    D: Domain + Clone + Send + Sync + 'static,
{
    let pagination = Pagination::try_from(params)?;
    pagination.enforce_max_scan_limit()?;

    let subject = hex::decode(&subject).map_err(|_| Error::InvalidAsset)?;
    let end_slot = domain.get_tip_slot()?;
    let stream = domain.query().blocks_by_asset_stream(
        &subject,
        0,
        end_slot,
        SlotOrder::from(pagination.order),
    );

    let chain = domain
        .get_chain_summary()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut matches = Vec::new();
    let mut stream = Box::pin(stream);

    while let Some(res) = stream.next().await {
        let (_slot, block) = res.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let Some(block) = block else {
            continue;
        };

        let mut txs = find_txs(&domain, &subject, &chain, &pagination, &block)
            .await
            .map_err(Error::Code)?;
        matches.append(&mut txs);

        if matches.len() >= pagination.from() + pagination.count {
            break;
        }
    }

    let transactions = matches
        .into_iter()
        .skip(pagination.from())
        .take(pagination.count)
        .collect();

    Ok(Json(transactions))
}

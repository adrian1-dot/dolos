use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use blockfrost_openapi::models::epoch_param_content::EpochParamContent;
use pallas::ledger::{primitives::Epoch, traverse::MultiEraBlock};

use dolos_core::{archive::Skippable as _, ArchiveStore, Domain};

use crate::{
    error::Error,
    mapping::IntoModel as _,
    pagination::{Order, Pagination, PaginationParameters},
    Facade,
};

pub mod cost_models;
pub mod mapping;

pub async fn latest_parameters<D: Domain>(
    State(domain): State<Facade<D>>,
) -> Result<Json<EpochParamContent>, Error> {
    eprintln!("latest_parameters: handler start");

    let tip = match domain.get_tip_slot() {
        Ok(t) => { eprintln!("latest_parameters: tip={} (ok)", t); t }
        Err(e) => {
            eprintln!("latest_parameters: get_tip_slot -> {:?}", e);
            return Err(Error::Code(e));
        }
    };

    let summary = match domain.get_chain_summary() {
        Ok(s) => { eprintln!("latest_parameters: got chain_summary"); s }
        Err(e) => {
            eprintln!("latest_parameters: get_chain_summary -> {:?}", e);
            return Err(Error::Code(e));
        }
    };

    eprintln!("latest_parameters: summary.start_slot={}", summary.epoch_start(0));

    let (epoch, _) = summary.slot_epoch(tip);
    eprintln!("latest_parameters: computed epoch from tip = {}", epoch);

    let state = match dolos_cardano::load_epoch::<D>(domain.state()) {
        Ok(s) => { eprintln!("latest_parameters: load_epoch ok"); s }
        Err(e) => {
            eprintln!("latest_parameters: load_epoch -> {:?}", e);
            return Err(Error::Code(StatusCode::INTERNAL_SERVER_ERROR));
        }
    };

    eprintln!("latest_parameters: pparams present? {}", state.pparams.live().is_some());

    let model = mapping::ParametersModelBuilder {
        epoch,
        params: state.pparams.live().cloned().unwrap_or_default(),
        genesis: &domain.genesis(),
        nonce: state.nonces.map(|x| x.active.to_string()),
    };

    eprintln!("latest_parameters: building model -> attempting into_response");

    match model.into_response() {
        Ok(json) => {
            eprintln!("latest_parameters: model.into_response ok");
            Ok(json)
        }
        Err(e) => {
            eprintln!("latest_parameters: model.into_response -> {:?}", e);
            Err(Error::Code(e))
        }
    }
}

pub async fn by_number_parameters<D: Domain>(
    State(domain): State<Facade<D>>,
    Path(epoch): Path<Epoch>,
) -> Result<Json<EpochParamContent>, Error> {
    let tip = domain.get_tip_slot()?;
    let summary = domain.get_chain_summary()?;
    let (curr, _) = summary.slot_epoch(tip);

    let epoch = if epoch == curr {
        dolos_cardano::load_epoch::<D>(domain.state())
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        domain
            .get_epoch_log(epoch, &summary)?
            .ok_or(StatusCode::NOT_FOUND)?
    };

    let model = mapping::ParametersModelBuilder {
        epoch: epoch.number,
        params: epoch.pparams.live().cloned().unwrap_or_default(),
        genesis: &domain.genesis(),
        nonce: epoch.nonces.map(|x| x.active.to_string()),
    };

    Ok(model.into_response()?)
}

pub async fn by_number_blocks<D: Domain>(
    Path(epoch): Path<u64>,
    Query(params): Query<PaginationParameters>,
    State(domain): State<Facade<D>>,
) -> Result<Json<Vec<String>>, Error> {
    let chain = domain.get_chain_summary()?;
    let pagination = Pagination::try_from(params)?;
    let start = chain.epoch_start(epoch);
    let end = chain.epoch_start(epoch + 1) - 1;

    let mut iter = domain
        .archive()
        .get_range(Some(start), Some(end))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Skip past pages using key-only traversal (no block data read).
    match pagination.order {
        Order::Asc => iter.skip_forward(pagination.skip()),
        Order::Desc => iter.skip_backward(pagination.skip()),
    }

    let decode = |(_slot, body): (_, Vec<u8>)| -> Result<String, StatusCode> {
        let block = MultiEraBlock::decode(&body).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(block.hash().to_string())
    };

    Ok(Json(match pagination.order {
        Order::Asc => iter
            .take(pagination.count)
            .map(decode)
            .collect::<Result<_, StatusCode>>()?,
        Order::Desc => iter
            .rev()
            .take(pagination.count)
            .map(decode)
            .collect::<Result<_, _>>()?,
    }))
}

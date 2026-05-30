use dolos_cardano::forks::force_pparams_version;

#[test]
fn force_pparams_version_into_alonzo_populates_alonzo_fields() {
    let genesis = dolos_cardano::include::preview::load();

    let initial = dolos_cardano::PParamsSet::default();

    // migrate from protocol 0 -> 5 (Alonzo)
    let pparams = force_pparams_version(&initial, &genesis, 0, 5).expect("migration failed");

    // Alonzo should populate execution costs and max-ex-units
    assert!(pparams.execution_costs().is_some(), "execution costs missing");
    assert!(pparams.max_tx_ex_units().is_some(), "max_tx_ex_units missing");
    assert!(pparams.cost_models_plutus_v1().is_some() || pparams.cost_models_for_script_languages().plutus_v1.is_some(), "cost models (PlutusV1) missing");
}

#[test]
fn force_pparams_version_into_conway_populates_conway_fields() {
    let genesis = dolos_cardano::include::preview::load();

    let initial = dolos_cardano::PParamsSet::default();

    // migrate from protocol 0 -> 9 (Conway)
    let pparams = force_pparams_version(&initial, &genesis, 0, 9).expect("migration failed");

    // Conway should populate PlutusV3 cost-models and governance fields
    assert!(pparams.cost_models_plutus_v3().is_some(), "PlutusV3 cost models missing");
    assert!(pparams.min_fee_ref_script_cost_per_byte().is_some(), "min_fee_ref_script_cost_per_byte missing");
}

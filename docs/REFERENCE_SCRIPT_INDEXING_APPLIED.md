# Reference Script Indexing — Changes applied

## Status
- Implemented (2026-02-14): UTxO-level reference-script indexing and minibf `/scripts/{script_hash}` endpoint.
- Scope: indexes current (live) UTxOs only; historical/archive lookup is a follow-up.

---

## Summary of changes
- Indexing: add `utxo::REFERENCE_SCRIPT` dimension and tag produced UTxOs when they include a `script_ref` (CIP-33).
- Extraction: use Pallas helpers (canonical types + hash utilities) to extract and canonicalise reference-script hashes.
- API: expose `GET /scripts/{script_hash}` in `dolos-minibf` that returns script metadata (type + serialised_size) for a live UTxO referencing the script.
- Tests & build: workspace builds and existing tests pass. (New unit + integration tests are recommended next steps.)

---

## Files changed (high level)
- `crates/cardano/src/indexes/dimensions.rs` — added `utxo::REFERENCE_SCRIPT`
- `crates/cardano/src/indexes/delta.rs` — `extract_utxo_tags` now extracts `script_ref` and pushes a `REFERENCE_SCRIPT` tag (uses Pallas `ComputeHash` / `OriginalHash` where appropriate)
- `crates/minibf/src/lib.rs` — registered route `GET /scripts/{script_hash}`
- `crates/minibf/src/routes/scripts.rs` — added handler `by_script_hash` (looks up UTxO by tag, loads `script_ref`, computes type & size)

---

## Implementation details
- Index key
  - Script hashes are indexed as raw bytes (blake2b-224) under the dimension `reference_script` for UTxO filter queries.

- Extraction logic
  - `CardanoIndexDeltaBuilder::extract_utxo_tags` detects `output.script_ref()` on Babbage/Conway outputs and adds a `Tag::new(utxo::REFERENCE_SCRIPT, <hash_bytes>)`.
  - Uses Pallas-provided helpers to compute canonical hashes (avoid manual CBOR hashing).

- minibf endpoint
  - Route: `GET /scripts/{script_hash}` (expects 56 hex chars = 28 bytes)
  - Lookup: `indexes().utxos_by_tag(utxo::REFERENCE_SCRIPT, hash)` → then `state().get_utxos([TxoRef])` to read the UTxO.
  - Response: JSON with `script_hash`, `type` (e.g. `plutusV1`, `plutusV2`, `native`, etc.), and `serialised_size` (bytes).
  - Behaviour: returns data for *live* UTxOs only; 404 if not found.

---

## Example response
{
  "script_hash": "13a3efd825703a352a8f71f4e2758d08c28c564e8dfcce9f77776ad1",
  "type": "plutusV1",
  "serialised_size": 3119
}

---

## How to verify locally
- Build & tests:
  - `cargo clippy --workspace --all-targets --all-features`
  - `cargo build --workspace --all-targets --all-features`
  - `cargo test --workspace --all-features`
- Run minibf against an in-memory domain containing a UTxO with `script_ref` and call:
  - `curl -sS http://localhost:your_port/scripts/<56-hex-chars>`

---

## Limitations & next steps
- Current scope: **current UTxOs only**. Historical/archive-backed lookup requires adding archive tags + QueryHelpers enhancements and reindexing.
- Add tests (PR follow-up):
  1. Unit test for `extract_utxo_tags` emitting `REFERENCE_SCRIPT`.
  2. minibf integration test that inserts a test UTxO with `script_ref` and asserts `GET /scripts/{script_hash}`.
- Optional: persist raw script CBOR in a `scripts` entity if fast re-use is desired.

---

## Notes for reviewers / maintainer
- I chose to re-use existing Pallas utilities for hashing and type handling to avoid subtle mismatches with on-chain hashing.
- The handler computes `serialised_size` from the same script representation used by Pallas to ensure consistency.

---

If you want, I can add the unit + integration tests next and prepare a PR with a changelog entry.

## Appendix — conversation & verification notes (2026-02-14)

- **Scope implemented:** index *current* UTxOs only; compute script metadata on‑demand (no raw CBOR persisted).
- **What was done:** added `utxo::REFERENCE_SCRIPT` dimension, tagged produced UTxOs when `script_ref` is present, implemented `GET /scripts/{script_hash}` in `dolos-minibf`, and added unit + integration tests.
- **Failing test → root cause:** minibf integration fixture was a *full Conway transaction* (not a single output); tests attempted to decode it as an output → CBOR type mismatch. Also the Redb index initially lacked a `reference_script` filter table.
- **Fix applied:** decode the fixture as a transaction, extract the output that contains `script_ref`, encode that output as `EraCbor` for indexing; added `reference_script` table in `dolos-redb3`; use Pallas `ComputeHash` / `OriginalHash` for canonical script hashing.
- **Important hashing note:** always use Pallas helpers (they include the language discriminator) — do **not** hash raw script bytes directly (that produces incorrect digests).
- **Verification:** updated unit + integration tests pass; clippy/build ran clean for the touched crates; minibf route returns correct `type` and `serialised_size` for referenced scripts.
- **Next steps / recommendations:** open a PR (include CHANGELOG); add tests for native, PlutusV1/V2/V3 variants; consider archive/historical indexing in a follow-up.

---

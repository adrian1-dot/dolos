# Reference Script Indexing in Dolos — Implementation & Status

## Status (2026-02-14)
- ✅ Implemented: UTxO-level reference-script indexing (CIP-33) and minibf endpoint `GET /scripts/{script_hash}`.
- Scope for this change: **current (live) UTxOs only**. Historical/archive lookup is a follow-up.

---

## Goals
- Index Cardano reference scripts at the UTxO level.
- Expose script metadata via `GET /scripts/{script_hash}` (type + serialised_size).
- Use canonical hashing and Pallas utilities to avoid mismatches (blake2b-224).
- Keep code well-tested, lint-clean, and maintainable.

---

## What was implemented (high-level)
- Added `utxo::REFERENCE_SCRIPT` index dimension.
- `CardanoIndexDeltaBuilder::extract_utxo_tags` now extracts `script_ref` and tags produced/consumed UTxOs by script hash.
- minibf: implemented `GET /scripts/{script_hash}` which:
  - finds a live UTxO by `reference_script` tag,
  - loads the UTxO from state,
  - returns `{ script_hash, type, serialised_size }`.
- All workspace builds & existing tests pass after changes.

---

## Key implementation details
- Extraction & indexing
  - Location: `crates/cardano/src/indexes/delta.rs` (`extract_utxo_tags`).
  - For Babbage/Conway outputs, if `output.script_ref()` is Some, compute the canonical hash and push:
    `Tag::new(utxo::REFERENCE_SCRIPT, <hash_bytes>)`.
  - Hashing and type handling use Pallas utilities (no ad-hoc CBOR hashing).

- Query & API
  - minibf route: `GET /scripts/{script_hash}` (registered in `crates/minibf/src/lib.rs`).
  - Handler: `crates/minibf/src/routes/scripts.rs::by_script_hash`.
  - Query flow: `indexes().utxos_by_tag(utxo::REFERENCE_SCRIPT, key)` → `state().get_utxos([TxoRef])` → read `script_ref`.
  - Response fields: `script_hash` (hex), `type` (e.g. `plutusV1`/`plutusV2`/`native`), `serialised_size` (bytes).

- Type & hash correctness
  - Use Pallas `ComputeHash` / `OriginalHash` / canonical ScriptRef enums to ensure on‑chain-compatible hash values.

---

## Example response

```json
{
  "script_hash": "13a3efd825703a352a8f71f4e2758d08c28c564e8dfcce9f77776ad1",
  "type": "plutusV1",
  "serialised_size": 3119
}
```

---

## Tests & verification
- Manual/automated checks performed:
  - `cargo clippy --workspace --all-targets --all-features` ✅
  - `cargo build --workspace --all-targets --all-features` ✅
  - `cargo test --workspace --all-features` ✅
- Recommended additional tests (follow-up PR):
  - Unit test: verify `extract_utxo_tags` emits `REFERENCE_SCRIPT` for outputs with `script_ref`.
  - minibf integration test: insert UTxO with `script_ref` into in-memory domain and assert `/scripts/{script_hash}` response.

---

## Limitations & next work
- This change covers *live* UTxOs only. To return historical/consumed reference scripts we must:
  - add archive tagging for scripts, and
  - extend QueryHelpers and minibf handler to search archive + join with blocks.
- Optional: store raw script CBOR in a `scripts` entity for faster reads — tradeoff: storage vs compute.

---

## Files changed (precise)
- `crates/cardano/src/indexes/dimensions.rs` — added `utxo::REFERENCE_SCRIPT`
- `crates/cardano/src/indexes/delta.rs` — `extract_utxo_tags` now tags `REFERENCE_SCRIPT`
- `crates/minibf/src/lib.rs` — added route `GET /scripts/{script_hash}`
- `crates/minibf/src/routes/scripts.rs` — implemented `by_script_hash` handler

---

## How to test manually
1. Start minibf connected to a domain that contains a UTxO with `script_ref`.
2. Compute the script hash (blake2b-224) and call:
   `curl -sS http://localhost:PORT/scripts/<56-hex-chars>`
3. Expect JSON with `script_hash`, `type`, and `serialised_size`.

---

If you'd like, I can now add the unit + minibf integration tests and open a PR with a changelog entry. (Recommended next step.)


# Plan: Implement UTxO-level Reference Script Indexing & API
TL;DR — Add UTxO-level indexing for Cardano reference scripts, expose a minibf /scripts/{script_hash} endpoint that returns script metadata (type + serialized_size) by looking up the current UTxO set, and add unit + integration tests. Key decisions: use Pallas utilities for hashing/type, compute metadata on-demand from UTxO (no raw-CBOR persistence), and limit scope to current UTxOs only (no archive historical lookup in this change). ✅

Why
REFERENCE_SCRIPT_INDEXING.md documents the feature but the codebase currently:
does not add a utxo::REFERENCE_SCRIPT tag,
does not extract reference-script hashes in extract_utxo_tags, and
minibf lacks a /scripts/{script_hash} route.
Implementing these closes the doc → code gap and enables mini-bf to return reference-script info for live UTxOs.

Steps (implementation draft)
Add UTxO tag dimension

Edit dimensions.rs
Add pub const REFERENCE_SCRIPT: TagDimension = "reference_script"; under utxo
Extract & index reference-script hashes (primary change)

Edit delta.rs
Implement extract_reference_script_hash(output: &MultiEraOutput) -> Option<Vec<u8>> (or inline)
Use output.script_ref() + Pallas compute_hash() / original_hash() (prefer Pallas utilities)
Push Tag: Tag::new(utxo::REFERENCE_SCRIPT, reference_script_hash_bytes)
Update CardanoIndexDeltaBuilder::extract_utxo_tags to include the tag
Tests — index builder + state

Add unit test(s) in delta.rs for extract_utxo_tags with script_ref
Add integration test in utxoset.rs (or testing) asserting indexes.utxos_by_tag("reference_script", hash) returns expected TxoRef
minibf API: /scripts/{script_hash}

Add route mapping in lib.rs
Implement handler in scripts.rs:
Query domain.indexes().utxos_by_tag(utxo::REFERENCE_SCRIPT, script_hash_bytes)
If found, load UTxO via domain.state().get_utxos(...) or StateStore and extract script_ref raw/variant
Compute type (PlutusV1/PlutusV2/Native) and serialised_size from script raw CBOR
Return Blockfrost-style model { script_hash, type, serialised_size }
Add minibf integration test to assert endpoint returns expected JSON for a test UTxO with script_ref
Testing & verification

Add test UTxO fixtures (update testing if needed) with script_ref present
Ensure CI checks pass
Docs & housekeeping

Update REFERENCE_SCRIPT_INDEXING.md to mark implemented behavior and limitations (current-UTxO-only)
Add CHANGELOG / commit message

Files & Symbols to change (concrete)
Add dimension
dimensions.rs — add utxo::REFERENCE_SCRIPT
Index extraction
delta.rs — update extract_utxo_tags (and add helper extract_reference_script_hash if desired)
Symbol: CardanoIndexDeltaBuilder::extract_utxo_tags
Tests / fixtures
delta.rs — unit tests
crates/testing/* — add sample UTxO with script_ref
utxoset.rs — index tests that query utxos_by_tag
minibf API
lib.rs — route registration "/scripts/{script_hash}"
scripts.rs — handler implementation + tests
Use existing UtxoOutputModelBuilder::output.script_ref() mapping as reference
Docs
REFERENCE_SCRIPT_INDEXING.md — update status/notes

Verification (how to test)
Unit + integration tests (added above)
CLI / workspace checks:
cargo clippy --workspace --all-targets --all-features
cargo build --workspace --all-targets --all-features
cargo test --workspace --all-features
Manual (dev):
Run minibf with in-memory test data and call GET /scripts/{script_hash}
Verify JSON fields: script_hash, type (plutusV1/plutusV2/native), serialised_size
Decisions & rationale
Use Pallas utilities to compute script hash/type (avoid manual CBOR/hashing) — consistency & correctness.
Compute script metadata on-demand from UTxO (do not persist raw CBOR) — lower storage / faster iteration.
Scope: current UTxOs only for this change (per your selection). Historical/archive lookup is a later enhancement.
Priority & effort estimate
High priority, small-medium effort:
Add dimension + index extraction + unit tests — Small
Medium priority, small effort:
minibf route + handler + integration test — Small
Low priority, follow-up:
Historical archive indexing / storing raw CBOR — Medium (requires more design & reindex)

Edge-cases & blockers
UTxO consumed (not in current StateStore) → endpoint will return 404 (documented behavior).
Type / hash mismatches — use fully-qualified Pallas types to avoid ambiguous types.
If you later want historical coverage, we must add archive tagging + query helpers and reindex.
Next steps (recommended PR task list)
Add utxo::REFERENCE_SCRIPT constant (dimensions.rs) — commit
Implement extract_reference_script_hash + update extract_utxo_tags (delta.rs) — commit + unit tests
Add state/index tests (redb3/testing) to verify utxos_by_tag("reference_script", ...) — commit
Add minibf route + handler + integration test — commit
Update REFERENCE_SCRIPT_INDEXING.md and run full CI — finalize PR
# Hardcoded Network Values in Dolos MiniBF

## Current Hardcoded Values

In `crates/minibf/src/hacks.rs`, the following values are hardcoded:

- **Genesis Hashes:**
  - Mainnet: `5f20df933584822601f9e3f8c024eb5eb252fe8cefb24d1317dc3d432e940ebb`
  - Preprod: `d4b8de7a11d929a323373cbab6c1a9bdc931beffff11db111cf9d57356ee1937`
  - Preview: `83de1d7302569ad56cf9139a41e2e11346d4cb4a31c00142557b6ab3fa550761`

- **Network Magic Mapping:**
  - 764824073 → Mainnet
  - 1 → Preprod
  - 2 → Preview

- **Genesis Block Construction:**
  - Only these networks have custom logic for genesis block API responses.

## Why Are They Hardcoded?

- To provide Blockfrost-compatible API responses for mainnet, preprod, and preview.
- To identify the network and synthesize genesis block data for Blockfrost endpoints.
- Blockfrost clients expect these hashes and block data to match the public networks.

## Problems with Hardcoding

- Custom/private testnets are not supported unless added to the code.
- Users must patch the code to add their own network magic and genesis hash.
- Not flexible for new or experimental networks.

## Possible Solutions (implemented)

The minibf service now supports three resolution mechanisms for identifying a domain's genesis/block identity (in decreasing precedence):

1. Minibf-configured override (`MinibfConfig.hardcoded_network`) — highest precedence
2. The computed Shelley genesis hash that Dolos loads at startup (canonical for this domain)
3. Known public-genesis constants (mainnet / preprod / preview) when they match the computed Shelley hash

The code implements a hybrid approach that keeps instant responses for public networks while enabling full support for custom/private networks via configuration or by using the computed genesis hash.

### Configurable hardcoded networks
- New config type: `HardcodedNetwork { magic: u64, genesis_hash: String }`.
- New `MinibfConfig` field: `hardcoded_network: Option<HardcodedNetwork>` (single mapping).
- Use-case: add a single custom/private testnet mapping (or override a public hash) without recompiling.

Example `dolos.toml` snippet:

```toml
[serve.minibf]
listen_address = "0.0.0.0:8080"

# optional: set a single hardcoded network mapping for minibf
hardcoded_network = { magic = 42, genesis_hash = "012345..." }
```

### Resolution semantics (precise)
- If `hardcoded_network` is configured and its `magic` equals the domain's configured `network_magic`, its `genesis_hash` is used.
- Otherwise the computed Shelley genesis hash (loaded via pallas when Dolos boots) is returned.
- If that computed shelley hash matches one of the known public hashes, we still expose the public constant value for perfect Blockfrost compatibility.
- This ensures:
  - public networks keep exact, known genesis hashes
  - custom/private networks work out-of-the-box via computed hash or `hardcoded_network`

### API behaviour changes
- `/genesis`, `/blocks/{hash}`, and Blockfrost-compatible endpoints use the resolved genesis hash as described above.
- The genesis-block helpers will return the configured/computed genesis hash (not only hardcoded constants).

### Testing
- Unit tests and integration tests were added/updated:
  - `hacks::genesis_hash_prefers_minibf_config` — verifies config override precedence
  - `hacks::genesis_hash_falls_back_to_shelley_hash` — verifies shelley hash fallback
  - `crates/minibf/tests/hardcoded_networks.rs` — router-level tests for `/blocks/{genesis}` and `/blocks/0`

### Migration & compatibility notes
- Existing deployments continue to work unchanged for public networks (mainnet/preprod/preview).
- To support a custom/private network, set `serve.minibf.hardcoded_network` in `dolos.toml` or ensure the genesis files Dolos loads match the network you expect.

## Performance considerations
- Known public networks keep precomputed behavior for Blockfrost compatibility (no additional overhead).
- Custom networks use the computed shelley hash (no runtime heavy work) and only perform archive lookups when necessary (minimal cost).

---

If you want, I can add an example `dolos.toml` to the repository docs (recommended) and add an integration test that boots minibf with a user-provided genesis file.

#!/usr/bin/env bash
# fix_testnet_genesis_for_dolos.sh
# -----------------------------------------------------------------------------
# Make Cardano `testnet` genesis files Dolos-compatible and update `dolos.toml`.
# - Adds `prSteps`/`prMem` (rationals) from `priceSteps`/`priceMemory` if missing
# - Adds `exUnitsMem`/`exUnitsSteps` where `memory`/`steps` exist
# - Converts `committee.threshold` integers/floats -> fraction object
# - Writes absolute genesis paths into `dolos.toml` [genesis]
# - Sets `upstream.peer_address` in `dolos.toml` using the node `port` file
# Backups are created before modifying any file.
# Usage: ./fix_testnet_genesis_for_dolos.sh [TESTNET_DIR] [DOLOS_TOML] [NODE_PORT_FILE]
# Defaults match your workspace (no args required).
# -----------------------------------------------------------------------------
IFS=$'\n\t'

TESTNET_DIR="${1:-/home/adrian/cardano/cardano-src/cardano-node/testnet}"
DOLOS_TOML="${2:-/home/adrian/cardano/dolos/dolos.toml}"
NODE_PORT_FILE="${3:-/home/adrian/cardano/cardano-src/cardano-node/testnet/node-data/node1/port}"

TIMESTAMP=$(date +%Y%m%d_%H%M%S)

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || { echo "Error: required command '$1' not found" >&2; exit 1; }
}

# require jq for JSON editing (no python fallback)
require_cmd jq


float_to_frac() {
  # prints: <numerator> <denominator> for decimal/scientific input (bash only)
  local s; s=$(printf '%s' "$1" | tr -d '[:space:]')

  # handle empty/zero
  if [ -z "$s" ] || [ "$s" = "0" ] || [ "$s" = "0.0" ]; then
    printf "0 1"
    return
  fi

  # capture sign
  local sign=""
  if [[ "$s" == -* ]]; then
    sign='-'
    s="${s#-}"
  fi

  # split exponent (if any)
  local mantissa exp
  if [[ "$s" == *[eE]* ]]; then
    mantissa="${s%%[eE]*}"
    exp="${s#*[eE]}"
  else
    mantissa="$s"
    exp=0
  fi

  # split mantissa into integer and fractional parts
  local intpart fracpart digits d e adj numer denom pow i g
  if [[ "$mantissa" == *.* ]]; then
    intpart="${mantissa%%.*}"
    fracpart="${mantissa#*.}"
  else
    intpart="$mantissa"
    fracpart=""
  fi

  intpart="${intpart:-0}"
  fracpart="${fracpart:-}"
  digits="${intpart}${fracpart}"
  # strip leading zeros
  digits="${digits#${digits%%[!0]*}}"
  [ -z "$digits" ] && digits=0
  d=${#fracpart}

  # normalize exponent
  if [[ "$exp" == +* ]]; then exp="${exp#+}"; fi
  if [[ -z "$exp" ]]; then exp=0; fi
  e=$((exp))
  adj=$((e - d))

  if [ "$adj" -ge 0 ]; then
    pow=1
    for ((i=0;i<adj;i++)); do pow=$((pow * 10)); done
    numer=$((digits * pow))
    denom=1
  else
    pow=1
    for ((i=0;i< -adj;i++)); do pow=$((pow * 10)); done
    numer=$((digits))
    denom=$pow
  fi

  # gcd and reduce
  gcd() { local a=$1 b=$2; while [ $b -ne 0 ]; do local t=$b; b=$((a % b)); a=$t; done; echo $a; }
  g=$(gcd "$numer" "$denom")
  numer=$((numer / g))
  denom=$((denom / g))
  if [ "$sign" = "-" ]; then numer=$(( -numer )); fi
  printf "%s %s" "$numer" "$denom"
} 

patch_alonzo() {
  local f="$1"
  [ -f "$f" ] || { echo "skip: $f (not found)"; return; }
  echo "Patching $f"
  # backup_file removed

    # add prSteps from priceSteps
    if jq -e '.executionPrices.prSteps' "$f" >/dev/null 2>&1; then
      echo "  prSteps OK (exists)"
    else
      if jq -e '.executionPrices.priceSteps' "$f" >/dev/null 2>&1; then
        priceSteps=$(jq -r '.executionPrices.priceSteps' "$f")
        IFS=" " read -r n d <<< "$(float_to_frac "$priceSteps")"
        jq --arg n "$n" --arg d "$d" '.executionPrices.prSteps = {numerator:($n|tonumber), denominator:($d|tonumber)}' "$f" > "$f.tmp" && mv "$f.tmp" "$f"
        echo "  added executionPrices.prSteps = $n/$d"
      fi
    fi

    # add prMem from priceMemory
    if jq -e '.executionPrices.prMem' "$f" >/dev/null 2>&1; then
      echo "  prMem OK (exists)"
    else
      if jq -e '.executionPrices.priceMemory' "$f" >/dev/null 2>&1; then
        priceMem=$(jq -r '.executionPrices.priceMemory' "$f")
        IFS=" " read -r n d <<< "$(float_to_frac "$priceMem")"
        jq --arg n "$n" --arg d "$d" '.executionPrices.prMem = {numerator:($n|tonumber), denominator:($d|tonumber)}' "$f" > "$f.tmp" && mv "$f.tmp" "$f"
        echo "  added executionPrices.prMem = $n/$d"
      fi
    fi

    # propagate exUnits keys if missing
    jq '
      if (.maxBlockExUnits.exUnitsMem? == null) and (.maxBlockExUnits.memory? != null) then .maxBlockExUnits.exUnitsMem = .maxBlockExUnits.memory else . end |
      if (.maxBlockExUnits.exUnitsSteps? == null) and (.maxBlockExUnits.steps? != null) then .maxBlockExUnits.exUnitsSteps = .maxBlockExUnits.steps else . end |
      if (.maxTxExUnits.exUnitsMem? == null) and (.maxTxExUnits.memory? != null) then .maxTxExUnits.exUnitsMem = .maxTxExUnits.memory else . end |
      if (.maxTxExUnits.exUnitsSteps? == null) and (.maxTxExUnits.steps? != null) then .maxTxExUnits.exUnitsSteps = .maxTxExUnits.steps else . end
    ' "$f" > "$f.tmp" && mv "$f.tmp" "$f"
    echo "  ensured exUnits keys in maxBlockExUnits / maxTxExUnits"
}

patch_conway() {
  local f="$1"
  [ -f "$f" ] || { echo "skip: $f (not found)"; return; }
  echo "Patching $f"
  # backup_file removed

    # if committee.threshold exists and is a number -> convert to fraction
    if jq -e '.committee.threshold' "$f" >/dev/null 2>&1; then
      ttype=$(jq -r '.committee.threshold | type' "$f")
      if [ "$ttype" = "number" ]; then
        thr=$(jq -r '.committee.threshold' "$f")
        IFS=" " read -r n d <<< "$(float_to_frac "$thr")"
        jq --arg n "$n" --arg d "$d" '.committee.threshold = {numerator:($n|tonumber), denominator:($d|tonumber)}' "$f" > "$f.tmp" && mv "$f.tmp" "$f"
        echo "  converted committee.threshold -> $n/$d"
      else
        echo "  committee.threshold already non-numeric (type=$ttype), skipped"
      fi
    else
      echo "  committee.threshold missing, skipped"
    fi
}

# TOML: set key in section (replace if present; insert if missing; create section if missing)
# toml_set SECTION KEY VALUE FILE
# VALUE must already be properly quoted for strings (e.g. "..."), or a bare token/number
toml_set() {
  local section="$1"; local key="$2"; local value="$3"; local file="$4"
  # do NOT create backups for toml edits (dolos expects exact filename)
  awk -v SECTION="[$section]" -v KEY="$key" -v VAL="$value" '
    BEGIN{insec=0; done=0}
    /^\[.*\]/{
      if(insec==1 && done==0){ print KEY" = "VAL; done=1 }
      insec = ($0==SECTION)
      print; next
    }
    {
      if(insec==1 && $0 ~ ("^"KEY"[[:space:]]*=")){
        print KEY" = "VAL; done=1; next
      }
      print
    }
    END{
      if(done==0){ if(insec==0) print "\n" SECTION; print KEY" = "VAL }
    }' "$file" > "$file.tmp" && mv "$file.tmp" "$file"
}

# Set a single `serve.minibf.hardcoded_network` table (singular) in the TOML file
# toml_set_hardcoded_network MAGIC GENESIS_HASH FILE
# - Replaces any existing array-based `hardcoded_networks` value and writes a
#   single-object `hardcoded_network = { magic = ..., genesis_hash = "..." }`.
# - Creates the [serve.minibf] section if missing.
toml_set_hardcoded_network() {
  local magic="$1"; local ghash="$2"; local file="$3"

  # Remove old array-based `hardcoded_networks = [ ... ]` if present (naive but effective)
  if grep -qE "^\s*hardcoded_networks\s*=" "$file"; then
    awk 'BEGIN{skip=0} /^\s*hardcoded_networks\s*=\s*\[/ {skip=1; next} skip==1 && /^\s*\]/ {skip=0; next} skip==0{print}' "$file" > "$file.tmp" && mv "$file.tmp" "$file"
    echo "  removed existing hardcoded_networks array (will set singular hardcoded_network)"
  fi

  # Use toml_set to write the single-object mapping under [serve.minibf]
  toml_set "serve.minibf" "hardcoded_network" "{ magic = $magic, genesis_hash = \"$ghash\" }" "$file"
  echo "  wrote serve.minibf.hardcoded_network -> magic=$magic"
}


# Main ------------------------------------------------------------------------

# Ensure testnet directory exists
if [ ! -d "$TESTNET_DIR" ]; then
  echo "ERROR: testnet dir not found: $TESTNET_DIR" >&2
  exit 2
fi

ALONZO_JSON="$TESTNET_DIR/alonzo-genesis.json"
CONWAY_JSON="$TESTNET_DIR/conway-genesis.json"
SHELLEY_JSON="$TESTNET_DIR/shelley-genesis.json"
BYRON_JSON="$TESTNET_DIR/byron-genesis.json"

# Patch genesis files
patch_alonzo "$ALONZO_JSON"
patch_conway "$CONWAY_JSON"

echo "\nUpdating $DOLOS_TOML genesis paths and upstream.peer_address"

# Read port from node port file
if [ -f "$NODE_PORT_FILE" ]; then
  port_raw=$(tr -d '[:space:]' < "$NODE_PORT_FILE")
  if [[ "$port_raw" == *":"* ]]; then
    peer_addr="$port_raw"
  else
    peer_addr="localhost:$port_raw"
  fi
  echo "  read peer port -> $peer_addr"
else
  echo "WARN: node port file not found: $NODE_PORT_FILE" >&2
  peer_addr="localhost:44625"
  echo "  falling back to $peer_addr"
fi

# Set genesis file paths in dolos.toml
if [ -f "$DOLOS_TOML" ]; then
  toml_set genesis byron_path "\"$BYRON_JSON\"" "$DOLOS_TOML"
  toml_set genesis shelley_path "\"$SHELLEY_JSON\"" "$DOLOS_TOML"
  toml_set genesis alonzo_path  "\"$ALONZO_JSON\"" "$DOLOS_TOML"
  toml_set genesis conway_path  "\"$CONWAY_JSON\"" "$DOLOS_TOML"

  toml_set upstream peer_address "\"$peer_addr\"" "$DOLOS_TOML"

  # --- set upstream.network_magic from shelley genesis (if present) ---
  if jq -e '.networkMagic' "$SHELLEY_JSON" >/dev/null 2>&1; then
    magic_val=$(jq -r '.networkMagic' "$SHELLEY_JSON")
    if [ -n "$magic_val" ]; then
      toml_set upstream network_magic $magic_val "$DOLOS_TOML"
      echo "  set upstream.network_magic -> $magic_val"
    fi
  else
    echo "  WARN: networkMagic not found in $SHELLEY_JSON; skipping upstream.network_magic update"
  fi

  # --- read Shelley genesis hash from testnet/configuration.yaml and set minibf hardcoded_network ---
  CONFIG_YAML="$TESTNET_DIR/configuration.yaml"
  if [ -f "$CONFIG_YAML" ] && jq -e '.ShelleyGenesisHash' "$CONFIG_YAML" >/dev/null 2>&1; then
    shelley_hash=$(jq -r '.ShelleyGenesisHash' "$CONFIG_YAML")
    echo "  found ShelleyGenesisHash in $CONFIG_YAML -> $shelley_hash"

    # ensure we have a magic value (prefer the one read above; fall back to any value in dolos.toml)
    if [ -z "$magic_val" ]; then
      magic_val=$(awk -F'=' '/^\s*network_magic\s*=/{gsub(/\"/,"",$2); print $2; exit}' "$DOLOS_TOML" | tr -d '[:space:]')
    fi

    if [ -n "$magic_val" ]; then
      toml_set_hardcoded_network "$magic_val" "$shelley_hash" "$DOLOS_TOML"
    else
      echo "  WARN: could not determine network magic; skipping hardcoded_network entry"
    fi
  else
    echo "  WARN: $CONFIG_YAML missing or ShelleyGenesisHash not present; skipping hardcoded_network update"
  fi

  echo "  updated $DOLOS_TOML"
else
  echo "ERROR: dolos.toml not found at $DOLOS_TOML" >&2
  exit 3
fi

cat <<EOF
Done! Genesis files patched and $DOLOS_TOML updated.
EOF

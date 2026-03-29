#!/usr/bin/env bash

set -euo pipefail

ignored=0
nocapture=0
filter=""
config_file=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --ignored)
      ignored=1
      shift
      ;;
    --nocapture)
      nocapture=1
      shift
      ;;
    -c|--config)
      if [[ $# -lt 2 ]]; then
        echo "missing value after $1" >&2
        exit 1
      fi
      config_file="$2"
      shift 2
      ;;
    --config=*)
      config_file="${1#*=}"
      shift
      ;;
    *)
      filter="$1"
      shift
      ;;
  esac
done

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(cd "$script_dir/.." && pwd)"
config_file="${config_file:-$project_root/hive.example.toml}"

toml_value() {
  local file="$1"
  local section="$2"
  local key="$3"
  awk -v section="$section" -v key="$key" '
    /^[[:space:]]*#/ || /^[[:space:]]*$/ { next }
    /^[[:space:]]*\[/ {
      current = $0
      gsub(/^[[:space:]]*\[/, "", current)
      gsub(/\][[:space:]]*$/, "", current)
      in_section = (current == section)
      next
    }
    in_section {
      pattern = "^[[:space:]]*" key "[[:space:]]*="
      if ($0 ~ pattern) {
        value = $0
        sub(/^[^=]*=[[:space:]]*/, "", value)
        gsub(/^[[:space:]]+|[[:space:]]+$/, "", value)
        gsub(/^"|"$/, "", value)
        gsub(/^'\''|'\''$/, "", value)
        print value
        exit
      }
    }
  ' "$file"
}

"$script_dir/start-db.sh" --config "$config_file"

export DATABASE_URL="$(toml_value "$config_file" database test_url)"
export CRV_RUN_HIVE_DB_TESTS=1

args=(test -p crv-hive --lib --tests)
if [[ -n "$filter" ]]; then
  args+=("$filter")
fi

args+=(--)
if [[ "$ignored" -eq 1 ]]; then
  args+=(--ignored)
fi
if [[ "$nocapture" -eq 1 ]]; then
  args+=(--nocapture)
fi

cd "$project_root"
cargo "${args[@]}"
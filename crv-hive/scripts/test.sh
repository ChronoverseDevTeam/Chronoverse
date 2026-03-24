#!/usr/bin/env bash

set -euo pipefail

ignored=0
nocapture=0
filter=""

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
    *)
      filter="$1"
      shift
      ;;
  esac
done

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(cd "$script_dir/.." && pwd)"

"$script_dir/start-db.sh"

if [[ -f "$project_root/.env" ]]; then
  set -a
  # shellcheck disable=SC1091
  source "$project_root/.env"
  set +a
fi

export TEST_DATABASE_URL="${TEST_DATABASE_URL:-postgres://crv:crv@127.0.0.1:55432/chronoverse_test}"
export DATABASE_URL="$TEST_DATABASE_URL"
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
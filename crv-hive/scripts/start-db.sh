#!/usr/bin/env bash

set -euo pipefail

config_file=""

while [[ $# -gt 0 ]]; do
  case "$1" in
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
      echo "unsupported argument: $1" >&2
      exit 1
      ;;
  esac
done

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(cd "$script_dir/.." && pwd)"
compose_file="$project_root/docker-compose.yml"
config_file="${config_file:-$project_root/hive.example.toml}"

if [[ ! -f "$compose_file" ]]; then
  echo "docker-compose.yml not found at $compose_file" >&2
  exit 1
fi

if [[ ! -f "$config_file" ]]; then
  echo "config file not found at $config_file" >&2
  exit 1
fi

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

parse_postgres_url() {
  local url="$1"
  local rest="${url#postgres://}"
  if [[ "$rest" == "$url" ]]; then
    echo "unsupported database url scheme: $url" >&2
    exit 1
  fi

  local credentials="${rest%@*}"
  local host_and_db="${rest#*@}"
  local user="${credentials%%:*}"
  local password="${credentials#*:}"
  local host_port="${host_and_db%%/*}"
  local database="${host_and_db#*/}"
  local host="${host_port%%:*}"
  local port="${host_port#*:}"

  if [[ "$host_port" == "$host" ]]; then
    port="5432"
  fi

  if [[ -z "$user" || -z "$password" || -z "$database" ]]; then
    echo "database url must include username, password, and database name: $url" >&2
    exit 1
  fi

  printf '%s\n%s\n%s\n%s\n%s\n' "$user" "$password" "$host" "$port" "$database"
}

database_url="$(toml_value "$config_file" database url)"
test_database_url="$(toml_value "$config_file" database test_url)"

mapfile -t database_parts < <(parse_postgres_url "$database_url")
mapfile -t test_database_parts < <(parse_postgres_url "$test_database_url")

export CRV_POSTGRES_USER="${database_parts[0]}"
export CRV_POSTGRES_PASSWORD="${database_parts[1]}"
export CRV_POSTGRES_PORT="${database_parts[3]}"
export CRV_POSTGRES_DB="${database_parts[4]}"
export CRV_POSTGRES_TEST_DB="${test_database_parts[4]}"

if [[ "${database_parts[0]}" != "${test_database_parts[0]}" || "${database_parts[1]}" != "${test_database_parts[1]}" || "${database_parts[2]}" != "${test_database_parts[2]}" || "${database_parts[3]}" != "${test_database_parts[3]}" ]]; then
  echo "[database].url and [database].test_url must use the same host, port, username, and password" >&2
  exit 1
fi

if command -v docker-compose >/dev/null 2>&1; then
  compose_cmd=(docker-compose -f "$compose_file")
elif command -v docker >/dev/null 2>&1; then
  compose_cmd=(docker compose -f "$compose_file")
else
  echo "Neither docker-compose nor docker is available in PATH." >&2
  exit 1
fi

"${compose_cmd[@]}" up -d postgres >/dev/null

for _ in {1..30}; do
  status="$(docker inspect -f '{{.State.Health.Status}}' crv-hive-postgres 2>/dev/null || true)"
  if [[ "$status" == "healthy" ]]; then
    echo "Postgres is healthy."
    exit 0
  fi
  sleep 2
done

echo "Postgres did not become healthy in time." >&2
exit 1
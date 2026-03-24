#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(cd "$script_dir/.." && pwd)"
compose_file="$project_root/docker-compose.yml"
env_file="$project_root/.env"

if [[ ! -f "$compose_file" ]]; then
  echo "docker-compose.yml not found at $compose_file" >&2
  exit 1
fi

if [[ -f "$env_file" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "$env_file"
  set +a
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
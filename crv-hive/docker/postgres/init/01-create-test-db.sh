#!/bin/sh
set -eu

test_db="${POSTGRES_TEST_DB:-chronoverse_test}"
escaped_identifier=$(printf '%s' "$test_db" | sed 's/"/""/g')
escaped_literal=$(printf '%s' "$test_db" | sed "s/'/''/g")

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname postgres <<EOF
SELECT 'CREATE DATABASE "${escaped_identifier}"'
WHERE NOT EXISTS (
    SELECT FROM pg_database WHERE datname = '${escaped_literal}'
)\gexec
EOF
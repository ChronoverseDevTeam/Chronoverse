# crv-hive

`crv-hive` is developed as an independent service.  For local work, keep the
Rust process on the host and run only Postgres in Docker.

## Local database environment

The local database stack is defined in [docker-compose.yml](docker-compose.yml).
It starts a single Postgres 16 container with:

- host port `55432`
- dev database `chronoverse_dev`
- test database `chronoverse_test`
- persistent named volume `crv-hive-postgres-data`

The test database is created by
[docker/postgres/init/01-create-test-db.sql](docker/postgres/init/01-create-test-db.sql)
when the container is initialised for the first time.

## Environment contract

The tracked file [.env](.env) is the active local environment contract for
`crv-hive`. Update it directly if you need different local database values.

Current shared variables:

- `DATABASE_URL`: primary database connection string
- `TEST_DATABASE_URL`: connection string used by local database-backed tests
- `RUST_LOG`: default tracing filter
- `DB_MAX_CONNECTIONS`: reserved for the upcoming pooled database integration

The file [hive.example.toml](hive.example.toml) records the config shape planned
for upcoming database wiring.  The current executable does not read it yet, but
the values mirror the environment contract so local setup and future CI stay aligned.

## Scripts

PowerShell:

```powershell
./scripts/start-db.ps1
./scripts/test.ps1
./scripts/test.ps1 --Ignored
./scripts/test.ps1 --Ignored --NoCapture
```

POSIX shell:

```bash
./scripts/start-db.sh
./scripts/test.sh
./scripts/test.sh --ignored
./scripts/test.sh --ignored --nocapture
```

Behavior:

- `start-db` loads `.env`, starts the local Postgres container, and waits until it is healthy
- `test` loads `.env`, exports `DATABASE_URL=$TEST_DATABASE_URL`,
	sets `CRV_RUN_HIVE_DB_TESTS=1`, and runs `cargo test -p crv-hive --lib --tests`
- `--Ignored` / `--ignored` is intended for future database-backed tests that are
	explicitly gated behind ignored test cases

## Manual commands

If you prefer not to use the helper scripts:

```bash
docker-compose -f crv-hive/docker-compose.yml up -d postgres
cargo test -p crv-hive --lib --tests
DATABASE_URL=postgres://crv:crv@127.0.0.1:55432/chronoverse_test cargo test -p crv-hive --lib --tests -- --ignored
```

## Notes

- The current `crv-hive` tests are mostly transport-level and do not require
	Postgres yet; this setup is in place so database-backed tests can be added
	without changing the local workflow again.
- Existing doctests in the iroh modules currently fail independently of the
	database setup, so the helper scripts intentionally focus on library and
	integration tests.
- The same environment contract is intended to be reused later in GitHub Actions,
	but CI wiring is intentionally deferred for now.


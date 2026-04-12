# crv-hive

`crv-hive` is developed as an independent service.  For local work, keep the
Rust process on the host and run only Postgres in Docker.

## Local database environment

The local database stack is defined in [docker-compose.yml](docker-compose.yml).
It starts a single Postgres 16 container with:

- host port `5432`
- dev database `chronoverse_dev`
- test database `chronoverse_test`
- persistent named volume `crv-hive-postgres-data`

The test database is created by
[docker/postgres/init/01-create-test-db.sql](docker/postgres/init/01-create-test-db.sql)
when the container is initialised for the first time.

## Configuration

`crv-hive` now reads TOML configuration through a single config module.

- Default startup looks for `hive.toml` in the current working directory
- You can override the file with `-c <path>` or `--config <path>`
- Missing config items fall back to the defaults defined in `src/crv2/config/mod.rs`
- Runtime configuration is no longer read from environment variables
- Local Docker and test helper scripts default to `hive.example.toml`

Example:

```bash
cargo run -p crv-hive -- -c crv-hive/hive.example.toml
```

The sample schema is recorded in [hive.example.toml](hive.example.toml).

The file [hive.example.toml](hive.example.toml) records the supported TOML schema.

## Scripts

PowerShell:

```powershell
./scripts/start-db.ps1
./scripts/start-db.ps1 -Config ./hive.example.toml
./scripts/test.ps1
./scripts/test.ps1 -Config ./hive.example.toml
./scripts/test.ps1 --Ignored
./scripts/test.ps1 --Ignored --NoCapture
```

POSIX shell:

```bash
./scripts/start-db.sh
./scripts/start-db.sh --config ./hive.example.toml
./scripts/test.sh
./scripts/test.sh --config ./hive.example.toml
./scripts/test.sh --ignored
./scripts/test.sh --ignored --nocapture
```

Behavior:

- `start-db` reads `[database]` from `hive.example.toml`, starts the local Postgres container, and waits until it is healthy
- `test` reads `[database].test_url` from the same TOML file, starts Postgres, and runs `cargo test -p crv-hive --lib --tests`
- `--Ignored` / `--ignored` is intended for future database-backed tests that are
	explicitly gated behind ignored test cases

## Manual commands

If you prefer not to use the helper scripts:

```bash
docker-compose -f crv-hive/docker-compose.yml up -d postgres
cargo test -p crv-hive --lib --tests
cargo test -p crv-hive --lib --tests -- --ignored
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


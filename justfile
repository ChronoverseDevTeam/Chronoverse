# justfile — task runner for Chronoverse development
# Install: cargo install just

default:
    @just --list

# Build all crates
build:
    cargo build --workspace

# Build with release optimizations
release:
    cargo build --workspace --release

# Run all tests
test:
    cargo test --workspace

# Run tests with output
test-verbose:
    cargo test --workspace -- --nocapture

# Run clippy lints
lint:
    cargo clippy --workspace -- -D warnings

# Format code
fmt:
    cargo fmt --all

# Check formatting without changes
fmt-check:
    cargo fmt --all -- --check

# Create the PostgreSQL database
db-create:
    createdb chronoverse || echo "Database may already exist"

# Run database migrations
db-migrate:
    cargo run -p crv-core

# Start crv-core server
run-server:
    cargo run -p crv-core

# Watch for changes and restart server
dev-server:
    cargo watch -x "run -p crv-core"

# Clean build artifacts
clean:
    cargo clean

# Full CI check
ci: fmt-check lint test build

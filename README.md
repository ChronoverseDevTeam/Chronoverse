# Chronoverse — Open-Source Perforce-Compatible Version Control

**Chronoverse** is a centralized version control system compatible with the Perforce/Helix Core workflow. It consists of:

- **`crv-core`** — The central server (REST API + PostgreSQL + file storage)
- **`crv`** — The client application (CLI mode + REST daemon mode)

## Quick Start

### Prerequisites
- Rust 1.80+
- PostgreSQL 16+ (or Docker)
- `cargo install just` (optional, for task runner)

### Local Development

```bash
# Clone
git clone https://github.com/chronoverse/chronoverse
cd chronoverse

# Start PostgreSQL (via Docker)
docker compose up -d postgres

# Or use a local PostgreSQL
createdb chronoverse

# Configure
cp .env.example .env
# Edit .env with your DATABASE_URL

# Build & run server
cargo run -p crv-core

# In another terminal, use the CLI
cargo run -p crv -- login -u admin
cargo run -p crv -- client myws
cargo run -p crv -- add //depot/main/hello.txt
cargo run -p crv -- submit -d "initial commit"
cargo run -p crv -- sync
```

### Docker Deployment

```bash
docker compose up -d
# Server available at http://localhost:3000
```

## Architecture

```
┌──────────────┐     REST      ┌─────────────────────┐
│   crv (CLI)   │───►:3000────►│      crv-core        │
│ p4-compatible │              │  Axum + PostgreSQL   │
└──────────────┘              │  + File Depot        │
                              └─────────────────────┘
┌──────────────┐     proxy          │
│  crv daemon   │───►:4000─────────┘
│ local :4000   │
│ SQLite db.have│
└──────────────┘
```

## CLI Commands (p4-compatible)

| Command | Description |
|---------|-------------|
| `crv login -u <user>` | Authenticate |
| `crv logout` | End session |
| `crv client <name>` | Create/switch workspace |
| `crv add <files>` | Open files for add |
| `crv edit <files>` | Open files for edit |
| `crv delete <files>` | Open files for delete |
| `crv revert <files>` | Revert opened files |
| `crv submit -d "msg"` | Submit changelist |
| `crv sync [-f] [-n]` | Sync workspace |
| `crv changes` | List changelists |
| `crv filelog <files>` | Revision history |
| `crv fstat <files>` | File status |
| `crv opened` | List opened files |
| `crv lock/unlock <files>` | File locking |
| `crv integrate -s <src> -t <tgt>` | Branch/merge |
| `crv branch <name>` | Branch spec |
| `crv label <name>` | Label spec |
| `crv labelsync <name> <spec>` | Tag revisions |
| `crv stream <name>` | Stream spec |
| `crv users/groups` | User/group management |
| `crv protects` | List ACL |
| `crv daemon [-p <port>]` | Start local daemon |

## REST API

The server exposes a REST API at `http://localhost:3000/api/v1/`.

### Authentication
- `POST /auth/login` — Get ticket
- `POST /auth/logout` — Revoke ticket

### Users & Groups
- `GET/POST /users` — List/create users
- `GET/PUT/DELETE /users/:id` — Manage user
- `GET/POST /groups` — List/create groups
- `GET/PUT/DELETE /groups/:name` — Manage group

### Clients (Workspaces)
- `GET/POST /clients` — List/create clients
- `GET/PUT/DELETE /clients/:name` — Manage client

### File Operations
- `POST /clients/:client/files/{add,edit,delete,revert}` — Open files
- `GET /clients/:client/files/opened` — List opened files
- `GET /clients/:client/sync` — Compute sync diff
- `POST /clients/:client/sync/confirm` — Confirm sync

### Changelists
- `GET/POST /clients/:client/changes` — List/create changelists
- `GET /clients/:client/changes/:id` — Get changelist
- `POST /clients/:client/changes/:id/submit` — Submit changelist

### File Content
- `GET /files/{path}/content?rev=N` — Download file
- `POST /files/{path}/content` — Upload file
- `GET /files/{path}/fstat` — File status
- `GET /files/{path}/filelog?max=N` — Revision history

### Locks
- `POST /clients/:client/files/lock` — Lock files
- `POST /files/unlock` — Unlock files
- `GET /files/locks` — List locks

### Integration
- `POST /integrate` — Branch/merge files
- `GET /integrations?path=...` — Integration history

### Labels
- `GET/POST /labels` — List/create labels
- `DELETE /labels/:name` — Delete label
- `POST /labels/:name/sync` — Tag revisions
- `GET /labels/:name/revisions` — List tagged revisions

### Protections (ACL)
- `GET/POST /protections` — List/add ACL entries
- `DELETE /protections/:id` — Remove ACL entry

### Streams
- `GET/POST /streams` — List/create streams
- `DELETE /streams/:name` — Delete stream

### Branches
- `GET/POST /branches` — List/create branch specs
- `DELETE /branches/:name` — Delete branch spec

### Daemon (local only)
- `GET /daemon/status` — Daemon health
- `GET /daemon/workspace` — Workspace info

## Project Structure

```
chronoverse/
├── Cargo.toml              # Workspace root
├── docker-compose.yml      # Docker deployment
├── Dockerfile              # Server container
├── justfile                # Task runner
├── .env.example            # Config template
├── crates/
│   ├── crv-shared/         # Shared types, errors, protocol
│   ├── crv-core/           # Server application
│   │   ├── src/
│   │   │   ├── api/        # REST handlers (auth, users, files, etc.)
│   │   │   ├── auth/       # Ticket, password, middleware
│   │   │   ├── db/         # Connection pool, migrations
│   │   │   ├── engine/     # Business logic (submit, sync, lock, integrate, label, protect)
│   │   │   ├── storage/    # Depot file I/O
│   │   │   └── server/     # Router, status endpoints
│   │   └── migrations/     # SQLx migrations
│   └── crv/                # Client application
│       └── src/
│           ├── api/        # REST client to crv-core
│           ├── cli/        # CLI commands, config, output formatting
│           ├── daemon/     # Local daemon server + proxy
│           └── workspace/  # Local SQLite db.have
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `CRV_HOST` | `0.0.0.0` | Server bind address |
| `CRV_PORT` | `3000` | Server port |
| `DATABASE_URL` | — | PostgreSQL connection (required) |
| `CRV_TICKET_SECRET` | `change-me...` | HMAC key for auth tickets |
| `CRV_DEPOT_ROOT` | `./data/depot` | File storage root |
| `CRV_TICKET_TTL_HOURS` | `24` | Ticket expiry |
| `CRV_SERVER_URL` | `http://localhost:3000` | Server URL for CLI |
| `CRV_CLIENT` | — | Default client/workspace name |

## License

MIT

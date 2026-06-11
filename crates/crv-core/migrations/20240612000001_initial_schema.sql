-- Initial migration: core tables for Chronoverse
-- This migration creates the foundational tables needed for authentication,
-- user management, and client/workspace management.

-- ── Users ────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS users (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL UNIQUE,
    email       TEXT NOT NULL UNIQUE,
    full_name   TEXT,
    password_hash TEXT NOT NULL,
    user_type   TEXT NOT NULL DEFAULT 'standard'
                CHECK (user_type IN ('standard', 'operator', 'service')),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── Auth Tickets ──────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS auth_tickets (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    ticket_hash TEXT NOT NULL UNIQUE,
    expires_at  TIMESTAMPTZ NOT NULL,
    client_addr TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_auth_tickets_user ON auth_tickets(user_id);
CREATE INDEX idx_auth_tickets_hash ON auth_tickets(ticket_hash);

-- ── Groups ────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS groups (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL UNIQUE,
    description TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS group_members (
    group_id    UUID NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (group_id, user_id)
);

-- ── Clients (Workspaces) ──────────────────────────────────────────
CREATE TABLE IF NOT EXISTS clients (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL UNIQUE,
    owner_id    UUID NOT NULL REFERENCES users(id),
    root        TEXT NOT NULL,
    view_json   JSONB NOT NULL DEFAULT '[]',
    options_json JSONB NOT NULL DEFAULT '{}',
    host        TEXT,
    stream_id   UUID,
    description TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_clients_owner ON clients(owner_id);

-- ── Changelists ───────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS changes (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    number      BIGSERIAL NOT NULL,
    client_id   UUID NOT NULL REFERENCES clients(id),
    user_id     UUID NOT NULL REFERENCES users(id),
    description TEXT NOT NULL DEFAULT '',
    status      TEXT NOT NULL DEFAULT 'pending'
                CHECK (status IN ('pending', 'submitted', 'shelved')),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_changes_number ON changes(number);
CREATE INDEX idx_changes_client ON changes(client_id);
CREATE INDEX idx_changes_user ON changes(user_id);

-- ── File Revisions ────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS file_revisions (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    depot_path  TEXT NOT NULL,
    revision    INTEGER NOT NULL,
    change_id   UUID REFERENCES changes(id),
    action      TEXT NOT NULL
                CHECK (action IN ('add', 'edit', 'delete', 'branch', 'integrate', 'move_add', 'move_delete')),
    file_type   TEXT NOT NULL DEFAULT 'text',
    digest      TEXT NOT NULL,
    size        BIGINT NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (depot_path, revision)
);

CREATE INDEX idx_file_revisions_path ON file_revisions(depot_path);
CREATE INDEX idx_file_revisions_change ON file_revisions(change_id);

-- ── Working (Opened Files) ────────────────────────────────────────
CREATE TABLE IF NOT EXISTS working (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    client_id       UUID NOT NULL REFERENCES clients(id) ON DELETE CASCADE,
    depot_path      TEXT NOT NULL,
    action          TEXT NOT NULL
                    CHECK (action IN ('add', 'edit', 'delete', 'branch', 'integrate', 'move_add', 'move_delete')),
    change_id       UUID REFERENCES changes(id),
    base_revision   INTEGER,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (client_id, depot_path)
);

CREATE INDEX idx_working_client ON working(client_id);

-- ── Have (Client Synced State) ────────────────────────────────────
CREATE TABLE IF NOT EXISTS have (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    client_id   UUID NOT NULL REFERENCES clients(id) ON DELETE CASCADE,
    depot_path  TEXT NOT NULL,
    revision    INTEGER NOT NULL,
    digest      TEXT NOT NULL,
    file_size   BIGINT NOT NULL DEFAULT 0,
    sync_time   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (client_id, depot_path)
);

CREATE INDEX idx_have_client ON have(client_id);

-- ── Locks ─────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS locks (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    depot_path  TEXT NOT NULL UNIQUE,
    client_id   UUID NOT NULL REFERENCES clients(id),
    user_id     UUID NOT NULL REFERENCES users(id),
    lock_type   TEXT NOT NULL DEFAULT 'exclusive'
                CHECK (lock_type IN ('exclusive', 'shared')),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_locks_path ON locks(depot_path);

-- ── Integrations ──────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS integrations (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_path         TEXT NOT NULL,
    source_start_rev    INTEGER NOT NULL,
    source_end_rev      INTEGER NOT NULL,
    target_path         TEXT NOT NULL,
    target_start_rev    INTEGER NOT NULL,
    target_end_rev      INTEGER NOT NULL,
    action              TEXT NOT NULL
                        CHECK (action IN ('branch_from', 'merge_from', 'copy_from', 'delete_from', 'ignore')),
    change_id           UUID REFERENCES changes(id),
    user_id             UUID NOT NULL REFERENCES users(id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_integrations_source ON integrations(source_path);
CREATE INDEX idx_integrations_target ON integrations(target_path);

-- ── Labels ────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS labels (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL UNIQUE,
    owner_id    UUID NOT NULL REFERENCES users(id),
    description TEXT,
    view_json   JSONB DEFAULT '[]',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS label_revisions (
    label_id    UUID NOT NULL REFERENCES labels(id) ON DELETE CASCADE,
    depot_path  TEXT NOT NULL,
    revision    INTEGER NOT NULL,
    PRIMARY KEY (label_id, depot_path)
);

-- ── Protections ───────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS protections (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    perm_type           TEXT NOT NULL
                        CHECK (perm_type IN ('read', 'write', 'open', 'admin', 'super', 'review', 'owner')),
    perm_level          TEXT NOT NULL DEFAULT 'user'
                        CHECK (perm_level IN ('user', 'group', 'any')),
    entity_type         TEXT NOT NULL
                        CHECK (entity_type IN ('user', 'group')),
    entity_name         TEXT NOT NULL,
    depot_path_pattern  TEXT NOT NULL,
    "order"             INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_protections_perm ON protections(perm_type, depot_path_pattern);

-- ── Streams ───────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS streams (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL UNIQUE,
    parent_id   UUID REFERENCES streams(id),
    stream_type TEXT NOT NULL DEFAULT 'development'
                CHECK (stream_type IN ('mainline', 'release', 'development', 'task', 'virtual')),
    view_json   JSONB NOT NULL DEFAULT '[]',
    options_json JSONB NOT NULL DEFAULT '{}',
    owner_id    UUID NOT NULL REFERENCES users(id),
    description TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── Branches ──────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS branches (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL UNIQUE,
    owner_id    UUID NOT NULL REFERENCES users(id),
    description TEXT,
    view_json   JSONB NOT NULL DEFAULT '[]',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

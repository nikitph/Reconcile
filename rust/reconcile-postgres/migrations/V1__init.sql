-- Reconcile v0.1.0 — Initial schema
-- Supports: resources, events (append-only), audit log, snapshots

CREATE TABLE resources (
    id UUID PRIMARY KEY,
    resource_type TEXT NOT NULL,
    state TEXT NOT NULL,
    desired_state TEXT,
    data JSONB NOT NULL DEFAULT '{}',
    version INT NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE events (
    id UUID PRIMARY KEY,
    event_offset BIGSERIAL UNIQUE NOT NULL,
    event_type TEXT NOT NULL,
    resource_id UUID NOT NULL REFERENCES resources(id),
    payload JSONB NOT NULL DEFAULT '{}',
    actor TEXT NOT NULL,
    authority_level TEXT NOT NULL,
    created_ts TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE audit_log (
    id UUID PRIMARY KEY,
    resource_type TEXT NOT NULL,
    resource_id UUID NOT NULL REFERENCES resources(id),
    actor TEXT NOT NULL,
    role TEXT NOT NULL,
    authority_level TEXT NOT NULL,
    previous_state TEXT NOT NULL,
    new_state TEXT NOT NULL,
    policies_evaluated JSONB NOT NULL DEFAULT '[]',
    invariants_checked JSONB NOT NULL DEFAULT '[]',
    created_ts TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE snapshots (
    id UUID PRIMARY KEY,
    resource_id UUID NOT NULL REFERENCES resources(id),
    state TEXT NOT NULL,
    data JSONB NOT NULL DEFAULT '{}',
    version INT NOT NULL,
    event_offset BIGINT NOT NULL,
    created_ts TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_resources_type_state ON resources (resource_type, state);
CREATE INDEX idx_events_resource ON events (resource_id, event_offset);
CREATE INDEX idx_audit_resource ON audit_log (resource_id, created_ts);
CREATE INDEX idx_snapshots_resource ON snapshots (resource_id, event_offset DESC);

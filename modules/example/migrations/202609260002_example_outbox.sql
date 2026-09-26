CREATE TABLE outbox_events (
    event_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    event_type text NOT NULL,
    event_version smallint NOT NULL CHECK (event_version > 0),
    aggregate_type text NOT NULL,
    aggregate_id bigint NOT NULL,
    aggregate_sequence bigint NOT NULL CHECK (aggregate_sequence > 0),
    occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    payload_schema_version smallint NOT NULL CHECK (payload_schema_version > 0),
    payload jsonb NOT NULL,
    available_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    attempt_count integer NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    claimed_by text,
    claim_until timestamptz,
    delivered_at timestamptz,
    UNIQUE (aggregate_type, aggregate_id, aggregate_sequence)
);

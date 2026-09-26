ALTER TABLE outbox_events
    ADD COLUMN dead_lettered_at timestamptz,
    ADD COLUMN dead_letter_reason text,
    ADD CONSTRAINT outbox_events_dead_letter_state_check
        CHECK ((dead_lettered_at IS NULL) = (dead_letter_reason IS NULL));

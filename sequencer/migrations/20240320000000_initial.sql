CREATE TABLE IF NOT EXISTS events (
    tx_hash TEXT PRIMARY KEY,
    status JSONB NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_events_status ON events ((status->>'type'));
CREATE INDEX IF NOT EXISTS idx_events_created_at ON events (created_at); 
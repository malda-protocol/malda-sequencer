-- Drop existing table and type if they exist
DROP TABLE IF EXISTS events;
DROP TYPE IF EXISTS event_status;

-- Create enum type for status
CREATE TYPE event_status AS ENUM (
    'Received',
    'Processed',
    'IncludedInBatch',
    'ProofRequested',
    'ProofReceived',
    'BatchSubmitStarted',
    'BatchSubmitted',
    'Failed'
);

CREATE TABLE events (
    tx_hash TEXT PRIMARY KEY,
    event_type TEXT,
    src_chain_id INTEGER,
    dst_chain_id INTEGER,
    msg_sender TEXT,
    amount TEXT,
    status event_status NOT NULL,
    received_at TIMESTAMP WITH TIME ZONE,
    processed_at TIMESTAMP WITH TIME ZONE,
    included_in_batch_at TIMESTAMP WITH TIME ZONE,
    proof_requested_at TIMESTAMP WITH TIME ZONE,
    proof_received_at TIMESTAMP WITH TIME ZONE,
    batch_submit_started_at TIMESTAMP WITH TIME ZONE,
    batch_submitted_at TIMESTAMP WITH TIME ZONE,
    batch_id TEXT,
    proof_data BYTEA,
    proof_index INTEGER,
    batch_tx_hash TEXT,
    error TEXT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_events_status ON events(status);
CREATE INDEX idx_events_batch_id ON events(batch_id);
CREATE INDEX idx_events_created_at ON events(created_at);
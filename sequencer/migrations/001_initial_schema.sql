DROP TABLE IF EXISTS events;

-- Create enum type for status
CREATE TYPE event_status AS ENUM (
    'Received',
    'Processed',
    'IncludedInBatch',
    'ProofRequested',
    'ProofReceived',
    'BatchSubmitted',
    'BatchIncluded',
    'BatchFailed',
    'TxProcessSuccess',
    'TxProcessFail',
    'Failed'
);

CREATE TABLE events (
    tx_hash TEXT PRIMARY KEY,
    status event_status NOT NULL,
    event_type TEXT,
    src_chain_id INTEGER,
    dst_chain_id INTEGER,
    msg_sender TEXT,
    amount TEXT,
    target_function TEXT,
    market TEXT,
    journal_index INTEGER,
    journal BYTEA,
    seal BYTEA,
    batch_tx_hash TEXT,
    received_at TIMESTAMP WITH TIME ZONE,
    processed_at TIMESTAMP WITH TIME ZONE,
    proof_requested_at TIMESTAMP WITH TIME ZONE,
    proof_received_at TIMESTAMP WITH TIME ZONE,
    batch_submitted_at TIMESTAMP WITH TIME ZONE,
    batch_included_at TIMESTAMP WITH TIME ZONE,
    tx_finished_at TIMESTAMP WITH TIME ZONE,
    resubmitted INTEGER,
    error TEXT
);

CREATE INDEX idx_events_status ON events(status);
DROP TABLE IF EXISTS events;
DROP TABLE IF EXISTS finished_events;
DROP TABLE IF EXISTS chain_batch_sync;
DROP TABLE IF EXISTS sync_timestamps;
DROP TABLE IF EXISTS volume_flow;
DROP TABLE IF EXISTS rpc_monitor;
DROP TABLE IF EXISTS node_status;

DROP TYPE IF EXISTS event_status;

-- Create enum type for status
CREATE TYPE event_status AS ENUM (
    'Received',
    'Processed',
    'IncludedInBatch',
    'ReorgSecurityDelay',
    'ReadyToRequestProof',
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
    received_block_timestamp INTEGER,
    received_at_block INTEGER,
    should_request_proof_at_block INTEGER,
    journal_index INTEGER,
    journal BYTEA,
    seal BYTEA,
    bonsai_uuid TEXT,
    stark_time INTEGER,
    snark_time INTEGER,
    total_cycles INTEGER,
    batch_tx_hash TEXT,
    received_at TIMESTAMP WITH TIME ZONE,
    proof_requested_at TIMESTAMP WITH TIME ZONE,
    proof_received_at TIMESTAMP WITH TIME ZONE,
    batch_submitted_at TIMESTAMP WITH TIME ZONE,
    batch_included_at TIMESTAMP WITH TIME ZONE,
    tx_finished_at TIMESTAMP WITH TIME ZONE,
    finished_block_timestamp INTEGER,
    resubmitted INTEGER,
    error TEXT
);

CREATE TABLE IF NOT EXISTS finished_events (
    tx_hash TEXT PRIMARY KEY,
    status event_status NOT NULL,
    event_type TEXT,
    src_chain_id INTEGER,
    dst_chain_id INTEGER,
    msg_sender TEXT,
    amount TEXT,
    target_function TEXT,
    market TEXT,
    received_block_timestamp INTEGER,
    received_at_block INTEGER,
    should_request_proof_at_block INTEGER,
    journal_index INTEGER,
    bonsai_uuid TEXT,
    stark_time INTEGER,
    snark_time INTEGER,
    total_cycles INTEGER,
    batch_tx_hash TEXT,
    received_at TIMESTAMP WITH TIME ZONE,
    proof_requested_at TIMESTAMP WITH TIME ZONE,
    proof_received_at TIMESTAMP WITH TIME ZONE,
    batch_submitted_at TIMESTAMP WITH TIME ZONE,
    batch_included_at TIMESTAMP WITH TIME ZONE,
    tx_finished_at TIMESTAMP WITH TIME ZONE,
    finished_block_timestamp INTEGER,
    resubmitted INTEGER,
    error TEXT
);

CREATE TABLE IF NOT EXISTS archive_events (
    tx_hash TEXT PRIMARY KEY,
    status event_status NOT NULL,
    event_type TEXT,
    src_chain_id INTEGER,
    dst_chain_id INTEGER,
    msg_sender TEXT,
    amount TEXT,
    target_function TEXT,
    market TEXT,
    received_block_timestamp INTEGER,
    received_at_block INTEGER,
    should_request_proof_at_block INTEGER,
    journal_index INTEGER,
    bonsai_uuid TEXT,
    stark_time INTEGER,
    snark_time INTEGER,
    total_cycles INTEGER,
    batch_tx_hash TEXT,
    received_at TIMESTAMP WITH TIME ZONE,
    proof_requested_at TIMESTAMP WITH TIME ZONE,
    proof_received_at TIMESTAMP WITH TIME ZONE,
    batch_submitted_at TIMESTAMP WITH TIME ZONE,
    batch_included_at TIMESTAMP WITH TIME ZONE,
    tx_finished_at TIMESTAMP WITH TIME ZONE,
    finished_block_timestamp INTEGER,
    resubmitted INTEGER,
    error TEXT
);

CREATE TABLE sync_timestamps (
    id INTEGER PRIMARY KEY DEFAULT 1,
    last_proof_requested_at TIMESTAMP WITH TIME ZONE,
    CONSTRAINT single_row CHECK (id = 1)
);

-- Insert initial row
INSERT INTO sync_timestamps (id, last_proof_requested_at)
VALUES (1, NULL)
ON CONFLICT (id) DO NOTHING;

CREATE TABLE chain_batch_sync (
    dst_chain_id INTEGER PRIMARY KEY,
    last_batch_submitted_at TIMESTAMP WITH TIME ZONE
);

CREATE TABLE volume_flow (
    chain_id INTEGER PRIMARY KEY,
    last_reset TIMESTAMP WITH TIME ZONE,
    dollar_value INTEGER
);

-- Insert initial rows for each chain
INSERT INTO volume_flow (chain_id, last_reset, dollar_value)
VALUES 
    (59141, NOW(), 0),  -- Linea Sepolia
    (11155111, NOW(), 0),  -- Ethereum Sepolia
    (11155420, NOW(), 0)  -- Optimism Sepolia
ON CONFLICT (chain_id) DO NOTHING;

-- Create table to track node status and failures
CREATE TABLE node_status (
    id SERIAL PRIMARY KEY,
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    chain_id INTEGER NOT NULL,
    primary_url TEXT NOT NULL,
    fallback_url TEXT NOT NULL,
    reason TEXT NOT NULL
);

CREATE INDEX idx_events_status ON events(status);
CREATE INDEX finished_events_status_idx ON finished_events(status);
CREATE INDEX idx_events_dst_chain_id ON events(dst_chain_id);
CREATE INDEX idx_node_status_timestamp ON node_status(timestamp);

-- Grant SELECT permissions to frontend user
GRANT SELECT ON events TO "Frontend";
GRANT SELECT ON finished_events TO "Frontend";

GRANT SELECT ON events TO "Analytics";
GRANT SELECT ON finished_events TO "Analytics";
GRANT SELECT ON archive_events TO "Analytics";
GRANT SELECT ON node_status TO "Analytics";

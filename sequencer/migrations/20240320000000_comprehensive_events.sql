CREATE TABLE IF NOT EXISTS events (
    tx_hash TEXT PRIMARY KEY,
    
    -- Event Details
    event_type TEXT,
    src_chain_id INTEGER,
    dst_chain_id INTEGER,
    msg_sender TEXT,
    amount TEXT,  -- Using TEXT to store U256 values
    
    -- Status Timestamps (NULL if not reached yet)
    received_at TIMESTAMP WITH TIME ZONE,
    processed_at TIMESTAMP WITH TIME ZONE,
    included_in_batch_at TIMESTAMP WITH TIME ZONE,
    proof_requested_at TIMESTAMP WITH TIME ZONE,
    proof_received_at TIMESTAMP WITH TIME ZONE,
    batch_submit_started_at TIMESTAMP WITH TIME ZONE,
    batch_submitted_at TIMESTAMP WITH TIME ZONE,
    
    -- Batch and Proof Details
    batch_id TEXT,
    proof_data BYTEA,
    proof_index INTEGER,
    batch_tx_hash TEXT,
    
    -- Current Status
    current_status TEXT NOT NULL,
    error TEXT,
    
    -- Metadata
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- Indexes for common queries
CREATE INDEX IF NOT EXISTS idx_events_status ON events(current_status);
CREATE INDEX IF NOT EXISTS idx_events_batch_id ON events(batch_id);
CREATE INDEX IF NOT EXISTS idx_events_created_at ON events(created_at); 
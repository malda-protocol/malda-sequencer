-- Migration to add liquidatee and collateral fields to events table
-- These fields will store the address of the user being liquidated and the collateral token address

-- Add liquidatee field to events table
ALTER TABLE events ADD COLUMN IF NOT EXISTS liquidatee TEXT;

-- Add collateral field to events table  
ALTER TABLE events ADD COLUMN IF NOT EXISTS collateral TEXT;

-- Add liquidatee field to finished_events table
ALTER TABLE finished_events ADD COLUMN IF NOT EXISTS liquidatee TEXT;

-- Add collateral field to finished_events table
ALTER TABLE finished_events ADD COLUMN IF NOT EXISTS collateral TEXT;

-- Add liquidatee field to archive_events table
ALTER TABLE archive_events ADD COLUMN IF NOT EXISTS liquidatee TEXT;

-- Add collateral field to archive_events table
ALTER TABLE archive_events ADD COLUMN IF NOT EXISTS collateral TEXT;

-- Create indexes for better query performance on the new fields
CREATE INDEX IF NOT EXISTS idx_events_liquidatee ON events(liquidatee);
CREATE INDEX IF NOT EXISTS idx_events_collateral ON events(collateral);
CREATE INDEX IF NOT EXISTS idx_finished_events_liquidatee ON finished_events(liquidatee);
CREATE INDEX IF NOT EXISTS idx_finished_events_collateral ON finished_events(collateral); 
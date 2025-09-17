#!/bin/bash
set -e  # Exit on error

# Get the directory where the script is located
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
# Get the sequencer directory (parent of scripts directory)
SEQUENCER_DIR="$( cd "$SCRIPT_DIR/.." && pwd )"

echo "Script directory: $SCRIPT_DIR"
echo "Sequencer directory: $SEQUENCER_DIR"

# Load environment variables from .env file
if [ -f "$SEQUENCER_DIR/.env" ]; then
    echo "Loading environment variables from .env file..."
    source "$SEQUENCER_DIR/.env"
else
    echo "Error: .env file not found at $SEQUENCER_DIR/.env"
    exit 1
fi

# Function to clean a database
clean_database() {
    local db_url="$1"
    local db_name="$2"
    
    echo "=== Cleaning $db_name ==="
    
    # Check if database is accessible
    echo "Checking $db_name connection..."
    if ! psql "$db_url" -c "\q" > /dev/null 2>&1; then
        echo "Warning: Cannot connect to $db_name. Skipping..."
        return 1
    fi
    echo "$db_name connection successful!"
    
    # Terminate all active connections to prevent hanging
    echo "Terminating all active database connections..."
    psql "$db_url" -c "
        SELECT pg_terminate_backend(pid) 
        FROM pg_stat_activity 
        WHERE datname = current_database() 
          AND pid <> pg_backend_pid()
          AND state IN ('active', 'idle in transaction');
    " > /dev/null 2>&1 || true
    
    # Wait a moment for connections to close
    sleep 2
    
    # Drop all tables and types in the correct order
    echo "Dropping all tables and types..."
    
    # First drop tables that depend on the event_status type
    psql "$db_url" -c "DROP TABLE IF EXISTS events CASCADE;" > /dev/null 2>&1 || true
    psql "$db_url" -c "DROP TABLE IF EXISTS finished_events CASCADE;" > /dev/null 2>&1 || true
    psql "$db_url" -c "DROP TABLE IF EXISTS archive_events CASCADE;" > /dev/null 2>&1 || true
    
    # Drop other tables
    psql "$db_url" -c "DROP TABLE IF EXISTS sync_timestamps CASCADE;" > /dev/null 2>&1 || true
    psql "$db_url" -c "DROP TABLE IF EXISTS chain_batch_sync CASCADE;" > /dev/null 2>&1 || true
    psql "$db_url" -c "DROP TABLE IF EXISTS volume_flow CASCADE;" > /dev/null 2>&1 || true
    psql "$db_url" -c "DROP TABLE IF EXISTS node_status CASCADE;" > /dev/null 2>&1 || true
    psql "$db_url" -c "DROP TABLE IF EXISTS boundless_users CASCADE;" > /dev/null 2>&1 || true
    
    # Drop the event_status type
    psql "$db_url" -c "DROP TYPE IF EXISTS event_status CASCADE;" > /dev/null 2>&1 || true
    
    # Finally drop the migrations table
    psql "$db_url" -c "DROP TABLE IF EXISTS _sqlx_migrations CASCADE;" > /dev/null 2>&1 || true
    
    # Verify all tables are dropped
    echo "Verifying cleanup..."
    REMAINING_TABLES=$(psql "$db_url" -t -c "SELECT tablename FROM pg_tables WHERE schemaname = 'public';" 2>/dev/null | grep -v '^$' | wc -l)
    if [ "$REMAINING_TABLES" -eq 0 ]; then
        echo "✅ $db_name cleanup completed successfully!"
    else
        echo "⚠️  Warning: Some tables may still exist in $db_name"
        psql "$db_url" -c "SELECT tablename FROM pg_tables WHERE schemaname = 'public';" 2>/dev/null || true
    fi
    echo ""
}

# Stop the sequencer if it's running
if [ -f "$SEQUENCER_DIR/logs/sequencer.pid" ]; then
    OLD_PID=$(cat "$SEQUENCER_DIR/logs/sequencer.pid")
    if ps -p $OLD_PID > /dev/null 2>&1; then
        echo "Stopping sequencer process (PID: $OLD_PID)..."
        kill $OLD_PID
        sleep 5
        if ps -p $OLD_PID > /dev/null 2>&1; then
            echo "Force killing sequencer process..."
            kill -9 $OLD_PID
        fi
        echo "Sequencer process stopped."
        echo ""
    fi
fi

# Clean main database
clean_database "$DATABASE_URL" "Main Database"

# Clean fallback database if FALLBACK_DATABASE_URL is set
if [ -n "$DATABASE_URL_FALLBACK" ]; then
    clean_database "$DATABASE_URL_FALLBACK" "Fallback Database"
else
    echo "DATABASE_URL_FALLBACK not set, skipping fallback database cleanup."
fi

echo "All database cleanup operations completed!" 
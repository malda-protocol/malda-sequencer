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
    
    # Drop tables and migrations
    echo "Dropping $db_name tables and migrations..."
    psql "$db_url" -c "DROP TABLE IF EXISTS _sqlx_migrations CASCADE;"
    psql "$db_url" -c "DROP TABLE IF EXISTS events CASCADE;"
    psql "$db_url" -c "DROP TABLE IF EXISTS finished_events CASCADE;"
    psql "$db_url" -c "DROP TABLE IF EXISTS sync_timestamps CASCADE;"
    psql "$db_url" -c "DROP TABLE IF EXISTS chain_batch_sync CASCADE;"
    psql "$db_url" -c "DROP TYPE IF EXISTS event_status CASCADE;"
    psql "$db_url" -c "DROP TABLE IF EXISTS node_status CASCADE;"
    echo "$db_name cleanup completed successfully!"
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
if [ -n "$FALLBACK_DATABASE_URL" ]; then
    clean_database "$FALLBACK_DATABASE_URL" "Fallback Database"
else
    echo "FALLBACK_DATABASE_URL not set, skipping fallback database cleanup."
fi

echo "All database cleanup operations completed!" 
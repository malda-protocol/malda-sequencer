#!/bin/bash
set -e  # Exit on error

# Get the directory where the script is located
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
# Get the sequencer directory (parent of scripts directory)
SEQUENCER_DIR="$( cd "$SCRIPT_DIR/.." && pwd )"

echo "Script directory: $SCRIPT_DIR"
echo "Sequencer directory: $SEQUENCER_DIR"

# Set database URL
export DATABASE_URL="postgres://app:NVHEojL0TwU2xOhhAegvhNKGx2OvwsjJyL9JfNNIE5u4OX8Zo5JZARmEBfGWo7r7@dal.database.lsh.io:31187/app"

# Check if database is accessible
echo "Checking database connection..."
if ! psql "$DATABASE_URL" -c "\q" > /dev/null 2>&1; then
    echo "Error: Cannot connect to database. Please check your connection settings."
    exit 1
fi
echo "Database connection successful!"

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
    fi
fi

# Drop tables and migrations
echo "Dropping database tables and migrations..."
psql "$DATABASE_URL" -c "DROP TABLE IF EXISTS _sqlx_migrations CASCADE;"
psql "$DATABASE_URL" -c "DROP TABLE IF EXISTS events CASCADE;"
psql "$DATABASE_URL" -c "DROP TABLE IF EXISTS finished_events CASCADE;"
psql "$DATABASE_URL" -c "DROP TABLE IF EXISTS sync_timestamps CASCADE;"
psql "$DATABASE_URL" -c "DROP TABLE IF EXISTS chain_batch_sync CASCADE;"
psql "$DATABASE_URL" -c "DROP TYPE IF EXISTS event_status CASCADE;"
psql "$DATABASE_URL" -c "DROP TABLE IF EXISTS node_status CASCADE;"
echo "Database cleanup completed successfully!" 
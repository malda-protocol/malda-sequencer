#!/bin/bash
set -e  # Exit on error

# Get the directory where the script is located
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
# Get the sequencer directory (parent of scripts directory)
SEQUENCER_DIR="$( cd "$SCRIPT_DIR/.." && pwd )"

echo "Script directory: $SCRIPT_DIR"
echo "Sequencer directory: $SEQUENCER_DIR"

# Create logs directory if it doesn't exist
mkdir -p "$SEQUENCER_DIR/logs"

# Set Bonsai API environment variables
export BONSAI_API_KEY="tpXcM8jXP08AKwfZ9M7ryaXVDX3u0Zaa4Z2Cpzhe"
export BONSAI_API_URL="https://api.bonsai.xyz/"

# Set database URL
export DATABASE_URL="postgres://app:8Ixx4N5OYuDR6lQGZfDgxxF9cUnl2yW6jYbFsZRMi69ZrCnhYIgU7HuQCsgKhWyl@dal.database.lsh.io:30166/app"

# Check if database is accessible
echo "Checking database connection..."
if ! psql "$DATABASE_URL" -c "\q" > /dev/null 2>&1; then
    echo "Error: Cannot connect to database. Please check your connection settings."
    exit 1
fi
echo "Database connection successful!"

# Check if sequencer is already running
if [ -f "$SEQUENCER_DIR/logs/sequencer.pid" ]; then
    OLD_PID=$(cat "$SEQUENCER_DIR/logs/sequencer.pid")
    if ps -p $OLD_PID > /dev/null 2>&1; then
        echo "Stopping existing sequencer process (PID: $OLD_PID)..."
        kill $OLD_PID
        sleep 5
        if ps -p $OLD_PID > /dev/null 2>&1; then
            echo "Force killing existing sequencer process..."
            kill -9 $OLD_PID
        fi
        echo "Existing sequencer process stopped."
    fi
fi

# Run migrations using sqlx from the sequencer directory
echo "Running database migrations..."
cd "$SEQUENCER_DIR"
sqlx migrate run --database-url "${DATABASE_URL}"

# Rename existing log file with timestamp if it exists
if [ -f "$SEQUENCER_DIR/logs/sequencer.log" ]; then
    TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
    mv "$SEQUENCER_DIR/logs/sequencer.log" "$SEQUENCER_DIR/logs/sequencer_${TIMESTAMP}.log"
    echo "Renamed old log file to sequencer_${TIMESTAMP}.log"
fi

# Start the sequencer, redirecting output to the log file directly
cd "$SEQUENCER_DIR"
RUST_LOG=debug cargo run --release --bin sequencer > "$SEQUENCER_DIR/logs/sequencer.log" 2>&1 &
SEQUENCER_PID=$!

# Save the process ID
echo $SEQUENCER_PID > "$SEQUENCER_DIR/logs/sequencer.pid"
echo "Sequencer started with PID: $SEQUENCER_PID"
echo "Logs are being written to: $SEQUENCER_DIR/logs/sequencer.log (rotated by logrotate)"

# Wait a moment and check if the process is still running
sleep 2
if ps -p $SEQUENCER_PID > /dev/null 2>&1; then
    echo "Sequencer is running successfully!"
else
    echo "Error: Sequencer failed to start. Check the logs for details."
    exit 1
fi 
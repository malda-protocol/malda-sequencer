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
export BONSAI_API_KEY="SrSzB6P4SFaWv7WAK12ph5K6aL6dXs4S1a0XMif5"
export BONSAI_API_URL="https://api.bonsai.xyz/"

# Set database URL
export DATABASE_URL="postgres://doadmin:AVNS_G5U-F8YEsMY2G4odL39@db-postgresql-lon1-66182-do-user-15988403-0.k.db.ondigitalocean.com:25060/defaultdb?sslmode=require"

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

# Start the sequencer in the background with verbose logging
echo "Starting sequencer..."
cd "$SEQUENCER_DIR"
RUST_LOG=debug nohup cargo run --release --bin sequencer > "$SEQUENCER_DIR/logs/sequencer.log" 2>&1 &

# Save the process ID
echo $! > "$SEQUENCER_DIR/logs/sequencer.pid"
echo "Sequencer started with PID: $!"
echo "Logs are being written to: $SEQUENCER_DIR/logs/sequencer.log"

# Wait a moment and check if the process is still running
sleep 2
if ps -p $(cat "$SEQUENCER_DIR/logs/sequencer.pid") > /dev/null 2>&1; then
    echo "Sequencer is running successfully!"
else
    echo "Error: Sequencer failed to start. Check the logs for details."
    exit 1
fi 
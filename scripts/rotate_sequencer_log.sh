#!/bin/bash
set -e

# Get the directory where the script is located
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
SEQUENCER_DIR="$( cd "$SCRIPT_DIR/.." && pwd )"
LOG_FILE="$SEQUENCER_DIR/logs/sequencer.log"
PID_FILE="$SEQUENCER_DIR/logs/sequencer_logrotate.pid"

# Write this script's PID to the pid file
echo $$ > "$PID_FILE"

echo "Log rotation process started with PID: $$"
echo "Script dir: $SCRIPT_DIR"
echo "Sequencer dir: $SEQUENCER_DIR"
echo "Log file: $LOG_FILE"
echo "PID file: $PID_FILE"

if [ ! -f "$LOG_FILE" ]; then
    echo "WARNING: Log file $LOG_FILE does not exist at startup. Will wait for it to be created."
fi

while true; do
    TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
    ROTATED_LOG="$SEQUENCER_DIR/logs/sequencer_${TIMESTAMP}.log"
    if [ -f "$LOG_FILE" ]; then
        LOG_SIZE=$(stat --format=%s "$LOG_FILE")
        if [ "$LOG_SIZE" -gt 0 ]; then
            tail -c "$LOG_SIZE" "$LOG_FILE" > "$ROTATED_LOG" && echo "[$(date)] Saved $LOG_SIZE bytes from $LOG_FILE to $ROTATED_LOG" || echo "[$(date)] ERROR: Failed to save log content"
            : > "$LOG_FILE" && echo "[$(date)] Truncated $LOG_FILE" || echo "[$(date)] ERROR: Failed to truncate $LOG_FILE"
        else
            echo "[$(date)] $LOG_FILE is empty, skipping rotation."
        fi
    else
        echo "[$(date)] Log file $LOG_FILE does not exist, skipping rotation."
    fi
    sleep 21600  # 6 hours
    # Check if the parent sequencer process is still running (optional)
done 
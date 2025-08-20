#!/bin/bash
set -e

# Get the directory where the script is located
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
SEQUENCER_DIR="$( cd "$SCRIPT_DIR/.." && pwd )"

echo "Malda Sequencer Deployment Script"
echo "================================="

# Function to display usage
usage() {
    echo "Usage: $0 [testnet|mainnet] [--clean]"
    echo ""
    echo "Options:"
    echo "  testnet    Deploy to testnet environment"
    echo "  mainnet    Deploy to mainnet environment"
    echo "  --clean    Clean build before deployment"
    echo ""
    echo "Examples:"
    echo "  $0 testnet           # Deploy to testnet"
    echo "  $0 mainnet --clean   # Clean build and deploy to mainnet"
    exit 1
}

# Parse command line arguments
ENVIRONMENT=""
CLEAN_BUILD=false

while [[ $# -gt 0 ]]; do
    case $1 in
        testnet|mainnet)
            ENVIRONMENT="$1"
            shift
            ;;
        --clean)
            CLEAN_BUILD=true
            shift
            ;;
        -h|--help)
            usage
            ;;
        *)
            echo "Unknown option: $1"
            usage
            ;;
    esac
done

# Validate environment
if [[ -z "$ENVIRONMENT" ]]; then
    echo "Error: Environment must be specified (testnet or mainnet)"
    usage
fi

echo "Deploying to $ENVIRONMENT environment..."

# Set environment variable
export ENVIRONMENT="$ENVIRONMENT"

# Load appropriate .env file
if [[ -f "$SEQUENCER_DIR/.env.$ENVIRONMENT" ]]; then
    echo "Loading environment-specific configuration from .env.$ENVIRONMENT"
    export $(cat "$SEQUENCER_DIR/.env.$ENVIRONMENT" | grep -v '^#' | xargs)
else
    echo "Warning: No .env.$ENVIRONMENT file found, using default .env"
    if [[ -f "$SEQUENCER_DIR/.env" ]]; then
        export $(cat "$SEQUENCER_DIR/.env" | grep -v '^#' | xargs)
    else
        echo "Error: No .env file found"
        exit 1
    fi
fi

# Verify environment variable is set
if [[ "$ENVIRONMENT" != "$(echo $ENVIRONMENT | tr '[:upper:]' '[:lower:]')" ]]; then
    echo "Error: ENVIRONMENT variable mismatch"
    exit 1
fi

echo "Environment: $ENVIRONMENT"
echo "Database URL: ${DATABASE_URL:0:20}..."
if [[ -n "$DATABASE_URL_FALLBACK" ]]; then
    echo "Fallback Database URL: ${DATABASE_URL_FALLBACK:0:20}..."
fi
echo "Sequencer Address: $SEQUENCER_ADDRESS"

# Check if database is accessible
echo "Checking database connection..."
DATABASE_CONNECTED=false

# Try primary database first
if [[ -n "$DATABASE_URL" ]]; then
    echo "Testing primary database connection..."
    if timeout 5 psql "$DATABASE_URL" -c "\q" > /dev/null 2>&1; then
        echo "✅ Primary database connection successful!"
        DATABASE_CONNECTED=true
        PRIMARY_DB_WORKING=true
    else
        echo "❌ Primary database connection failed."
        PRIMARY_DB_WORKING=false
    fi
fi

# Try fallback database if primary failed
if [[ "$DATABASE_CONNECTED" == false && -n "$DATABASE_URL_FALLBACK" ]]; then
    echo "Testing fallback database connection..."
    if timeout 5 psql "$DATABASE_URL_FALLBACK" -c "\q" > /dev/null 2>&1; then
        echo "✅ Fallback database connection successful!"
        DATABASE_CONNECTED=true
        # Store the original primary URL for the application
        export ORIGINAL_DATABASE_URL="$DATABASE_URL"
        # Use fallback URL for migrations but keep original for app
        export MIGRATION_DATABASE_URL="$DATABASE_URL_FALLBACK"
        echo "Using fallback database for migrations."
    else
        echo "❌ Fallback database connection failed."
    fi
fi

# Check if we have at least one working database
if [[ "$DATABASE_CONNECTED" == false ]]; then
    echo "Error: Cannot connect to any database. Please check your connection settings."
    echo "Primary URL: ${DATABASE_URL:0:30}..."
    if [[ -n "$DATABASE_URL_FALLBACK" ]]; then
        echo "Fallback URL: ${DATABASE_URL_FALLBACK:0:30}..."
    fi
    exit 1
fi

# Clean build if requested
if [[ "$CLEAN_BUILD" == true ]]; then
    echo "Performing clean build..."
    cd "$SEQUENCER_DIR"
    cargo clean
fi

# Run database migrations
echo "Running database migrations..."
cd "$SEQUENCER_DIR"

MIGRATION_SUCCESS=false

# Try primary database first (only if we know it's working)
if [[ -n "$DATABASE_URL" && "$PRIMARY_DB_WORKING" == true ]]; then
    echo "Running migrations on primary database..."
    if sqlx migrate run --database-url "${DATABASE_URL}" > /dev/null 2>&1; then
        echo "✅ Migrations successful on primary database!"
        MIGRATION_SUCCESS=true
    else
        echo "❌ Migrations failed on primary database."
    fi
fi

# Try fallback database if primary failed or wasn't working
if [[ "$MIGRATION_SUCCESS" == false && -n "$DATABASE_URL_FALLBACK" ]]; then
    echo "Running migrations on fallback database..."
    if sqlx migrate run --database-url "${DATABASE_URL_FALLBACK}" > /dev/null 2>&1; then
        echo "✅ Migrations successful on fallback database!"
        MIGRATION_SUCCESS=true
    else
        echo "❌ Migrations failed on fallback database."
    fi
fi

# Check if migrations succeeded on at least one database
if [[ "$MIGRATION_SUCCESS" == false ]]; then
    echo "Error: Migrations failed on all databases. Please check your database setup."
    exit 1
fi

# Build the project
echo "Building sequencer..."
cd "$SEQUENCER_DIR"
cargo build --release

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

# Create logs directory if it doesn't exist
mkdir -p "$SEQUENCER_DIR/logs"

# Rename existing log file with timestamp if it exists
if [ -f "$SEQUENCER_DIR/logs/sequencer.log" ]; then
    TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
    mv "$SEQUENCER_DIR/logs/sequencer.log" "$SEQUENCER_DIR/logs/sequencer_${TIMESTAMP}.log"
    echo "Renamed old log file to sequencer_${TIMESTAMP}.log"
fi

# Start the sequencer
echo "Starting sequencer in $ENVIRONMENT mode..."
cd "$SEQUENCER_DIR"
RUST_LOG=info cargo run --release --bin sequencer > "$SEQUENCER_DIR/logs/sequencer.log" 2>&1 &
SEQUENCER_PID=$!

# Save the process ID
echo $SEQUENCER_PID > "$SEQUENCER_DIR/logs/sequencer.pid"
echo "Sequencer started with PID: $SEQUENCER_PID"
echo "Logs are being written to: $SEQUENCER_DIR/logs/sequencer.log"

# Wait a moment and check if the process is still running
sleep 3
if ps -p $SEQUENCER_PID > /dev/null 2>&1; then
    echo "✅ Sequencer is running successfully in $ENVIRONMENT mode!"
    echo ""
    
    # Extract and display configuration summary from log file
    echo "Configuration Summary from Sequencer:"
    echo "====================================="
    if [ -f "$SEQUENCER_DIR/logs/sequencer.log" ]; then
        # Extract the configuration summary section from the log
        awk '/================ Sequencer Configuration Summary ================/,/================================================================/' "$SEQUENCER_DIR/logs/sequencer.log" | head -n -1 | tail -n +2
    else
        echo "Log file not found yet, configuration summary will appear in logs."
    fi
    echo ""
    
    echo "Deployment Summary:"
    echo "  Environment: $ENVIRONMENT"
    echo "  Process ID: $SEQUENCER_PID"
    echo "  Log File: $SEQUENCER_DIR/logs/sequencer.log"
    if [[ "$DATABASE_CONNECTED" == true ]]; then
        if [[ "$DATABASE_URL" == "$DATABASE_URL_FALLBACK" ]]; then
            echo "  Database: Fallback (${DATABASE_URL:0:30}...)"
        else
            echo "  Database: Primary (${DATABASE_URL:0:30}...)"
        fi
    fi
    echo ""
    echo "To stop the sequencer:"
    echo "  kill $SEQUENCER_PID"
    echo ""
    echo "To view logs:"
    echo "  tail -f $SEQUENCER_DIR/logs/sequencer.log"
else
    echo "❌ Error: Sequencer failed to start. Check the logs for details."
    echo "Log file: $SEQUENCER_DIR/logs/sequencer.log"
    exit 1
fi 
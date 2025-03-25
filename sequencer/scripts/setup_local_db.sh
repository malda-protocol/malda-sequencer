#!/bin/bash
set -e  # Exit on error

# Get the directory where the script is located
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
# Get the sequencer directory (parent of scripts directory)
SEQUENCER_DIR="$( cd "$SCRIPT_DIR/.." && pwd )"

echo "Script directory: $SCRIPT_DIR"
echo "Sequencer directory: $SEQUENCER_DIR"

# Stop and remove existing container
docker stop sequencer-db || true
docker rm sequencer-db || true

# Start PostgreSQL container
docker run -d \
  --name sequencer-db \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=sequencer \
  -p 5432:5432 \
  postgres:14

# Wait for PostgreSQL to be ready
echo "Waiting for PostgreSQL to be ready..."
until docker exec sequencer-db pg_isready; do
  echo "Database not ready, waiting..."
  sleep 1
done

# Install sqlx-cli if not already installed
if ! command -v sqlx &> /dev/null; then
    echo "Installing sqlx-cli..."
    cargo install sqlx-cli
fi

# Set database URL
export DATABASE_URL="postgres://postgres:postgres@localhost:5432/sequencer"

# Run migrations using sqlx from the sequencer directory
echo "Running database migrations..."
cd "$SEQUENCER_DIR"
sqlx migrate run --database-url "${DATABASE_URL}"

# Verify the table was created
echo "Verifying table creation..."
docker exec sequencer-db psql -U postgres -d sequencer -c "\dt events"

# Start the sequencer (if needed)
echo "Starting sequencer..."
cd "$SEQUENCER_DIR"
# nohup cargo run --release --bin sequencer > /dev/null 2>&1 & 
RUST_LOG=info cargo run --release --bin sequencer
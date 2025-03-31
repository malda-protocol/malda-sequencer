#!/bin/bash
set -e  # Exit on error

# Get the directory where the script is located
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
# Get the sequencer directory (parent of scripts directory)
SEQUENCER_DIR="$( cd "$SCRIPT_DIR/.." && pwd )"

echo "Script directory: $SCRIPT_DIR"
echo "Sequencer directory: $SEQUENCER_DIR"

# Set Bonsai API environment variables
export BONSAI_API_KEY="SrSzB6P4SFaWv7WAK12ph5K6aL6dXs4S1a0XMif5"
export BONSAI_API_URL="https://api.bonsai.xyz/"

# Install sqlx-cli if not already installed
if ! command -v sqlx &> /dev/null; then
    echo "Installing sqlx-cli..."
    cargo install sqlx-cli
fi

# Set database URL
export DATABASE_URL="postgres://doadmin:AVNS_G5U-F8YEsMY2G4odL39@db-postgresql-lon1-66182-do-user-15988403-0.k.db.ondigitalocean.com:25060/defaultdb?sslmode=require"

# Run migrations using sqlx from the sequencer directory
echo "Running database migrations..."
cd "$SEQUENCER_DIR"
sqlx migrate run --database-url "${DATABASE_URL}"

# Verify the table was created
echo "Verifying table creation..."
psql "${DATABASE_URL}" -c "\dt events"

echo "Database setup complete!"
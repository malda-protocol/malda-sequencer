#!/bin/bash
docker stop sequencer-db
docker rm sequencer-db
# Start PostgreSQL container
docker run -d \
  --name sequencer-db \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=sequencer \
  -p 5432:5432 \
  postgres:14

# Wait for PostgreSQL to start
echo "Waiting for PostgreSQL to start..."
sleep 5

# Run migrations
echo "Running database migrations..."
cargo run --bin sequencer 
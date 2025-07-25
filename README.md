# Malda Sequencer

A high-performance blockchain sequencer that processes cross-chain events, generates Zero-Knowledge (ZK) proofs, and manages transaction submissions across multiple Layer 1 and Layer 2 networks.

## Table of Contents

- [Overview](#overview)
- [Architecture](#architecture)
- [Prerequisites](#prerequisites)
- [Installation](#installation)
- [Configuration](#configuration)
- [Database Setup](#database-setup)
- [Environment Variables](#environment-variables)
- [Deployment](#deployment)
- [Development](#development)
- [Monitoring](#monitoring)
- [Troubleshooting](#troubleshooting)

## Overview

The Malda Sequencer is a sophisticated blockchain infrastructure component that:

- **Monitors blockchain events** across multiple chains (Ethereum, Linea, Optimism, Base)
- **Generates ZK proofs** using RISC0 framework for cross-chain operations
- **Manages transaction submissions** with gas optimization and retry logic
- **Provides fault tolerance** with fallback providers and parallel processing
- **Supports both testnet and mainnet** environments

### Key Features

- **Multi-Chain Support**: Ethereum, Linea, Optimism, Base (mainnet and testnet)
- **ZK Proof Generation**: Real and dummy proof modes for development/production
- **Event Processing**: Monitors and processes cross-chain events
- **Transaction Management**: Automated transaction submission with gas optimization
- **Database Integration**: PostgreSQL for event tracking and state management
- **Provider Fallback**: Automatic failover between primary and backup RPC endpoints
- **Parallel Processing**: Independent chain processing for fault isolation

## Architecture

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   Event Listener│    │  Proof Generator│    │Transaction Mgr  │
│                 │    │                 │    │                 │
│ • Multi-chain   │    │ • ZK Proof Gen  │    │ • Gas Estimation│
│ • Event Filtering│   │ • Boundless/SDK │    │ • Tx Submission │
│ • WebSocket     │    │ • Dummy Mode    │    │ • Retry Logic   │
└─────────────────┘    └─────────────────┘    └─────────────────┘
         │                       │                       │
         └───────────────────────┼───────────────────────┘
                                 │
                    ┌─────────────────┐
                    │    Database     │
                    │                 │
                    │ • PostgreSQL    │
                    │ • Event Tracking│
                    │ • State Mgmt    │
                    └─────────────────┘
```

### Component Overview

1. **Event Listener**: Monitors blockchain events across all configured chains
2. **Proof Generator**: Generates ZK proofs for cross-chain operations
3. **Transaction Manager**: Submits transactions to destination chains
4. **Batch Event Listener**: Monitors batch processing events
5. **Lane Manager**: Manages volume limits and lane status
6. **Gas Fee Distributer**: Handles ETH balance management and cross-chain rebalancing
7. **Reset Transaction Manager**: Manages transaction resets and rebalancing

## Prerequisites

### System Requirements

- **Rust**: 1.70+ (see `rust-toolchain.toml`)
- **PostgreSQL**: 13+ with SSL support
- **Linux/macOS**: Tested on Ubuntu 22.04+
- **Memory**: 4GB+ RAM recommended
- **Storage**: 10GB+ free space

### External Dependencies

- **Bonsai API**: For ZK proof generation (production)
- **Alchemy/Infura**: For RPC endpoints
- **Pinata**: For IPFS storage (optional)

### Rust Dependencies

The sequencer uses several key Rust crates:

- `tokio`: Async runtime
- `alloy`: Ethereum RPC and contract interactions
- `sqlx`: Database operations
- `risc0-ethereum-contracts`: ZK proof encoding
- `malda_rs`: Custom ZK coprocessor integration

## Installation

### 1. Clone the Repository

```bash
git clone <repository-url>
cd malda-sequencer
```

### 2. Install Rust Dependencies

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Install build dependencies
sudo apt update
sudo apt install build-essential pkg-config libssl-dev libpq-dev

# Install PostgreSQL client
sudo apt install postgresql-client
```

### 3. Build the Project

```bash
# Build in release mode
cargo build --release

# Or build in debug mode for development
cargo build
```

## Configuration

### Environment Setup

The sequencer uses environment-based configuration with support for testnet and mainnet environments.

#### 1. Create Environment Files

Create `.env.testnet` and `.env.mainnet` files:

```bash
# .env.testnet
ENVIRONMENT=testnet
DATABASE_URL=postgres://username:password@host:port/database

# Chain RPC URLs (testnet)
RPC_URL_LINEA_SEPOLIA=https://linea-sepolia.g.alchemy.com/v2/YOUR_API_KEY
RPC_URL_ETHEREUM_SEPOLIA=https://eth-sepolia.g.alchemy.com/v2/YOUR_API_KEY
RPC_URL_OPTIMISM_SEPOLIA=https://opt-sepolia.g.alchemy.com/v2/YOUR_API_KEY
RPC_URL_BASE_SEPOLIA=https://base-sepolia.g.alchemy.com/v2/YOUR_API_KEY

# WebSocket URLs (testnet)
WS_URL_LINEA_SEPOLIA=wss://linea-sepolia.g.alchemy.com/v2/YOUR_API_KEY
WS_URL_ETHEREUM_SEPOLIA=wss://eth-sepolia.g.alchemy.com/v2/YOUR_API_KEY
WS_URL_OPTIMISM_SEPOLIA=wss://opt-sepolia.g.alchemy.com/v2/YOUR_API_KEY
WS_URL_BASE_SEPOLIA=wss://base-sepolia.g.alchemy.com/v2/YOUR_API_KEY

# Fallback URLs (testnet)
RPC_URL_LINEA_SEPOLIA_FALLBACK=https://linea-sepolia.infura.io/v3/YOUR_INFURA_KEY
RPC_URL_ETHEREUM_SEPOLIA_FALLBACK=https://eth-sepolia.infura.io/v3/YOUR_INFURA_KEY
RPC_URL_OPTIMISM_SEPOLIA_FALLBACK=https://opt-sepolia.infura.io/v3/YOUR_INFURA_KEY
RPC_URL_BASE_SEPOLIA_FALLBACK=https://base-sepolia.infura.io/v3/YOUR_INFURA_KEY

# Transaction-specific RPC URLs (testnet)
RPC_URL_LINEA_SEPOLIA_TRANSACTION=https://rpc.sepolia.linea.build
RPC_URL_ETHEREUM_SEPOLIA_TRANSACTION=https://eth-sepolia.g.alchemy.com/v2/YOUR_API_KEY
RPC_URL_OPTIMISM_SEPOLIA_TRANSACTION=https://opt-sepolia.g.alchemy.com/v2/YOUR_API_KEY
RPC_URL_BASE_SEPOLIA_TRANSACTION=https://base-sepolia.g.alchemy.com/v2/YOUR_API_KEY

# Sequencer Configuration
SEQUENCER_ADDRESS=YOUR_SEQUENCER_ADDRESS
SEQUENCER_PRIVATE_KEY=YOUR_SEQUENCER_PRIVATE_KEY

# Gas Fee Distributer
GAS_FEE_DISTRIBUTER_ADDRESS=YOUR_DISTRIBUTER_ADDRESS
GAS_FEE_DISTRIBUTER_PRIVATE_KEY=YOUR_DISTRIBUTER_PRIVATE_KEY

# Bonsai API (for ZK proofs)
BONSAI_API_KEY=YOUR_BONSAI_API_KEY
BONSAI_API_URL=https://api.bonsai.xyz/
IMAGE_ID_BONSAI=YOUR_IMAGE_ID

# Proof Generator Configuration
PROOF_GENERATOR_DUMMY_MODE=true  # Set to false for production
PROOF_GENERATOR_BATCH_LIMIT=100
PROOF_GENERATOR_MAX_PROOF_RETRIES=3
PROOF_GENERATOR_PROOF_RETRY_DELAY=5
PROOF_GENERATOR_PROOF_REQUEST_DELAY=10

# Event Listener Configuration
EVENT_LISTENER_MAX_RETRIES=3
EVENT_LISTENER_RETRY_DELAY_SECS=5
EVENT_LISTENER_POLL_INTERVAL_SECS=2
EVENT_LISTENER_BLOCK_RANGE_OFFSET_L1_FROM=0
EVENT_LISTENER_BLOCK_RANGE_OFFSET_L1_TO=1000
EVENT_LISTENER_BLOCK_RANGE_OFFSET_L2_FROM=0
EVENT_LISTENER_BLOCK_RANGE_OFFSET_L2_TO=1000

# Retarded Event Listener (for delayed processing)
EVENT_LISTENER_RETARDED_MAX_RETRIES=3
EVENT_LISTENER_RETARDED_RETRY_DELAY_SECS=10
EVENT_LISTENER_RETARDED_POLL_INTERVAL_SECS=5
EVENT_LISTENER_RETARDED_BLOCK_RANGE_OFFSET_L1_FROM=1000
EVENT_LISTENER_RETARDED_BLOCK_RANGE_OFFSET_L1_TO=2000
EVENT_LISTENER_RETARDED_BLOCK_RANGE_OFFSET_L2_FROM=1000
EVENT_LISTENER_RETARDED_BLOCK_RANGE_OFFSET_L2_TO=2000

# Transaction Manager Configuration
TRANSACTION_MANAGER_MAX_RETRIES=3
TRANSACTION_MANAGER_RETRY_DELAY=5
TRANSACTION_MANAGER_SUBMISSION_DELAY_SECONDS=2
TRANSACTION_MANAGER_POLL_INTERVAL=2
TRANSACTION_MANAGER_MAX_TX=10
TRANSACTION_MANAGER_TX_TIMEOUT=300
TRANSACTION_MANAGER_GAS_PERCENTAGE_INCREASE_PER_RETRY=10

# Lane Manager Configuration
LANE_MANAGER_POLL_INTERVAL_SECS=2
LANE_MANAGER_MAX_VOLUME=1000000
LANE_MANAGER_TIME_INTERVAL=3600
LANE_MANAGER_BLOCK_DELAY=100
LANE_MANAGER_REORG_PROTECTION=10

# Event Proof Ready Checker
EVENT_PROOF_READY_CHECKER_POLL_INTERVAL_SECONDS=30
EVENT_PROOF_READY_CHECKER_BLOCK_UPDATE_INTERVAL_SECONDS=60

# Reset Transaction Manager
RESET_TX_MANAGER_SAMPLE_SIZE=100
RESET_TX_MANAGER_MULTIPLIER=2
RESET_TX_MANAGER_MAX_RETRIES=3
RESET_TX_MANAGER_RETRY_DELAY_SECS=5
RESET_TX_MANAGER_POLL_INTERVAL_SECS=60
RESET_TX_MANAGER_BATCH_LIMIT=50
RESET_TX_MANAGER_MAX_RETRIES_RESET=3
RESET_TX_MANAGER_API_KEY=YOUR_API_KEY
RESET_TX_MANAGER_REBALANCER_URL=https://rebalancer.example.com
RESET_TX_MANAGER_REBALANCE_DELAY=300
RESET_TX_MANAGER_MINIMUM_USD_VALUE=100

# Gas Fee Distributer Configuration
GAS_FEE_DISTRIBUTER_POLL_INTERVAL_SECS=60
GAS_FEE_DISTRIBUTER_MINIMUM_SEQUENCER_BALANCE_PER_CHAIN=1000000000000000000
GAS_FEE_DISTRIBUTER_MIN_DISTRIBUTOR_BALANCE_PER_CHAIN=1000000000000000000
GAS_FEE_DISTRIBUTER_TARGET_SEQUENCER_BALANCE_PER_CHAIN=5000000000000000000
GAS_FEE_DISTRIBUTER_MINIMUM_HARVEST_BALANCE_PER_CHAIN=100000000000000000
GAS_FEE_DISTRIBUTER_BRIDGE_FEE_PERCENTAGE=1
GAS_FEE_DISTRIBUTER_MIN_AMOUNT_TO_BRIDGE=100000000000000000

# Chain-specific minimum harvest balances
MIN_HARVEST_BALANCE_ETHEREUM=100000000000000000
MIN_HARVEST_BALANCE_LINEA=10000000000000000
MIN_HARVEST_BALANCE_BASE=10000000000000000
MIN_HARVEST_BALANCE_OPTIMISM=10000000000000000

# Sequencer Request URLs (for L2 chains)
SEQUENCER_REQUEST_OPTIMISM_SEPOLIA=http://127.0.0.1:9545/gossip_getSequencerCommitment
SEQUENCER_REQUEST_OPTIMISM_SEPOLIA_FALLBACK=http://127.0.0.1:9545/gossip_getSequencerCommitment
```

```bash
# .env.mainnet
ENVIRONMENT=mainnet
DATABASE_URL=postgres://username:password@host:port/database

# Chain RPC URLs (mainnet)
RPC_URL_LINEA=https://linea-mainnet.g.alchemy.com/v2/YOUR_API_KEY
RPC_URL_ETHEREUM=https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY
RPC_URL_OPTIMISM=https://opt-mainnet.g.alchemy.com/v2/YOUR_API_KEY
RPC_URL_BASE=https://base-mainnet.g.alchemy.com/v2/YOUR_API_KEY

# WebSocket URLs (mainnet)
WS_URL_LINEA=wss://linea-mainnet.g.alchemy.com/v2/YOUR_API_KEY
WS_URL_ETHEREUM=wss://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY
WS_URL_OPTIMISM=wss://opt-mainnet.g.alchemy.com/v2/YOUR_API_KEY
WS_URL_BASE=wss://base-mainnet.g.alchemy.com/v2/YOUR_API_KEY

# Fallback URLs (mainnet)
RPC_URL_LINEA_FALLBACK=https://linea-mainnet.infura.io/v3/YOUR_INFURA_KEY
RPC_URL_ETHEREUM_FALLBACK=https://eth-mainnet.infura.io/v3/YOUR_INFURA_KEY
RPC_URL_OPTIMISM_FALLBACK=https://opt-mainnet.infura.io/v3/YOUR_INFURA_KEY
RPC_URL_BASE_FALLBACK=https://base-mainnet.infura.io/v3/YOUR_INFURA_KEY

# Transaction-specific RPC URLs (mainnet)
RPC_URL_LINEA_TRANSACTION=https://rpc.linea.build
RPC_URL_ETHEREUM_TRANSACTION=https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY
RPC_URL_OPTIMISM_TRANSACTION=https://opt-mainnet.g.alchemy.com/v2/YOUR_API_KEY
RPC_URL_BASE_TRANSACTION=https://base-mainnet.g.alchemy.com/v2/YOUR_API_KEY

# ... (same configuration as testnet but with mainnet values)
```

#### 2. Required API Keys and Services

**RPC Providers:**
- **Alchemy**: Primary RPC provider for all chains
- **Infura**: Fallback RPC provider
- **Chain-specific RPCs**: Linea, Base, Optimism public RPCs

**ZK Proof Services:**
- **Bonsai API**: For production ZK proof generation
- **API Key**: Required for Bonsai service
- **Image ID**: Specific ZK circuit identifier

**Database:**
- **PostgreSQL**: Hosted or self-hosted
- **SSL Support**: Required for secure connections

## Database Setup

### 1. PostgreSQL Installation

```bash
# Ubuntu/Debian
sudo apt update
sudo apt install postgresql postgresql-contrib

# macOS
brew install postgresql
```

### 2. Database Creation

```bash
# Create database and user
sudo -u postgres psql

CREATE DATABASE malda_sequencer;
CREATE USER sequencer_user WITH PASSWORD 'your_password';
GRANT ALL PRIVILEGES ON DATABASE malda_sequencer TO sequencer_user;
\q
```

### 3. Run Migrations

```bash
# Install sqlx CLI
cargo install sqlx-cli

# Run migrations
sqlx migrate run --database-url "postgres://sequencer_user:your_password@localhost/malda_sequencer"
```

### 4. Database Schema

The database includes several key tables:

- **`events`**: Main event tracking table
- **`finished_events`**: Completed events archive
- **`chain_batch_sync`**: Batch synchronization tracking
- **`volume_flow`**: Volume tracking per chain
- **`node_status`**: Provider health monitoring

## Environment Variables

### Core Configuration

| Variable | Description | Required |
|----------|-------------|----------|
| `ENVIRONMENT` | `testnet` or `mainnet` | Yes |
| `DATABASE_URL` | PostgreSQL connection string | Yes |

### Chain Configuration

Each chain requires these variables:

| Pattern | Description | Example |
|---------|-------------|---------|
| `RPC_URL_[CHAIN]` | Primary RPC URL | `https://linea-mainnet.g.alchemy.com/v2/...` |
| `RPC_URL_[CHAIN]_FALLBACK` | Fallback RPC URL | `https://linea-mainnet.infura.io/v3/...` |
| `WS_URL_[CHAIN]` | WebSocket URL | `wss://linea-mainnet.g.alchemy.com/v2/...` |
| `RPC_URL_[CHAIN]_TRANSACTION` | Transaction-specific RPC | `https://rpc.linea.build` |

### Supported Chains

**Testnet:**
- `LINEA_SEPOLIA` (Chain ID: 59141)
- `ETHEREUM_SEPOLIA` (Chain ID: 11155111)
- `OPTIMISM_SEPOLIA` (Chain ID: 11155420)
- `BASE_SEPOLIA` (Chain ID: 84532)

**Mainnet:**
- `LINEA` (Chain ID: 59144)
- `ETHEREUM` (Chain ID: 1)
- `OPTIMISM` (Chain ID: 10)
- `BASE` (Chain ID: 8453)

### ZK Proof Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `BONSAI_API_KEY` | Bonsai API key for ZK proofs | Required |
| `BONSAI_API_URL` | Bonsai API endpoint | `https://api.bonsai.xyz/` |
| `IMAGE_ID_BONSAI` | ZK circuit image ID | Required |
| `PROOF_GENERATOR_DUMMY_MODE` | Use dummy proofs (dev) | `true` |

### Security Configuration

| Variable | Description | Required |
|----------|-------------|----------|
| `SEQUENCER_ADDRESS` | Sequencer wallet address | Yes |
| `SEQUENCER_PRIVATE_KEY` | Sequencer private key | Yes |
| `GAS_FEE_DISTRIBUTER_ADDRESS` | Distributer wallet address | Yes |
| `GAS_FEE_DISTRIBUTER_PRIVATE_KEY` | Distributer private key | Yes |

## Deployment

### 1. Manual Deployment

```bash
# Set environment
export ENVIRONMENT=testnet

# Run database migrations
sqlx migrate run --database-url "$DATABASE_URL"

# Build and run
cargo build --release
./target/release/sequencer
```

### 2. Using Deployment Scripts

The repository includes deployment scripts for easy setup:

```bash
# Deploy to testnet
./scripts/deploy.sh testnet

# Deploy to mainnet
./scripts/deploy.sh mainnet

# Clean build and deploy
./scripts/deploy.sh testnet --clean
```

### 3. Testing Configuration

Test your configuration before deployment:

```bash
# Test configuration
./scripts/test_config.sh

# This will show which chains would be enabled
```

### 4. Database Cleanup

If you need to reset the database:

```bash
# Clean database (WARNING: This will delete all data)
./scripts/clean_db.sh
```

## Development

### 1. Local Development

```bash
# Set environment for development
export ENVIRONMENT=testnet
export RUST_LOG=debug

# Run in debug mode
cargo run

# Run with specific log level
RUST_LOG=info cargo run
```

### 2. Testing

```bash
# Run tests
cargo test

# Run with verbose output
cargo test -- --nocapture
```

### 3. Code Quality

```bash
# Check for warnings
cargo check

# Format code
cargo fmt

# Lint code
cargo clippy
```

### 4. Configuration Testing

```bash
# Test configuration without running
cargo check

# Validate environment variables
./scripts/test_config.sh
```

## Monitoring

### 1. Log Files

The sequencer generates detailed logs:

```bash
# View real-time logs
tail -f logs/sequencer.log

# View specific component logs
grep "Proof Generator" logs/sequencer.log
```

### 2. Database Monitoring

```bash
# Check event status
psql "$DATABASE_URL" -c "SELECT status, COUNT(*) FROM events GROUP BY status;"

# Check recent events
psql "$DATABASE_URL" -c "SELECT * FROM events ORDER BY received_at DESC LIMIT 10;"
```

### 3. Process Monitoring

```bash
# Check if sequencer is running
ps aux | grep sequencer

# Check process ID
cat logs/sequencer.pid
```

### 4. Health Checks

The sequencer provides several health indicators:

- **Database connectivity**: Automatic reconnection
- **RPC provider health**: Fallback to backup providers
- **Event processing**: Real-time event monitoring
- **Proof generation**: Success/failure tracking

## Troubleshooting

### Common Issues

#### 1. Database Connection Issues

```bash
# Test database connection
psql "$DATABASE_URL" -c "\q"

# Check if migrations are up to date
sqlx migrate info --database-url "$DATABASE_URL"
```

#### 2. RPC Provider Issues

```bash
# Test RPC endpoints
curl -X POST -H "Content-Type: application/json" \
  --data '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' \
  https://linea-mainnet.g.alchemy.com/v2/YOUR_API_KEY
```

#### 3. ZK Proof Issues

```bash
# Check Bonsai API status
curl -H "Authorization: Bearer $BONSAI_API_KEY" \
  "$BONSAI_API_URL/status"

# Enable dummy mode for testing
export PROOF_GENERATOR_DUMMY_MODE=true
```

#### 4. Memory Issues

```bash
# Monitor memory usage
htop

# Check for memory leaks
valgrind --tool=memcheck ./target/release/sequencer
```

### Debug Mode

Enable debug logging for detailed troubleshooting:

```bash
# Set debug logging
export RUST_LOG=debug

# Run with debug output
RUST_LOG=debug cargo run
```

### Performance Optimization

1. **Database Indexing**: Ensure proper indexes on frequently queried columns
2. **Connection Pooling**: Configure appropriate connection pool sizes
3. **Batch Processing**: Adjust batch sizes based on system resources
4. **Memory Management**: Monitor memory usage and adjust accordingly

### Security Considerations

1. **Private Keys**: Never commit private keys to version control
2. **API Keys**: Use environment variables for all API keys
3. **Database Security**: Use SSL connections and strong passwords
4. **Network Security**: Restrict database access to trusted IPs

## Contributing

### Development Workflow

1. **Fork the repository**
2. **Create a feature branch**: `git checkout -b feature/your-feature`
3. **Make changes**: Follow Rust coding standards
4. **Test thoroughly**: Run all tests and check configuration
5. **Submit pull request**: Include detailed description of changes

### Code Standards

- Follow Rust formatting: `cargo fmt`
- Run clippy checks: `cargo clippy`
- Add tests for new functionality
- Update documentation for API changes

### Testing Guidelines

- Test both testnet and mainnet configurations
- Verify database migrations work correctly
- Test with dummy proof mode enabled
- Validate all environment variable combinations

## License

[Add your license information here]

## Support

For issues and questions:

1. **Check the logs**: `tail -f logs/sequencer.log`
2. **Review configuration**: Ensure all required variables are set
3. **Test connectivity**: Verify database and RPC connections
4. **Enable debug mode**: `RUST_LOG=debug cargo run`

## Changelog

### Version 0.1.0
- Initial release
- Multi-chain event processing
- ZK proof generation
- Transaction management
- Database integration
- Deployment scripts 
# Malda Sequencer

A high-performance blockchain sequencer that processes cross-chain events, generates Zero-Knowledge (ZK) proofs, and manages transaction submissions across multiple Layer 1 and Layer 2 networks.

## Table of Contents

- [Overview](#overview)
- [Architecture](#architecture)
- [Prerequisites](#prerequisites)
- [Infrastructure Setup](#infrastructure-setup)
- [Sequencer Application Setup](#sequencer-application-setup)
- [Monitoring & Operations](#monitoring--operations)
- [Environment Variables](#environment-variables)
- [Contributing](#contributing)
- [Troubleshooting & Support](#troubleshooting--support)

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
7. **Reset Transaction Manager**: Manages transaction resets and asset rebalancing

## Prerequisites

### System Requirements

- **Rust**: 1.70+ (see `rust-toolchain.toml`)
- **PostgreSQL**: 13+ with SSL support
- **Linux/macOS**: Tested on Ubuntu 22.04+
- **Memory**: 64GB+ RAM recommended
- **Storage**: 16TB+ free space
- **Linux-based operating system**
- **Internet connectivity for time synchronization**
- **Appropriate permissions to modify system settings**

### External Dependencies

- **Bonsai API**: For ZK proof generation (production)
- **Alchemy**: For RPC endpoints (primary provider)
- **Your own node**: For fallback RPC endpoints (optional)

### Database Setup

The sequencer requires a PostgreSQL database for event tracking and state management. You'll need to set up a PostgreSQL instance and configure the `DATABASE_URL` in your environment files.

**Recommended Database Providers:**

- **[Latitude.sh](https://www.latitude.sh)**: High-performance PostgreSQL tailored for demanding workloads
- **[DigitalOcean](https://www.digitalocean.com)**: Managed PostgreSQL databases with automated backups and monitoring

**Database Connection String Format:**
```
postgres://username:password@host:port/database
```

**Example setup steps:**
1. Create a PostgreSQL instance on your chosen provider
2. Note the connection details (host, port, database name, username, password)
3. Configure the `DATABASE_URL` in your `.env.testnet` or `.env.mainnet` file
4. Ensure SSL support is enabled for secure connections

### Rust Dependencies

The sequencer uses several key Rust crates:

- `tokio`: Async runtime
- `alloy`: Ethereum RPC and contract interactions
- `sqlx`: Database operations
- `risc0-ethereum-contracts`: ZK proof encoding
- `malda_rs`: Custom ZK coprocessor integration

## Infrastructure Setup

### 3.1 System Preparation

#### Time Synchronization Setup

The MALDA Sequencer requires accurate system time for proper operation. Follow these steps to check and enable time synchronization.

**Check Current Time Sync Status:**

```bash
timedatectl
```

**Expected Output:**
```
               Local time: [current date and time]
           Universal time: [current UTC time]
                 RTC time: [current RTC time]
                Time zone: [your timezone]
System clock synchronized: yes
              NTP service: active
          RTC in local TZ: no
```

**Key indicators to look for:**
- `System clock synchronized: yes` - Time sync is working
- `NTP service: active` - Network Time Protocol service is running

**Alternative Check Method:**

```bash
systemctl status systemd-timesyncd
```

**Enable Time Synchronization (if not enabled):**

**Option A: Enable systemd-timesyncd (Recommended)**

```bash
# Enable the time synchronization service
sudo systemctl enable systemd-timesyncd

# Start the service
sudo systemctl start systemd-timesyncd

# Verify the service is running
sudo systemctl status systemd-timesyncd
```

**Option B: Enable NTP synchronization via timedatectl**

```bash
# Enable NTP synchronization
sudo timedatectl set-ntp true

# Verify the change
timedatectl
```

**Verify Time Sync is Working:**

```bash
# Check if system clock is synchronized
timedatectl show-timesync --property=NTPSynchronized

# Or use the full status check
timedatectl
```

**Troubleshooting:**

If time synchronization is not working:

1. **Check network connectivity:**
   ```bash
   ping -c 3 8.8.8.8
   ```

2. **Check if firewall is blocking NTP traffic:**
   ```bash
   sudo ufw status
   ```

3. **Restart the time sync service:**
   ```bash
   sudo systemctl restart systemd-timesyncd
   ```

4. **Check system logs for errors:**
   ```bash
   journalctl -u systemd-timesyncd -f
   ```

#### Disk Space Check

The MALDA Sequencer requires sufficient disk space for operation. Check available disk space before proceeding.

**Check Available Disk Space:**

```bash
df -h
```

**Expected Output:**
```
Filesystem      Size  Used Avail Use% Mounted on
/dev/md0        439G   11G  407G   3% /
/dev/md1         14T  236M   14T   1% /home
```

**Requirements:**
- Ensure you have at least **12TB** of free space on the root filesystem (`/`)
- For home directory (`/home`), ensure you have sufficient space for your nodes and data
- Recommended: At least **16TB** total free space for optimal performance

**Verify Space for Nodes:**

If you're planning to run multiple nodes, ensure you have enough space for:
- Node data directories
- Log files
- Temporary files
- Database storage (if applicable)

**Troubleshooting:**

If disk space is insufficient:
1. **Clean up unnecessary files:**
   ```bash
   sudo apt autoremove
   sudo apt autoclean
   ```

2. **Check largest directories:**
   ```bash
   du -h --max-depth=1 / | sort -hr | head -10
   ```

3. **Clear log files (if safe):**
   ```bash
   sudo journalctl --vacuum-time=7d
   ```

### 3.2 Docker Setup

The MALDA Sequencer uses Docker containers for deployment. This step installs Docker and configures it to use the large filesystem for optimal storage.

#### Install Docker

```bash
sudo apt update
sudo apt install -y docker.io docker-compose
```

#### Install Docker Compose Plugin

```bash
# Create the plugins directory
sudo mkdir -p ~/.docker/cli-plugins/

# Download and install the Docker Compose plugin
sudo curl -SL https://github.com/docker/compose/releases/latest/download/docker-compose-linux-x86_64 -o ~/.docker/cli-plugins/docker-compose

# Make it executable
sudo chmod +x ~/.docker/cli-plugins/docker-compose

# Verify installation
docker compose version
```

#### Configure Docker to Use Large Filesystem

```bash
# Stop Docker service
sudo systemctl stop docker

# Create Docker data directory on large filesystem
sudo mkdir -p /home/ubuntu/docker-data

# Create Docker configuration directory
sudo mkdir -p /etc/docker

# Configure Docker to use the large filesystem
sudo tee /etc/docker/daemon.json <<EOF
{
  "data-root": "/home/ubuntu/docker-data"
}
EOF

# Start Docker service
sudo systemctl start docker
```

#### Verify Docker Configuration

```bash
# Check Docker root directory
sudo docker info | grep "Docker Root Dir"

# Expected output:
# Docker Root Dir: /home/ubuntu/docker-data

# Verify the location is on the large filesystem
df -h /home/ubuntu/docker-data

# Expected output:
# Filesystem      Size  Used Avail Use% Mounted on
# /dev/md1         14T  237M   14T   1% /home
```

#### Test Docker Installation

```bash
# Test Docker with a simple container
sudo docker run hello-world

# Check Docker Compose
sudo docker-compose --version
```

**Benefits of this configuration:**
- ✅ All Docker containers, images, and volumes use the 14TB filesystem
- ✅ No need to configure storage for individual containers
- ✅ Data persists across restarts and reboots
- ✅ Optimal performance with large storage capacity

### 3.3 Blockchain Node Setup

This section covers setting up and running blockchain nodes for the MALDA Sequencer system.

#### Linea Node Setup

The MALDA Sequencer requires a Linea node for blockchain interaction. We'll use Linea Besu, which is an implementation of the Besu client with Linea-specific plugins.

**Reference:** [Linea Besu Documentation](https://docs.linea.build/get-started/how-to/run-a-node/linea-besu) - This is the detailed official documentation that can be followed for running a Linea node.

**Prerequisites:**

Before setting up the Linea node, ensure you have:
- Docker and Docker Compose installed (completed in Step 3.2)
- Sufficient disk space (Linea nodes require significant storage)
- Stable internet connection for blockchain synchronization

**Create Linea Node Directory:**

```bash
# Create directory for Linea node
mkdir -p /home/ubuntu/linea-node
cd /home/ubuntu/linea-node
```

**Create Docker Compose Configuration:**

```bash
cat > docker-compose-advanced-mainnet.yaml << 'EOF'
services:
  linea-besu-advanced-mainnet-node:
    platform: linux/amd64
    image: consensys/linea-besu-package:latest
    command: --profile=advanced-mainnet --p2p-host=185.26.8.113 --plugin-linea-l1-rpc-endpoint=https://eth-mainnet.g.alchemy.com/v2/xaLwh_pW3EIPXyTzwLQ-hIRiwtQO-egi
    ports:
      - 30304:30304
      - 8548:8545 # RPC
      - 8558:8546 # WS
    restart: unless-stopped
EOF
```

**Configuration Details:**
- **Profile**: `advanced-mainnet` - Enables Linea-specific plugins for full functionality
- **P2P Host**: Your public IP address for peer discovery
- **L1 RPC Endpoint**: Required for advanced profile to query finalization on L1
- **Ports**: 
  - `30304`: P2P communication
  - `8548`: RPC endpoint (mapped from container's 8545)
  - `8558`: WebSocket endpoint (mapped from container's 8546)

**Start the Linea Node:**

```bash
cd /home/ubuntu/linea-node
sudo docker-compose -f docker-compose-advanced-mainnet.yaml up
```

**Monitor the Node:**

```bash
# View real-time logs
sudo docker-compose -f docker-compose-advanced-mainnet.yaml logs -f

# Check container status
sudo docker ps

# Check resource usage
sudo docker stats
```

**Stop the Node:**

```bash
cd /home/ubuntu/linea-node
sudo docker-compose -f docker-compose-advanced-mainnet.yaml down
```

**Run Node in Detached Mode (Background):**

```bash
cd /home/ubuntu/linea-node
sudo docker-compose -f docker-compose-advanced-mainnet.yaml up -d
```

**Benefits of Detached Mode:**
- ✅ Node continues running after closing terminal
- ✅ Automatic restart on system reboot (due to `restart: unless-stopped`)
- ✅ Can be managed remotely
- ✅ Logs can be viewed separately

**Managing Detached Node:**
```bash
# View logs of detached container
sudo docker-compose -f docker-compose-advanced-mainnet.yaml logs -f

# Stop detached container
sudo docker-compose -f docker-compose-advanced-mainnet.yaml down

# Check if container is running
sudo docker ps | grep linea
```

**Node Synchronization:**

The Linea node will:
- Download the blockchain data (requires significant time for initial sync)
- Connect to peers for network communication
- Provide RPC and WebSocket endpoints for applications
- Support Linea-specific features like `linea_estimateGas`

**Storage Requirements:**
- **Full node**: ~316.63 GB (increasing by ~344.57 MB daily)
- **Archive node**: ~3.5 TB (increasing by ~1.32 GB daily)

All data is stored on the 14TB filesystem configured in Step 3.2.

#### Ethereum Node Setup

The MALDA Sequencer can also use an Ethereum node for additional blockchain interactions. We'll use Reth, a high-performance Ethereum implementation in Rust.

**Reference:** [Reth Docker Documentation](https://reth.rs/installation/docker/)

**Clone the Reth Repository:**

```bash
# Clone the Reth repository
git clone https://github.com/paradigmxyz/reth
cd reth
```

**Modify Docker Configuration:**

The default docker-compose.yml needs to be modified to enable WebSocket support and increase the proof window. The following changes are already applied in the repository:

**WebSocket Support:**
- Added port mapping: `"8546:8546" # websocket"`
- Added WebSocket server: `--ws --ws.addr 0.0.0.0 --ws.port 8546`
- Added WebSocket APIs: `--ws.api "eth,net,web3"`

**Proof Window:**
- Added proof window setting: `--rpc.eth-proof-window 128`

**Generate JWT Token:**

```bash
./etc/generate-jwt.sh
```

**Start the Ethereum Node:**

```bash
docker compose -f etc/docker-compose.yml -f etc/lighthouse.yml up -d
```

**Monitor the Node:**

**View Logs:**
```bash
# View all container logs
docker compose -f etc/docker-compose.yml -f etc/lighthouse.yml logs -f

# View specific container logs
docker compose -f etc/docker-compose.yml -f etc/lighthouse.yml logs -f reth
docker compose -f etc/docker-compose.yml -f etc/lighthouse.yml logs -f lighthouse
```

**Check Container Status:**
```bash
# Check running containers
docker compose -f etc/docker-compose.yml -f etc/lighthouse.yml ps

# Check resource usage
docker stats
```

**Access Monitoring Dashboards:**
- **Grafana Dashboard**: `http://localhost:3000` (admin/admin)
- **Prometheus Metrics**: `http://localhost:9090`

**Available Endpoints:**

- **HTTP RPC**: `http://localhost:8545`
- **WebSocket**: `ws://localhost:8546`
- **Engine API**: `http://localhost:8551`
- **Metrics**: `http://localhost:9001`
- **Grafana Dashboard**: `http://localhost:3000`
- **Prometheus**: `http://localhost:9090`

**Stop the Node:**

```bash
docker compose -f etc/docker-compose.yml -f etc/lighthouse.yml down
```

#### Base Node Setup

The MALDA Sequencer can also utilize a Base node for Base Chain interactions. Base is Coinbase's L2 network built on Ethereum.

**References:**
- [Base Node Documentation](https://docs.base.org/base-chain/node-operators/run-a-base-node)
- [Base Node GitHub Repository](https://github.com/base/node)

**Note:** It's recommended to follow the official documentation for detailed setup instructions. This section provides only the essential commands for Reth client.

**Clone and Setup:**

```bash
# Clone the Base node repository
git clone https://github.com/base/node
cd node
```

**Configure Environment:**

```bash
# Edit the mainnet environment file
nano .env.mainnet
```

**Required Configuration:**
```bash
OP_NODE_L1_ETH_RPC=<your-preferred-l1-rpc>
OP_NODE_L1_BEACON=<your-preferred-l1-beacon>
OP_NODE_L1_BEACON_ARCHIVER=<your-preferred-l1-beacon-archiver>
```

**Download and Restore Snapshot (Recommended):**

```bash
# Create data directory
mkdir -p reth-data

# Download the latest snapshot
wget https://mainnet-reth-archive-snapshots.base.org/$(curl https://mainnet-reth-archive-snapshots.base.org/latest)

# Extract the snapshot
tar -I zstd -xf base-mainnet-reth.tar.zst

# Move the extracted data
mv reth/* reth-data/

# Clean up
rm -rf reth
rm base-mainnet-reth.tar.zst
```

**Start Base Node:**

**Note:** Adjust port mappings in `docker-compose.yml` to avoid conflicts with other nodes.

```bash
# Start with Reth client
CLIENT=reth docker compose up --build
```

**Monitor Base Node:**

```bash
# Test RPC endpoint
curl -d '{"id":0,"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["latest",false]}' \
  -H "Content-Type: application/json" http://localhost:8549

# View logs
docker compose logs -f

# Stop node
docker compose down
```

**Available Endpoints:**
- **HTTP RPC**: `http://localhost:8549`
- **WebSocket**: `ws://localhost:8559`

#### Optimism Node Setup

Optimism node setup works equivalent to Base node setup. Use the same commands and procedures, but change these OP-specific environment variables:

**Required Changes:**
```bash
OP_NODE_NETWORK=optimism-mainnet  # Instead of base-mainnet
RETH_CHAIN=optimism               # Instead of base
RETH_SEQUENCER_HTTP=https://mainnet-sequencer.optimism.io  # Instead of Base sequencer
# etc.
```

**Note:** Optimism can be synced from zero as no Reth snapshot is currently known. Users should check Optimism documentation for available snapshots.

**References:**
- [Optimism Node Documentation](https://docs.optimism.io/chain/run-a-node)
- [Optimism Node GitHub Repository](https://github.com/ethereum-optimism/optimism)

### 3.4 P2P Listener Setup

The OP-P2P-Listener is a specialized component that captures and stores sequencer commitment data from P2P gossip messages. It acts as a listener that monitors the P2P network for block announcements and stores the signature and payload data for external consumption via HTTP API.

#### What OP-P2P-Listener Does

- **Captures Gossip Data**: Listens to P2P gossip messages containing sequencer commitments
- **Stores Signatures**: Captures the 65-byte signatures from incoming blocks
- **Stores Payloads**: Captures the full execution payload data
- **Exposes API**: Provides HTTP endpoint to retrieve the latest sequencer commitment
- **Non-Intrusive**: Works alongside existing op-node without affecting its operation

#### Clone the Repository

```bash
# Clone the op-p2p-listener repository
git clone https://github.com/malda-protocol/op-p2p-listener

# Navigate to the repository
cd op-p2p-listener

# Switch to the correct branch
git checkout p2pListener2
```

#### Build the OP-Node

```bash
# Navigate to the op-node directory
cd op-node

# Build the op-node binary
just

# Verify the binary was created
ls -la bin/op-node
```

#### Configure and Start the Listener

**Important Notes:**
- Adjust the `--l2` WebSocket port to match your running node
- Ensure the `--p2p.listen.tcp` port is not occupied by other services
- The `--rpc.port` determines where the commitment API is exposed

**For Base Mainnet:**

```bash
# Test without nohup first
./bin/op-node \
  --l1=https://eth-mainnet.g.alchemy.com/v2/xaLwh_pW3EIPXyTzwLQ-hIRiwtQO-egi \
  --l1.rpckind=alchemy \
  --l1.beacon=https://ethereum-beacon-api.publicnode.com \
  --l2=ws://localhost:8559 \
  --l2.jwt-secret=./jwt.txt \
  --network=base-mainnet \
  --syncmode=execution-layer \
  --l2.enginekind=geth \
  --rpc.port=7000 \
  --p2p.listen.ip=0.0.0.0 \
  --p2p.listen.tcp=9225 \
  --l1.trustrpc

# If working correctly, run with nohup
nohup ./bin/op-node \
  --l1=https://eth-mainnet.g.alchemy.com/v2/xaLwh_pW3EIPXyTzwLQ-hIRiwtQO-egi \
  --l1.rpckind=alchemy \
  --l1.beacon=https://ethereum-beacon-api.publicnode.com \
  --l2=ws://localhost:8559 \
  --l2.jwt-secret=./jwt.txt \
  --network=base-mainnet \
  --syncmode=execution-layer \
  --l2.enginekind=geth \
  --rpc.port=7000 \
  --p2p.listen.ip=0.0.0.0 \
  --p2p.listen.tcp=9225 \
  --l1.trustrpc > /dev/null 2>&1 &
```

**For Optimism Mainnet:**

```bash
# Test without nohup first
./bin/op-node \
  --l1=https://eth-mainnet.g.alchemy.com/v2/xaLwh_pW3EIPXyTzwLQ-hIRiwtQO-egi \
  --l1.rpckind=alchemy \
  --l1.beacon=https://ethereum-beacon-api.publicnode.com \
  --l2=ws://localhost:8557 \
  --l2.jwt-secret=./jwt.txt \
  --network=op-mainnet \
  --syncmode=execution-layer \
  --l2.enginekind=geth \
  --rpc.port=7001 \
  --p2p.listen.ip=0.0.0.0 \
  --p2p.listen.tcp=9226 \
  --l1.trustrpc

# If working correctly, run with nohup
nohup ./bin/op-node \
  --l1=https://eth-mainnet.g.alchemy.com/v2/xaLwh_pW3EIPXyTzwLQ-hIRiwtQO-egi \
  --l1.rpckind=alchemy \
  --l1.beacon=https://ethereum-beacon-api.publicnode.com \
  --l2=ws://localhost:8557 \
  --l2.jwt-secret=./jwt.txt \
  --network=op-mainnet \
  --syncmode=execution-layer \
  --l2.enginekind=geth \
  --rpc.port=7001 \
  --p2p.listen.ip=0.0.0.0 \
  --p2p.listen.tcp=9226 \
  --l1.trustrpc > /dev/null 2>&1 &
```

#### Test the Sequencer Commitment API

Once the listener is running, you can test if it's working by calling the HTTP endpoint:

```bash
# Test the sequencer commitment endpoint
curl http://localhost:7000/gossip_getSequencerCommitment
```

**Expected Response:**
```json
{
  "data": "0x...",
  "signature": {
    "r": "0x...",
    "s": "0x...",
    "yParity": "0x0"
  }
}
```

**If no data is available yet:**
```json
{
  "error": "No sequencer commitment data available yet"
}
```

## Sequencer Application Setup

### 4.1 Installation

#### Clone the Repository

```bash
git clone <repository-url>
cd malda-sequencer
```

#### Install Rust Dependencies

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Install build dependencies
sudo apt update
sudo apt install build-essential pkg-config libssl-dev libpq-dev

# Install PostgreSQL client
sudo apt install postgresql-client

# Verify PostgreSQL client installation
psql --version

# Test database connection (replace with your actual DATABASE_URL)
# psql "postgres://username:password@host:port/database" -c "SELECT version();"

# Install SQLx CLI for database migrations
cargo install sqlx-cli --no-default-features --features postgres

# Verify SQLx CLI installation
sqlx --version
```

#### Install RISC0 Dependencies

The Malda Sequencer uses RISC0 for Zero-Knowledge proof generation. For detailed installation instructions, follow the [official RISC0 installation guide](https://dev.risczero.com/api/zkvm/install).

```bash
# Install RISC0 toolchain installer
curl -L https://risczero.com/install | bash

# Install RISC0 toolchain
rzup install
```

**Note:** The RISC0 installation includes:
- `rzup`: RISC Zero toolchain installer
- `cargo-risczero`: Tool for creating and building RISC Zero zkVM projects
- RISC Zero toolchain for building zkVM guest programs in Rust

#### Build the Project

```bash
# Build in release mode
cargo build --release

# Or build in debug mode for development
cargo build
```

### 4.2 Configuration

#### Environment Setup

The sequencer uses environment-based configuration with support for testnet and mainnet environments.

**Create Environment Files:**

Create `.env.testnet` and `.env.mainnet` files:

```bash
# .env.testnet
ENVIRONMENT=testnet
DATABASE_URL=postgres://username:password@host:port/database

# Chain RPC URLs (testnet) - Primary Provider (Alchemy)
RPC_URL_LINEA_SEPOLIA=https://linea-sepolia.g.alchemy.com/v2/YOUR_API_KEY
RPC_URL_ETHEREUM_SEPOLIA=https://eth-sepolia.g.alchemy.com/v2/YOUR_API_KEY
RPC_URL_OPTIMISM_SEPOLIA=https://opt-sepolia.g.alchemy.com/v2/YOUR_API_KEY
RPC_URL_BASE_SEPOLIA=https://base-sepolia.g.alchemy.com/v2/YOUR_API_KEY

# WebSocket URLs (testnet) - Primary Provider
WS_URL_LINEA_SEPOLIA=wss://linea-sepolia.g.alchemy.com/v2/YOUR_API_KEY
WS_URL_ETHEREUM_SEPOLIA=wss://eth-sepolia.g.alchemy.com/v2/YOUR_API_KEY
WS_URL_OPTIMISM_SEPOLIA=wss://opt-sepolia.g.alchemy.com/v2/YOUR_API_KEY
WS_URL_BASE_SEPOLIA=wss://base-sepolia.g.alchemy.com/v2/YOUR_API_KEY

# Fallback URLs (testnet) - Your own node or backup provider
RPC_URL_LINEA_SEPOLIA_FALLBACK=https://your-node.linea-sepolia.com
RPC_URL_ETHEREUM_SEPOLIA_FALLBACK=https://your-node.eth-sepolia.com
RPC_URL_OPTIMISM_SEPOLIA_FALLBACK=https://your-node.opt-sepolia.com
RPC_URL_BASE_SEPOLIA_FALLBACK=https://your-node.base-sepolia.com

# Transaction-specific RPC URLs (testnet)
RPC_URL_LINEA_SEPOLIA_TRANSACTION=https://rpc.sepolia.linea.build
RPC_URL_ETHEREUM_SEPOLIA_TRANSACTION=https://eth-sepolia.g.alchemy.com/v2/YOUR_API_KEY
RPC_URL_OPTIMISM_SEPOLIA_TRANSACTION=https://opt-sepolia.g.alchemy.com/v2/YOUR_API_KEY
RPC_URL_BASE_SEPOLIA_TRANSACTION=https://base-sepolia.g.alchemy.com/v2/YOUR_API_KEY

# Sequencer Configuration
SEQUENCER_ADDRESS=YOUR_SEQUENCER_ADDRESS        # Address of the sequencer wallet
SEQUENCER_PRIVATE_KEY=YOUR_SEQUENCER_PRIVATE_KEY  # Private key of the sequencer wallet

# Node Configuration
NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L1=24   # Node delay before using fallback for L1
NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L2=12   # Node delay before using fallback for L2

# Restart Configuration
RESTART_LISTENER_INTERVAL_SECONDS=600           # Modules are restarted in this interval to reset and clean up
RESTART_LISTENER_DELAY_SECONDS=3                # Delay between new module started and old closed to prevent gaps

# Batch Configuration
BATCH_SUBMITTER_ADDRESS=0x04f0cDc5a215dEdf6A1Ed5444E07367e20768041  # Address of Batch Submitter
BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L1=2  # Block delay until Success/Fail is written in DB on L1
BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L2=6  # Block delay until Success/Fail is written in DB on L2
RETARDED_BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L1=50  # Same for retarded batch listener on L1
RETARDED_BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L2=300  # Same for retarded batch listener on L2
BATCH_EVENT_LISTENER_POLLING_INTERVAL=30        # Polling interval for batch events

# Gas Fee Distributer
GAS_FEE_DISTRIBUTER_ADDRESS=YOUR_DISTRIBUTER_ADDRESS
GAS_FEE_DISTRIBUTER_PRIVATE_KEY=YOUR_DISTRIBUTER_PRIVATE_KEY

# Bonsai API (for ZK proofs)
BONSAI_API_KEY=YOUR_BONSAI_API_KEY
BONSAI_API_URL=https://api.bonsai.xyz/
IMAGE_ID_BONSAI=YOUR_IMAGE_ID

# Proof Generator Configuration
PROOF_GENERATOR_DUMMY_MODE=true                 # Set to false for production
PROOF_GENERATOR_BATCH_LIMIT=200                 # Max tx per batch proof
PROOF_GENERATOR_MAX_PROOF_RETRIES=6             # Max retries before fail, after half switch to fallback rpc
PROOF_GENERATOR_PROOF_RETRY_DELAY=1             # Delay between retrying proof generation
PROOF_GENERATOR_PROOF_REQUEST_DELAY=15          # Request a proof every x seconds

# Event Listener Configuration
EVENT_LISTENER_MAX_RETRIES=3                    # Max retry init if something goes wrong before fail
EVENT_LISTENER_RETRY_DELAY_SECS=5               # Delay between re init
EVENT_LISTENER_POLL_INTERVAL_SECS=1             # Queries blockchain for new events every x seconds
EVENT_LISTENER_BLOCK_RANGE_OFFSET_L1_FROM=2     # When started, gets past x blocks for events on L1
EVENT_LISTENER_BLOCK_RANGE_OFFSET_L1_TO=0       # Block to query is x blocks behind tip of chain on L1
EVENT_LISTENER_BLOCK_RANGE_OFFSET_L2_FROM=12    # When started, gets past x blocks for events on L2
EVENT_LISTENER_BLOCK_RANGE_OFFSET_L2_TO=0       # Block to query is x blocks behind tip of chain on L2

# Retarded Event Listener (for delayed processing - picks up missed events)
EVENT_LISTENER_RETARDED_MAX_RETRIES=3           # Max retry init if something goes wrong before fail
EVENT_LISTENER_RETARDED_RETRY_DELAY_SECS=5      # Delay between re init
EVENT_LISTENER_RETARDED_POLL_INTERVAL_SECS=30   # Queries blockchain for new events every x seconds (runs 10 minutes behind tip)
EVENT_LISTENER_RETARDED_BLOCK_RANGE_OFFSET_L1_FROM=50   # When started, gets past x blocks for events on L1
EVENT_LISTENER_RETARDED_BLOCK_RANGE_OFFSET_L1_TO=50     # Block to query is x blocks behind tip of chain on L1
EVENT_LISTENER_RETARDED_BLOCK_RANGE_OFFSET_L2_FROM=300  # When started, gets past x blocks for events on L2
EVENT_LISTENER_RETARDED_BLOCK_RANGE_OFFSET_L2_TO=300    # Block to query is x blocks behind tip of chain on L2

# Transaction Manager Configuration
TRANSACTION_MANAGER_MAX_RETRIES_L1=5           # Max resubmission with higher gas for L1
TRANSACTION_MANAGER_RETRY_DELAY_L1=2           # Delay between restarting submission logic for L1
TRANSACTION_MANAGER_SUBMISSION_DELAY_SECONDS_L1=1  # Delay between submissions for L1
TRANSACTION_MANAGER_POLL_INTERVAL_L1=5         # Check for proofs to submit every x seconds for L1
TRANSACTION_MANAGER_MAX_TX_L1=50               # Max tx per batch submission for L1
TRANSACTION_MANAGER_TX_TIMEOUT_L1=15           # Wait before resubmitting with higher gas for L1
TRANSACTION_MANAGER_MAX_RETRIES_L2=10          # Max resubmission with higher gas for L2
TRANSACTION_MANAGER_RETRY_DELAY_L2=1           # Delay between restarting submission logic for L2
TRANSACTION_MANAGER_SUBMISSION_DELAY_SECONDS_L2=1  # Delay between submissions for L2
TRANSACTION_MANAGER_POLL_INTERVAL_L2=5         # Check for proofs to submit every x seconds for L2
TRANSACTION_MANAGER_MAX_TX_L2=50               # Max tx per batch submission for L2
TRANSACTION_MANAGER_TX_TIMEOUT_L2=4            # Wait before resubmitting with higher gas for L2
TRANSACTION_MANAGER_GAS_PERCENTAGE_INCREASE_PER_RETRY=20  # Increase gas by x % after every timeout

# Lane Manager Configuration
LANE_MANAGER_MAX_RETRIES=5                      # Max retry init if something goes wrong before fail
LANE_MANAGER_RETRY_DELAY_SECS=1                 # Delay between re init
LANE_MANAGER_POLL_INTERVAL_SECS=2               # Check to set reorg delay every x seconds
LANE_MANAGER_MAX_VOLUME_LINEA=4000000           # Max volume per time period in USD for Linea
LANE_MANAGER_MAX_VOLUME_ETHEREUM=4000000        # Max volume per time period in USD for Ethereum
LANE_MANAGER_MAX_VOLUME_BASE=4000000            # Max volume per time period in USD for Base
LANE_MANAGER_INTERVAL_SECS_LINEA=3600           # Time period in seconds for Linea volume limits
LANE_MANAGER_INTERVAL_SECS_ETHEREUM=3600        # Time period in seconds for Ethereum volume limits
LANE_MANAGER_INTERVAL_SECS_BASE=3600            # Time period in seconds for Base volume limits
LANE_MANAGER_BLOCK_DELAY_LINEA=10               # Delay x blocks if volume per time exceeded for Linea
LANE_MANAGER_BLOCK_DELAY_ETHEREUM=10            # Delay x blocks if volume per time exceeded for Ethereum
LANE_MANAGER_BLOCK_DELAY_BASE=10                # Delay x blocks if volume per time exceeded for Base
REORG_PROTECTION_LINEA=2                        # Standard reorg protection as part of the proof for Linea
REORG_PROTECTION_BASE=2                         # Standard reorg protection as part of the proof for Base
REORG_PROTECTION_ETHEREUM=0                     # Standard reorg protection as part of the proof for Ethereum

# Event Proof Ready Checker
PROOF_READY_CHECKER_POLL_INTERVAL_SECONDS=1     # Check if tx have passed ReorgDelay every x seconds
PROOF_READY_CHECKER_BLOCK_UPDATE_INTERVAL_SECONDS=2  # Current block per chain updated every x seconds

# Reset Transaction Manager
RESET_TX_MANAGER_SAMPLE_SIZE=10                 # Last x Success event to determine threshold for reset
RESET_TX_MANAGER_MULTIPLIER=5                   # Multiply avg time in each pipeline step by x for threshold
RESET_TX_MANAGER_MAX_RETRIES=5                  # Retry init
RESET_TX_MANAGER_RETRY_DELAY_SECS=1             # Delay retry init
RESET_TX_MANAGER_POLL_INTERVAL_SECS=30          # Reset stuck tx every x seconds
RESET_TX_MANAGER_BATCH_LIMIT=500                # Limit amount to reset per call
RESET_TX_MANAGER_MAX_RETRIES_RESET=3            # Max retries for reset operations
RESET_TX_MANAGER_API_KEY=YOUR_API_KEY           # API key for reset operations
RESET_TX_MANAGER_REBALANCER_URL=https://rebalancer.example.com  # URL for rebalancer service
RESET_TX_MANAGER_REBALANCE_DELAY=60             # Delay for rebalancing operations
RESET_TX_MANAGER_MINIMUM_USD_VALUE=100          # Minimum USD value for reset operations

# Gas Fee Distributer Configuration
GAS_FEE_DISTRIBUTER_POLL_INTERVAL_SECS=60       # Poll interval for gas fee distribution operations

# Minimum Sequencer Balances (harvest markets if balance below this and send to sequencer)
MIN_SEQUENCER_BALANCE_ETHEREUM=100000000000000000  # 0.1 ETH minimum sequencer balance for Ethereum
MIN_SEQUENCER_BALANCE_LINEA=10000000000000000      # 0.01 ETH minimum sequencer balance for Linea
MIN_SEQUENCER_BALANCE_BASE=10000000000000000       # 0.01 ETH minimum sequencer balance for Base

# Minimum Distributor Balances (keep min balance in fee distributor for gas)
MIN_DISTRIBUTOR_BALANCE_ETHEREUM=10000000000000000  # 0.01 ETH minimum distributor balance for Ethereum
MIN_DISTRIBUTOR_BALANCE_LINEA=1000000000000000      # 0.001 ETH minimum distributor balance for Linea
MIN_DISTRIBUTOR_BALANCE_BASE=1000000000000000       # 0.001 ETH minimum distributor balance for Base

# Target Sequencer Balances (if available top up sequencer to this balance)
TARGET_SEQUENCER_BALANCE_ETHEREUM=300000000000000000  # 0.3 ETH target sequencer balance for Ethereum
TARGET_SEQUENCER_BALANCE_LINEA=30000000000000000      # 0.03 ETH target sequencer balance for Linea
TARGET_SEQUENCER_BALANCE_BASE=30000000000000000       # 0.03 ETH target sequencer balance for Base

# Minimum Harvest Balances (only harvest markets that have more than x ETH)
MIN_HARVEST_BALANCE_ETHEREUM=100000000000000000  # 0.1 ETH minimum harvest balance for Ethereum
MIN_HARVEST_BALANCE_LINEA=10000000000000000      # 0.01 ETH minimum harvest balance for Linea
MIN_HARVEST_BALANCE_BASE=10000000000000000       # 0.01 ETH minimum harvest balance for Base

# Bridge Configuration
BRIDGE_FEE_PERCENTAGE=1                         # Fee to pay for across relayer (1%)
MIN_AMOUNT_TO_BRIDGE=100000000000000000         # Only bridge if more than 0.1 ETH is available

# Sequencer Request URLs (for L2 chains)
SEQUENCER_REQUEST_OPTIMISM_SEPOLIA=http://127.0.0.1:9545/gossip_getSequencerCommitment
SEQUENCER_REQUEST_OPTIMISM_SEPOLIA_FALLBACK=http://127.0.0.1:9545/gossip_getSequencerCommitment
```

```bash
# .env.mainnet
ENVIRONMENT=mainnet
DATABASE_URL=postgres://username:password@host:port/database

# Chain RPC URLs (mainnet) - Primary Provider (Alchemy)
RPC_URL_LINEA=https://linea-mainnet.g.alchemy.com/v2/YOUR_API_KEY
RPC_URL_ETHEREUM=https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY
RPC_URL_OPTIMISM=https://opt-mainnet.g.alchemy.com/v2/YOUR_API_KEY
RPC_URL_BASE=https://base-mainnet.g.alchemy.com/v2/YOUR_API_KEY

# WebSocket URLs (mainnet) - Primary Provider
WS_URL_LINEA=wss://linea-mainnet.g.alchemy.com/v2/YOUR_API_KEY
WS_URL_ETHEREUM=wss://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY
WS_URL_OPTIMISM=wss://opt-mainnet.g.alchemy.com/v2/YOUR_API_KEY
WS_URL_BASE=wss://base-mainnet.g.alchemy.com/v2/YOUR_API_KEY

# Fallback URLs (mainnet) - Your own node or backup provider
RPC_URL_LINEA_FALLBACK=https://your-node.linea-mainnet.com
RPC_URL_ETHEREUM_FALLBACK=https://your-node.eth-mainnet.com
RPC_URL_OPTIMISM_FALLBACK=https://your-node.opt-mainnet.com
RPC_URL_BASE_FALLBACK=https://your-node.base-mainnet.com

# Transaction-specific RPC URLs (mainnet)
RPC_URL_LINEA_TRANSACTION=https://rpc.linea.build
RPC_URL_ETHEREUM_TRANSACTION=https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY
RPC_URL_OPTIMISM_TRANSACTION=https://opt-mainnet.g.alchemy.com/v2/YOUR_API_KEY
RPC_URL_BASE_TRANSACTION=https://base-mainnet.g.alchemy.com/v2/YOUR_API_KEY

# ... (same configuration as testnet but with mainnet values)
```

**Required API Keys and Services:**

**RPC Providers:**
- **Alchemy**: Primary RPC provider for all chains
- **Your own node**: Fallback RPC provider (recommended for production)

**ZK Proof Services:**
- **Bonsai API**: For production ZK proof generation
- **API Key**: Required for Bonsai service
- **Image ID**: Specific ZK circuit identifier

**Database:**
- **PostgreSQL**: Hosted or self-hosted
- **SSL Support**: Required for secure connections

### 4.3 Deployment

#### Using Deployment Scripts (Recommended)

The repository includes comprehensive deployment scripts that handle all setup automatically:

**Quick Setup:**

```bash
# 1. Set your database URL in scripts/deploy.sh
# Edit the DATABASE_URL variable in the script

# 2. Set your Bonsai API key in scripts/deploy.sh  
# Edit the BONSAI_API_KEY variable in the script

# 3. Deploy to testnet
./scripts/deploy.sh testnet

# 4. Deploy to mainnet
./scripts/deploy.sh mainnet

# 5. Clean build and deploy
./scripts/deploy.sh testnet --clean
```

**What the Deployment Script Does:**

The `scripts/deploy.sh` script automatically:

1. **Environment Detection**: Loads the appropriate `.env.testnet` or `.env.mainnet` file
2. **Database Setup**: 
   - Connects to your PostgreSQL database
   - Runs all database migrations automatically
   - Creates necessary tables and indexes
3. **Build Process**: 
   - Performs clean build in release mode
   - Optimizes for production performance
4. **Process Management**:
   - Stops any existing sequencer process
   - Starts the sequencer in background
   - Saves process ID for management
5. **Logging**: 
   - Creates timestamped log files
   - Provides real-time log access
6. **Health Checks**:
   - Verifies database connectivity
   - Confirms process startup
   - Displays configuration summary

**Testing Configuration:**

Test your configuration before deployment:

```bash
# Test which chains would be enabled
./scripts/test_config.sh

# This will show which chains are configured and ready
```

**Database Management:**

```bash
# Clean database (WARNING: This will delete all data)
./scripts/clean_db.sh
```

## Monitoring & Operations

### Log Files

The sequencer generates detailed logs:

```bash
# View real-time logs
tail -f logs/sequencer.log

# View specific component logs
grep "Proof Generator" logs/sequencer.log
```

### Database Monitoring

```bash
# Check event status
psql "$DATABASE_URL" -c "SELECT status, COUNT(*) FROM events GROUP BY status;"

# Check recent events
psql "$DATABASE_URL" -c "SELECT * FROM events ORDER BY received_at DESC LIMIT 10;"
```

### Process Monitoring

```bash
# Check if sequencer is running
ps aux | grep sequencer

# Check process ID
cat logs/sequencer.pid
```

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
| `RPC_URL_[CHAIN]` | Primary RPC URL (Alchemy) | `https://linea-mainnet.g.alchemy.com/v2/...` |
| `RPC_URL_[CHAIN]_FALLBACK` | Fallback RPC URL (Your node) | `https://your-node.linea-mainnet.com` |
| `WS_URL_[CHAIN]` | WebSocket URL (Alchemy) | `wss://linea-mainnet.g.alchemy.com/v2/...` |
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



## Contributing

### Development Workflow

1. **Fork the repository**
2. **Create a feature branch**: `git checkout -b feature/your-feature`
3. **Make changes**: Follow Rust coding standards
4. **Test thoroughly**: Run all tests and check configuration
5. **Submit pull request**: Include detailed description of changes

### Code Standards

- Follow Rust formatting: `cargo fmt`
- Add tests for new functionality
- Update documentation for API changes

### Testing Guidelines

- Test both testnet and mainnet configurations
- Verify database migrations work correctly
- Test with dummy proof mode enabled
- Validate all environment variable combinations

## Troubleshooting & Support

### Common Issues

1. **Time sync not working:**
   - Ensure internet connectivity
   - Check if NTP ports (123) are not blocked by firewall
   - Restart the timesyncd service

2. **Permission denied errors:**
   - Ensure you have sudo privileges
   - Check if you're in the appropriate user group

3. **Sequencer issues:**
   - Check the logs: `tail -f logs/sequencer.log`
   - Review configuration: Ensure all required variables are set
   - Test connectivity: Verify database and RPC connections
   - Enable debug mode: `RUST_LOG=debug cargo run`

### Support

For additional support or questions regarding the MALDA Sequencer setup, please refer to the project documentation or contact the development team.

## License

[Add your license information here]

## Changelog

### Version 0.1.0
- Initial release
- Multi-chain event processing
- ZK proof generation
- Transaction management
- Database integration
- Deployment scripts

---

**Note:** This setup guide assumes a Linux system using systemd. For other operating systems or init systems, the commands may vary. 

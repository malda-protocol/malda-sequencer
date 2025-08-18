# Malda Sequencer Configuration Guide

## Overview

The Malda Sequencer has been redesigned with a simplified configuration system that makes it easy to deploy to different environments (testnet/mainnet) with minimal configuration changes.

## New Configuration System

### Key Benefits

1. **Environment-Based Configuration**: Simply set `ENVIRONMENT=testnet` or `ENVIRONMENT=mainnet`
2. **Automatic Chain Detection**: Chain configurations are automatically loaded based on environment
3. **Centralized Configuration**: All settings are managed through the `config.rs` module
4. **Clean Main Function**: The main.rs file is now much cleaner and easier to read

### How It Works

The new system uses a `SequencerConfig` struct that automatically:

1. **Detects the environment** from the `ENVIRONMENT` environment variable
2. **Loads appropriate chain configurations** based on the environment
3. **Sets up all components** with the correct parameters
4. **Manages all environment variables** in a centralized way

## Usage

### Basic Configuration

To deploy the sequencer, you only need to:

1. **Set the environment**:
   ```bash
   export ENVIRONMENT=testnet  # or mainnet
   ```

2. **Ensure your .env file** has the required variables (see examples below)

3. **Run the sequencer**:
   ```bash
   cargo run --release
   ```

### Environment Variables

The system automatically loads different configurations based on the `ENVIRONMENT` variable:

- `ENVIRONMENT=testnet` → Uses testnet chain IDs and configurations
- `ENVIRONMENT=mainnet` → Uses mainnet chain IDs and configurations

### Chain Configuration

Chains are automatically configured based on the environment and available environment variables. **Only chains with properly configured environment variables will be launched.**

#### Supported Chains

**Testnet Chains:**
- **Linea Sepolia** (Chain ID: 59141) - Requires `RPC_URL_LINEA_SEPOLIA`
- **Optimism Sepolia** (Chain ID: 11155420) - Requires `RPC_URL_OPTIMISM_SEPOLIA`
- **Ethereum Sepolia** (Chain ID: 11155111) - Requires `RPC_URL_ETHEREUM_SEPOLIA`
- **Base Sepolia** (Chain ID: 84532) - Requires `RPC_URL_BASE_SEPOLIA`

**Mainnet Chains:**
- **Linea** (Chain ID: 59144) - Requires `RPC_URL_LINEA`
- **Optimism** (Chain ID: 10) - Requires `RPC_URL_OPTIMISM`
- **Ethereum** (Chain ID: 1) - Requires `RPC_URL_ETHEREUM`
- **Base** (Chain ID: 8453) - Requires `RPC_URL_BASE`

#### Flexible Chain Selection

The system will only launch chains for which the required environment variables are set. For example:

- If you only set `RPC_URL_LINEA_SEPOLIA` and `RPC_URL_ETHEREUM_SEPOLIA`, only Linea Sepolia and Ethereum Sepolia will be launched
- If you don't set `RPC_URL_BASE_SEPOLIA`, Base Sepolia will not be launched
- The system will print which chains were successfully enabled at startup

## Configuration Structure

### SequencerConfig

The main configuration struct contains:

```rust
pub struct SequencerConfig {
    pub environment: Environment,
    pub chains: HashMap<u32, ChainConfig>,
    pub database_url: String,
    pub sequencer_address: Address,
    pub sequencer_private_key: String,
    pub gas_fee_distributer_address: Address,
    pub gas_fee_distributer_private_key: String,
    pub proof_config: ProofConfig,
    pub event_listener_config: EventListenerConfig,
    pub restart_config: RestartConfig,
}
```

### ChainConfig

Each chain has its own configuration:

```rust
pub struct ChainConfig {
    pub chain_id: u32,
    pub name: String,
    pub is_l1: bool,
    pub rpc_url: String,
    pub fallback_rpc_url: String,
    pub ws_url: String,
    pub fallback_ws_url: String,
    pub batch_submitter_address: Address,
    pub markets: Vec<Address>,
    pub events: Vec<String>,
    pub max_block_delay_secs: u64,
    pub block_delay: u64,
    pub retarded_block_delay: u64,
    pub reorg_protection: u64,
    pub transaction_config: TransactionChainConfig,
    pub lane_config: LaneChainConfig,
}
```

## Migration from Old System

### Before (Old main.rs)
```rust
// Hardcoded configurations scattered throughout
let chain_configs = vec![
    (
        &ws_url_linea,
        &ws_url_linea_backup,
        LINEA_SEPOLIA_CHAIN_ID,
        vec![
            HOST_BORROW_ON_EXTENSION_CHAIN_SIG,
            HOST_WITHDRAW_ON_EXTENSION_CHAIN_SIG,
        ],
    ),
    // ... more hardcoded configs
];

// Repetitive environment variable parsing
let max_block_delay_secs = if *chain_id == ETHEREUM_CHAIN_ID || *chain_id == ETHEREUM_SEPOLIA_CHAIN_ID {
    std::env::var("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L1").expect("...").parse().unwrap()
} else {
    std::env::var("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L2").expect("...").parse().unwrap()
};
```

### After (New main.rs)
```rust
// Simple configuration loading
let config = SequencerConfig::new()?;

// Clean component initialization
for chain in config.get_all_chains() {
    info!("Starting batch event listener for {}", chain.name);
    // All configuration is handled automatically
}
```

## Environment-Specific .env Files

### Testnet (.env.testnet)
```bash
ENVIRONMENT=testnet
DATABASE_URL=postgres://...
RPC_URL_LINEA_SEPOLIA=https://linea-sepolia.g.alchemy.com/v2/...
# ... other testnet-specific variables
```

### Mainnet (.env.mainnet)
```bash
ENVIRONMENT=mainnet
DATABASE_URL=postgres://...
RPC_URL_LINEA=https://linea-mainnet.g.alchemy.com/v2/...
# ... other mainnet-specific variables
```

## Adding New Chains

To add a new chain:

1. **Add chain constants** to `malda_rs::constants`
2. **Update the configuration** in `config.rs`:
   ```rust
   fn init_testnet_chains(&mut self) -> Result<()> {
       // Add new chain configuration
       self.chains.insert(NEW_CHAIN_ID, ChainConfig {
           chain_id: NEW_CHAIN_ID,
           name: "New Chain".to_string(),
           is_l1: false, // or true for L1
           // ... other configuration
       });
       Ok(())
   }
   ```
3. **Add environment variables** to your .env file

## Benefits of the New System

1. **Simplified Deployment**: Just change `ENVIRONMENT` variable
2. **Reduced Configuration Errors**: Centralized validation
3. **Better Maintainability**: All config logic in one place
4. **Easier Testing**: Clear separation between environments
5. **Type Safety**: Strongly typed configuration structs
6. **Documentation**: Self-documenting configuration structure

## Troubleshooting

### Common Issues

1. **Missing Environment Variable**: Check that all required variables are set in your .env file
2. **Wrong Environment**: Ensure `ENVIRONMENT` is set to `testnet` or `mainnet`
3. **Chain Configuration**: Verify that chain IDs match your environment

### Debugging

Enable debug logging to see configuration details:
```bash
RUST_LOG=debug cargo run
```

This will show:
- Which environment is being used
- Which chains are configured
- Configuration values being loaded

### Startup Output

When the sequencer starts, you'll see output like:
```
✅ Enabled testnet chains: Linea Sepolia, Ethereum Sepolia
```

This indicates which chains were successfully configured and will be launched. If a chain is missing from this list, check that its required environment variables are set.

## Future Enhancements

1. **Mainnet Support**: Complete mainnet chain configurations
2. **Configuration Validation**: Add validation for required fields
3. **Hot Reloading**: Support for configuration changes without restart
4. **Configuration UI**: Web interface for configuration management 
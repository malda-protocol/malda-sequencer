#!/bin/bash
set -e

echo "Testing Flexible Chain Configuration"
echo "==================================="
echo "This script demonstrates how the flexible chain configuration works."
echo "It shows which chains would be enabled based on different environment variable combinations."
echo ""

# Determine environment
ENVIRONMENT=${ENVIRONMENT:-testnet}
echo "🔧 Testing for environment: $ENVIRONMENT"

# Load environment variables from .env file
if [ -f ".env" ]; then
    echo "Loading environment variables from .env file..."
    export $(cat .env | grep -v '^#' | xargs)
fi
echo ""

if [ "$ENVIRONMENT" = "mainnet" ]; then
    # Mainnet chain variables
    LINEA_VAR="RPC_URL_LINEA"
    OPTIMISM_VAR="RPC_URL_OPTIMISM"
    ETHEREUM_VAR="RPC_URL_ETHEREUM"
    BASE_VAR="RPC_URL_BASE"
    LINEA_NAME="Linea Mainnet"
    OPTIMISM_NAME="Optimism Mainnet"
    ETHEREUM_NAME="Ethereum Mainnet"
    BASE_NAME="Base Mainnet"
else
    # Testnet chain variables
    LINEA_VAR="RPC_URL_LINEA_SEPOLIA"
    OPTIMISM_VAR="RPC_URL_OPTIMISM_SEPOLIA"
    ETHEREUM_VAR="RPC_URL_ETHEREUM_SEPOLIA"
    BASE_VAR="RPC_URL_BASE_SEPOLIA"
    LINEA_NAME="Linea Sepolia"
    OPTIMISM_NAME="Optimism Sepolia"
    ETHEREUM_NAME="Ethereum Sepolia"
    BASE_NAME="Base Sepolia"
fi

echo "🧪 Test: Only $LINEA_NAME"
echo "Environment variables: "
echo "export $LINEA_VAR='https://linea-$ENVIRONMENT.example.com'"
echo "export WS_URL_${LINEA_VAR#RPC_URL_}='ws://localhost:8554/'"
echo ""
echo "----------------------------------------"
echo "Testing configuration..."
if [ -n "${!LINEA_VAR}" ]; then
    echo "✅ $LINEA_NAME would be enabled"
else
    echo "❌ $LINEA_NAME would be disabled (missing $LINEA_VAR)"
fi
if [ -n "${!OPTIMISM_VAR}" ]; then
    echo "✅ $OPTIMISM_NAME would be enabled"
else
    echo "❌ $OPTIMISM_NAME would be disabled (missing $OPTIMISM_VAR)"
fi
if [ -n "${!ETHEREUM_VAR}" ]; then
    echo "✅ $ETHEREUM_NAME would be enabled"
else
    echo "❌ $ETHEREUM_NAME would be disabled (missing $ETHEREUM_VAR)"
fi
if [ -n "${!BASE_VAR}" ]; then
    echo "✅ $BASE_NAME would be enabled"
else
    echo "❌ $BASE_NAME would be disabled (missing $BASE_VAR)"
fi
echo ""

echo "🧪 Test: $LINEA_NAME and $ETHEREUM_NAME"
echo "Environment variables: "
echo "export $LINEA_VAR='https://linea-$ENVIRONMENT.example.com'"
echo "export WS_URL_${LINEA_VAR#RPC_URL_}='ws://localhost:8554/'"
echo "export $ETHEREUM_VAR='https://eth-$ENVIRONMENT.example.com'"
echo "export WS_URL_${ETHEREUM_VAR#RPC_URL_}='ws://localhost:8551/'"
echo ""
echo "----------------------------------------"
echo "Testing configuration..."
if [ -n "${!LINEA_VAR}" ]; then
    echo "✅ $LINEA_NAME would be enabled"
else
    echo "❌ $LINEA_NAME would be disabled (missing $LINEA_VAR)"
fi
if [ -n "${!OPTIMISM_VAR}" ]; then
    echo "✅ $OPTIMISM_NAME would be enabled"
else
    echo "❌ $OPTIMISM_NAME would be disabled (missing $OPTIMISM_VAR)"
fi
if [ -n "${!ETHEREUM_VAR}" ]; then
    echo "✅ $ETHEREUM_NAME would be enabled"
else
    echo "❌ $ETHEREUM_NAME would be disabled (missing $ETHEREUM_VAR)"
fi
if [ -n "${!BASE_VAR}" ]; then
    echo "✅ $BASE_NAME would be enabled"
else
    echo "❌ $BASE_NAME would be disabled (missing $BASE_VAR)"
fi
echo ""

echo "🧪 Test: All $ENVIRONMENT Chains"
echo "Environment variables: "
echo "export $LINEA_VAR='https://linea-$ENVIRONMENT.example.com'"
echo "export WS_URL_${LINEA_VAR#RPC_URL_}='ws://localhost:8554/'"
echo "export $OPTIMISM_VAR='https://opt-$ENVIRONMENT.example.com'"
echo "export WS_URL_${OPTIMISM_VAR#RPC_URL_}='ws://localhost:8552/'"
echo "export $ETHEREUM_VAR='https://eth-$ENVIRONMENT.example.com'"
echo "export WS_URL_${ETHEREUM_VAR#RPC_URL_}='ws://localhost:8551/'"
echo "export $BASE_VAR='https://base-$ENVIRONMENT.example.com'"
echo "export WS_URL_${BASE_VAR#RPC_URL_}='ws://localhost:8546/'"
echo ""
echo "----------------------------------------"
echo "Testing configuration..."
if [ -n "${!LINEA_VAR}" ]; then
    echo "✅ $LINEA_NAME would be enabled"
else
    echo "❌ $LINEA_NAME would be disabled (missing $LINEA_VAR)"
fi
if [ -n "${!OPTIMISM_VAR}" ]; then
    echo "✅ $OPTIMISM_NAME would be enabled"
else
    echo "❌ $OPTIMISM_NAME would be disabled (missing $OPTIMISM_VAR)"
fi
if [ -n "${!ETHEREUM_VAR}" ]; then
    echo "✅ $ETHEREUM_NAME would be enabled"
else
    echo "❌ $ETHEREUM_NAME would be disabled (missing $ETHEREUM_VAR)"
fi
if [ -n "${!BASE_VAR}" ]; then
    echo "✅ $BASE_NAME would be enabled"
else
    echo "❌ $BASE_NAME would be disabled (missing $BASE_VAR)"
fi
echo ""

echo "🧪 Test: No Chains Configured"
echo "Environment variables: "
echo ""
echo "----------------------------------------"
echo "Testing configuration..."
if [ -n "${!LINEA_VAR}" ]; then
    echo "✅ $LINEA_NAME would be enabled"
else
    echo "❌ $LINEA_NAME would be disabled (missing $LINEA_VAR)"
fi
if [ -n "${!OPTIMISM_VAR}" ]; then
    echo "✅ $OPTIMISM_NAME would be enabled"
else
    echo "❌ $OPTIMISM_NAME would be disabled (missing $OPTIMISM_VAR)"
fi
if [ -n "${!ETHEREUM_VAR}" ]; then
    echo "✅ $ETHEREUM_NAME would be enabled"
else
    echo "❌ $ETHEREUM_NAME would be disabled (missing $ETHEREUM_VAR)"
fi
if [ -n "${!BASE_VAR}" ]; then
    echo "✅ $BASE_NAME would be enabled"
else
    echo "❌ $BASE_NAME would be disabled (missing $BASE_VAR)"
fi
echo ""

echo "📋 Summary:"
echo "The system will only launch chains for which the required environment variables are set."
echo "This allows you to:"
echo "  - Run with only specific chains for testing"
echo "  - Gradually add chains by setting their environment variables"
echo "  - Avoid errors when some chains are not available"
echo ""
echo "Required variables per chain:"
echo "  - RPC_URL_[CHAIN]_[ENVIRONMENT]"
echo "  - WS_URL_[CHAIN]_[ENVIRONMENT]"
echo "  - Plus fallback URLs for reliability"
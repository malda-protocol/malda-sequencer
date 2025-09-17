# Malda Sequencer API Usage Guide

This document provides comprehensive documentation for the Malda Sequencer REST API, which manages boundless users in the sequencer database.

## Table of Contents
- [Configuration](#configuration)
- [Authentication](#authentication)
- [API Endpoints](#api-endpoints)
- [Request/Response Formats](#requestresponse-formats)
- [Error Handling](#error-handling)
- [Examples](#examples)
- [Troubleshooting](#troubleshooting)
- [Security Notes](#security-notes)

## Configuration

### Environment Variables

The API server can be configured using the following environment variables:

```bash
# API Server Configuration
API_HOST=0.0.0.0          # Host to bind to (default: 0.0.0.0)
API_PORT=3000             # Port to listen on (default: 3000)
API_KEY=your-secret-key   # API key for authentication (default: default-api-key)
```

### Default Configuration

If no environment variables are set, the API server uses these defaults:
- **Host**: `0.0.0.0` (binds to all network interfaces)
- **Port**: `3000`
- **API Key**: `default-api-key`

## Authentication

All API endpoints (except `/health`) require authentication using an API key in the `X-API-Key` header.

### Authentication Header

```bash
X-API-Key: your-api-key-here
```

### Authentication Examples

```bash
# Valid API key
curl -H "X-API-Key: default-api-key" http://localhost:3000/api/boundless-users

# Missing API key (returns 401 Unauthorized)
curl http://localhost:3000/api/boundless-users

# Invalid API key (returns 401 Unauthorized)
curl -H "X-API-Key: wrong-key" http://localhost:3000/api/boundless-users
```

## API Endpoints

### 1. Health Check

**Endpoint**: `GET /health`

**Description**: Check if the API server is running and healthy.

**Authentication**: Not required

**Response**: Always returns `200 OK` with server status.

**Example**:
```bash
curl http://localhost:3000/health
```

**Response**:
```json
{
  "success": true,
  "message": "API server is healthy",
  "data": null
}
```

### 2. Add Boundless User

**Endpoint**: `POST /api/boundless-users`

**Description**: Add a new boundless user to the database.

**Authentication**: Required (`X-API-Key` header)

**Request Body**:
```json
{
  "address": "0x1234567890123456789012345678901234567890"
}
```

**Validation**:
- Address must be a valid Ethereum address format (42 characters, starts with `0x`, hex characters only)
- Address is case-insensitive

**Example**:
```bash
curl -X POST \
  -H "X-API-Key: default-api-key" \
  -H "Content-Type: application/json" \
  -d '{"address": "0x1234567890123456789012345678901234567890"}' \
  http://localhost:3000/api/boundless-users
```

**Success Response** (200 OK):
```json
{
  "success": true,
  "message": "Successfully added boundless user: 0x1234567890123456789012345678901234567890",
  "data": null
}
```

**Error Response** (400 Bad Request - Invalid Address):
```json
{
  "success": false,
  "message": "Invalid Ethereum address format",
  "data": null
}
```

### 3. Check if User is Boundless

**Endpoint**: `GET /api/boundless-users/{address}`

**Description**: Check if a specific address is in the boundless users list.

**Authentication**: Required (`X-API-Key` header)

**Path Parameter**: `{address}` - Ethereum address to check

**Example**:
```bash
curl -H "X-API-Key: default-api-key" \
  http://localhost:3000/api/boundless-users/0x1234567890123456789012345678901234567890
```

**Success Response** (200 OK) - User is boundless:
```json
{
  "success": true,
  "message": "User 0x1234567890123456789012345678901234567890 is boundless",
  "data": true
}
```

**Success Response** (200 OK) - User is not boundless:
```json
{
  "success": true,
  "message": "User 0x9999999999999999999999999999999999999999 is not boundless",
  "data": false
}
```

### 4. Remove Boundless User

**Endpoint**: `DELETE /api/boundless-users/{address}`

**Description**: Remove a boundless user from the database.

**Authentication**: Required (`X-API-Key` header)

**Path Parameter**: `{address}` - Ethereum address to remove

**Example**:
```bash
curl -X DELETE \
  -H "X-API-Key: default-api-key" \
  http://localhost:3000/api/boundless-users/0x1234567890123456789012345678901234567890
```

**Success Response** (200 OK):
```json
{
  "success": true,
  "message": "Successfully removed boundless user: 0x1234567890123456789012345678901234567890",
  "data": null
}
```

**Note**: The endpoint returns success even if the user doesn't exist (graceful handling).

## Request/Response Formats

### Standard Response Format

All API responses follow this standard format:

```json
{
  "success": boolean,
  "message": "string",
  "data": any
}
```

**Fields**:
- `success`: Boolean indicating if the operation was successful
- `message`: Human-readable message describing the result
- `data`: Response data (can be null, boolean, object, or array)

### Request Headers

**Required for authenticated endpoints**:
```
X-API-Key: your-api-key
Content-Type: application/json  # For POST requests
```

## Error Handling

### HTTP Status Codes

| Status Code | Description | Example |
|-------------|-------------|---------|
| 200 | Success | Operation completed successfully |
| 400 | Bad Request | Invalid Ethereum address format |
| 401 | Unauthorized | Missing or invalid API key |
| 404 | Not Found | Endpoint does not exist |
| 405 | Method Not Allowed | HTTP method not supported for endpoint |

### Error Response Examples

**401 Unauthorized** (Missing API Key):
```bash
curl http://localhost:3000/api/boundless-users
# Response: 401 Unauthorized (no body)
```

**401 Unauthorized** (Invalid API Key):
```bash
curl -H "X-API-Key: wrong-key" http://localhost:3000/api/boundless-users
# Response: 401 Unauthorized (no body)
```

**400 Bad Request** (Invalid Address):
```bash
curl -X POST \
  -H "X-API-Key: default-api-key" \
  -H "Content-Type: application/json" \
  -d '{"address": "invalid-address"}' \
  http://localhost:3000/api/boundless-users

# Response:
{
  "success": false,
  "message": "Invalid Ethereum address format",
  "data": null
}
```

**404 Not Found** (Non-existent Endpoint):
```bash
curl -H "X-API-Key: default-api-key" http://localhost:3000/api/non-existent
# Response: 404 Not Found (no body)
```

**405 Method Not Allowed**:
```bash
curl -X PUT -H "X-API-Key: default-api-key" http://localhost:3000/api/boundless-users
# Response: 405 Method Not Allowed (no body)
```

## Examples

### Complete Workflow Example

Here's a complete example of adding, checking, and removing a boundless user:

```bash
# 1. Check if user exists (should return false)
curl -H "X-API-Key: default-api-key" \
  http://localhost:3000/api/boundless-users/0x1234567890123456789012345678901234567890

# Response: {"success":true,"message":"User 0x1234567890123456789012345678901234567890 is not boundless","data":false}

# 2. Add the user
curl -X POST \
  -H "X-API-Key: default-api-key" \
  -H "Content-Type: application/json" \
  -d '{"address": "0x1234567890123456789012345678901234567890"}' \
  http://localhost:3000/api/boundless-users

# Response: {"success":true,"message":"Successfully added boundless user: 0x1234567890123456789012345678901234567890","data":null}

# 3. Check if user exists (should return true)
curl -H "X-API-Key: default-api-key" \
  http://localhost:3000/api/boundless-users/0x1234567890123456789012345678901234567890

# Response: {"success":true,"message":"User 0x1234567890123456789012345678901234567890 is boundless","data":true}

# 4. Remove the user
curl -X DELETE \
  -H "X-API-Key: default-api-key" \
  http://localhost:3000/api/boundless-users/0x1234567890123456789012345678901234567890

# Response: {"success":true,"message":"Successfully removed boundless user: 0x1234567890123456789012345678901234567890","data":null}

# 5. Verify removal (should return false)
curl -H "X-API-Key: default-api-key" \
  http://localhost:3000/api/boundless-users/0x1234567890123456789012345678901234567890

# Response: {"success":true,"message":"User 0x1234567890123456789012345678901234567890 is not boundless","data":false}
```

### Edge Cases

**Zero Address**:
```bash
# Add zero address (valid Ethereum address)
curl -X POST \
  -H "X-API-Key: default-api-key" \
  -H "Content-Type: application/json" \
  -d '{"address": "0x0000000000000000000000000000000000000000"}' \
  http://localhost:3000/api/boundless-users

# Response: {"success":true,"message":"Successfully added boundless user: 0x0000000000000000000000000000000000000000","data":null}
```

**Duplicate Addition** (Idempotent):
```bash
# Add the same user twice - both return success
curl -X POST \
  -H "X-API-Key: default-api-key" \
  -H "Content-Type: application/json" \
  -d '{"address": "0x1234567890123456789012345678901234567890"}' \
  http://localhost:3000/api/boundless-users

curl -X POST \
  -H "X-API-Key: default-api-key" \
  -H "Content-Type: application/json" \
  -d '{"address": "0x1234567890123456789012345678901234567890"}' \
  http://localhost:3000/api/boundless-users

# Both return: {"success":true,"message":"Successfully added boundless user: 0x1234567890123456789012345678901234567890","data":null}
```

**Remove Non-existent User**:
```bash
# Remove a user that doesn't exist (graceful handling)
curl -X DELETE \
  -H "X-API-Key: default-api-key" \
  http://localhost:3000/api/boundless-users/0x9999999999999999999999999999999999999999

# Response: {"success":true,"message":"Successfully removed boundless user: 0x9999999999999999999999999999999999999999","data":null}
```

## Troubleshooting

### Common Issues

**1. Connection Refused**
```bash
curl: (7) Failed to connect to localhost port 3000: Connection refused
```
**Solution**: Ensure the sequencer is running and the API server has started. Check the logs for API server startup messages.

**2. 401 Unauthorized**
```bash
HTTP/1.1 401 Unauthorized
```
**Solution**: Include the correct `X-API-Key` header in your request.

**3. 400 Bad Request**
```bash
{"success":false,"message":"Invalid Ethereum address format","data":null}
```
**Solution**: Ensure the Ethereum address is in the correct format (42 characters, starts with `0x`, hex characters only).

### Debugging Commands

**Check if API server is running**:
```bash
# Check if port 3000 is listening
netstat -tlnp | grep :3000

# Test health endpoint
curl http://localhost:3000/health
```

**Check API server logs**:
```bash
# Look for API server startup messages in sequencer logs
tail -f logs/sequencer.log | grep -i "api"
```

**Test from different IP**:
```bash
# Test external access
curl http://0.0.0.0:3000/health
```

### Environment Variable Debugging

**Check current API configuration**:
```bash
echo "API_HOST: ${API_HOST:-default (0.0.0.0)}"
echo "API_PORT: ${API_PORT:-default (3000)}"
echo "API_KEY: ${API_KEY:-default (default-api-key)}"
```

## Security Notes

### API Key Security

1. **Use Strong API Keys**: Replace the default API key with a strong, randomly generated key
2. **Environment Variables**: Store API keys in environment variables, not in code
3. **HTTPS in Production**: Use HTTPS in production environments
4. **Rate Limiting**: Consider implementing rate limiting for production use

### Network Security

1. **Firewall Configuration**: Configure firewalls to restrict access to the API port
2. **Network Isolation**: Run the API server in a secure network environment
3. **Access Control**: Limit API access to trusted IP addresses if possible

### Example Production Configuration

```bash
# Production environment variables
export API_HOST=0.0.0.0
export API_PORT=3000
export API_KEY=your-super-secure-random-api-key-here

# Start sequencer with production config
./deploy.sh
```

## Integration Examples

### JavaScript/Node.js

```javascript
const axios = require('axios');

const API_BASE = 'http://localhost:3000';
const API_KEY = 'default-api-key';

const api = axios.create({
  baseURL: API_BASE,
  headers: {
    'X-API-Key': API_KEY,
    'Content-Type': 'application/json'
  }
});

// Add boundless user
async function addBoundlessUser(address) {
  try {
    const response = await api.post('/api/boundless-users', { address });
    return response.data;
  } catch (error) {
    console.error('Error adding boundless user:', error.response?.data);
    throw error;
  }
}

// Check if user is boundless
async function isBoundlessUser(address) {
  try {
    const response = await api.get(`/api/boundless-users/${address}`);
    return response.data.data; // Returns boolean
  } catch (error) {
    console.error('Error checking boundless user:', error.response?.data);
    throw error;
  }
}

// Remove boundless user
async function removeBoundlessUser(address) {
  try {
    const response = await api.delete(`/api/boundless-users/${address}`);
    return response.data;
  } catch (error) {
    console.error('Error removing boundless user:', error.response?.data);
    throw error;
  }
}

// Usage example
async function example() {
  const address = '0x1234567890123456789012345678901234567890';
  
  // Add user
  await addBoundlessUser(address);
  
  // Check status
  const isBoundless = await isBoundlessUser(address);
  console.log(`User is boundless: ${isBoundless}`);
  
  // Remove user
  await removeBoundlessUser(address);
}
```

### Python

```python
import requests

API_BASE = 'http://localhost:3000'
API_KEY = 'default-api-key'

headers = {
    'X-API-Key': API_KEY,
    'Content-Type': 'application/json'
}

def add_boundless_user(address):
    """Add a boundless user."""
    response = requests.post(
        f'{API_BASE}/api/boundless-users',
        json={'address': address},
        headers=headers
    )
    response.raise_for_status()
    return response.json()

def is_boundless_user(address):
    """Check if a user is boundless."""
    response = requests.get(
        f'{API_BASE}/api/boundless-users/{address}',
        headers=headers
    )
    response.raise_for_status()
    return response.json()['data']  # Returns boolean

def remove_boundless_user(address):
    """Remove a boundless user."""
    response = requests.delete(
        f'{API_BASE}/api/boundless-users/{address}',
        headers=headers
    )
    response.raise_for_status()
    return response.json()

# Usage example
if __name__ == '__main__':
    address = '0x1234567890123456789012345678901234567890'
    
    # Add user
    result = add_boundless_user(address)
    print(f"Added user: {result}")
    
    # Check status
    is_boundless = is_boundless_user(address)
    print(f"User is boundless: {is_boundless}")
    
    # Remove user
    result = remove_boundless_user(address)
    print(f"Removed user: {result}")
```

---

**Last Updated**: August 25, 2025  
**API Version**: 1.0  
**Tested**: All endpoints verified and working 
// Copyright (c) 2026 Merge Layers Inc.
//
// This source code is licensed under the Business Source License 1.1
// (the "License"); you may not use this file except in compliance with the
// License. You may obtain a copy of the License at
//
//     https://github.com/malda-protocol/malda-sequencer/blob/main/LICENSE-BSL.md
//
// See the License for the specific language governing permissions and
// limitations under the License.

//! # API Module
//!
//! This module provides a REST API server for managing boundless users
//! and other database operations. It serves as a layer between external
//! clients and the database, providing a clean interface for user management.
//!
//! ## Key Features:
//! - **REST API Server**: HTTP endpoints for database operations
//! - **Boundless User Management**: Add/remove users from boundless_users table
//! - **Address Validation**: Ethereum address format validation
//! - **API Key Authentication**: Simple API key validation
//! - **Error Handling**: Proper HTTP status codes and error responses
//!
//! ## API Endpoints:
//! - `POST /api/boundless-users` - Add a boundless user
//! - `DELETE /api/boundless-users/{address}` - Remove a boundless user
//! - `GET /api/boundless-users/{address}` - Check if user is boundless
//! - `GET /health` - Health check endpoint

use axum::{
    extract::{Path, State},
    http::{StatusCode, HeaderMap},
    response::{IntoResponse, Json},
    routing::{get, post, delete},
    Router,
    middleware::{self, Next},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, error};
use eyre::Result;

use sequencer::database::Database;

/// API server configuration
#[derive(Clone)]
pub struct ApiConfig {
    /// Host address to bind the server to
    pub host: String,
    /// Port to bind the server to
    pub port: u16,
    /// API key for authentication
    pub api_key: String,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 3000,
            api_key: "default-api-key".to_string(),
        }
    }
}

/// Request structure for adding a boundless user
#[derive(Deserialize)]
pub struct AddBoundlessUserRequest {
    /// Ethereum address to add
    pub address: String,
}

/// Response structure for API operations
#[derive(Serialize)]
pub struct ApiResponse<T> {
    /// Success status
    pub success: bool,
    /// Response message
    pub message: String,
    /// Response data (optional)
    pub data: Option<T>,
}

/// API server state
#[derive(Clone)]
pub struct ApiState {
    /// Database connection
    pub db: Database,
    /// API configuration
    pub config: ApiConfig,
}

/// API server implementation
pub struct ApiServer {
    /// Server configuration
    config: ApiConfig,
    /// Database connection
    db: Database,
}

impl ApiServer {
    /// Creates a new API server instance
    pub fn new(config: ApiConfig, db: Database) -> Self {
        Self { config, db }
    }

    /// Starts the API server
    pub async fn start(&self) -> Result<()> {
        let state = ApiState {
            db: self.db.clone(),
            config: self.config.clone(),
        };

        // Create router with routes
        let app = Router::new()
            .route("/health", get(health_check))
            .route("/api/boundless-users", post(add_boundless_user))
            .route("/api/boundless-users/:address", delete(remove_boundless_user))
            .route("/api/boundless-users/:address", get(is_boundless_user))
            .layer(middleware::from_fn_with_state(
                Arc::new(state.clone()),
                api_key_middleware,
            ))
            .with_state(Arc::new(state));

        let addr = format!("{}:{}", self.config.host, self.config.port);

        info!("Starting API server on {}", addr);

        let listener = tokio::net::TcpListener::bind(&addr).await
            .map_err(|e| eyre::eyre!("Failed to bind to address: {}", e))?;
        
        axum::serve(listener, app)
            .await
            .map_err(|e| eyre::eyre!("Server error: {}", e))?;

        Ok(())
    }
}

/// API key validation middleware
async fn api_key_middleware(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Result<axum::response::Response, StatusCode> {
    // Skip authentication for health check
    if request.uri().path() == "/health" {
        return Ok(next.run(request).await);
    }

    // Check for API key in headers
    let api_key = headers
        .get("X-API-Key")
        .and_then(|h| h.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if api_key != state.config.api_key {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(next.run(request).await)
}

/// Health check endpoint
async fn health_check() -> impl IntoResponse {
    Json(ApiResponse {
        success: true,
        message: "API server is healthy".to_string(),
        data: None::<()>,
    })
}

/// Add a boundless user
async fn add_boundless_user(
    State(state): State<Arc<ApiState>>,
    Json(payload): Json<AddBoundlessUserRequest>,
) -> impl IntoResponse {
    // Validate Ethereum address format
    if !is_valid_ethereum_address(&payload.address) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<()> {
                success: false,
                message: "Invalid Ethereum address format".to_string(),
                data: None,
            }),
        ).into_response();
    }

    // Add user to database
    match state.db.add_boundless_user(&payload.address).await {
        Ok(_) => {
            info!("Added boundless user: {}", payload.address);
            (
                StatusCode::OK,
                Json(ApiResponse::<()> {
                    success: true,
                    message: format!("Successfully added boundless user: {}", payload.address),
                    data: None,
                }),
            ).into_response()
        }
        Err(e) => {
            error!("Failed to add boundless user {}: {:?}", payload.address, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()> {
                    success: false,
                    message: format!("Failed to add boundless user: {}", e),
                    data: None,
                }),
            ).into_response()
        }
    }
}

/// Remove a boundless user
async fn remove_boundless_user(
    State(state): State<Arc<ApiState>>,
    Path(address): Path<String>,
) -> impl IntoResponse {
    // Validate Ethereum address format
    if !is_valid_ethereum_address(&address) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<()> {
                success: false,
                message: "Invalid Ethereum address format".to_string(),
                data: None,
            }),
        ).into_response();
    }

    // Remove user from database
    match state.db.remove_boundless_user(&address).await {
        Ok(_) => {
            info!("Removed boundless user: {}", address);
            (
                StatusCode::OK,
                Json(ApiResponse::<()> {
                    success: true,
                    message: format!("Successfully removed boundless user: {}", address),
                    data: None,
                }),
            ).into_response()
        }
        Err(e) => {
            error!("Failed to remove boundless user {}: {:?}", address, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()> {
                    success: false,
                    message: format!("Failed to remove boundless user: {}", e),
                    data: None,
                }),
            ).into_response()
        }
    }
}

/// Check if a user is boundless
async fn is_boundless_user(
    State(state): State<Arc<ApiState>>,
    Path(address): Path<String>,
) -> impl IntoResponse {
    // Validate Ethereum address format
    if !is_valid_ethereum_address(&address) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<()> {
                success: false,
                message: "Invalid Ethereum address format".to_string(),
                data: None,
            }),
        ).into_response();
    }

    // Check if user is boundless
    match state.db.is_boundless_user(&address).await {
        Ok(is_boundless) => {
            (
                StatusCode::OK,
                Json(ApiResponse {
                    success: true,
                    message: format!("User {} is {}", address, if is_boundless { "boundless" } else { "not boundless" }),
                    data: Some(is_boundless),
                }),
            ).into_response()
        }
        Err(e) => {
            error!("Failed to check boundless status for {}: {:?}", address, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<bool> {
                    success: false,
                    message: format!("Failed to check boundless status: {}", e),
                    data: None,
                }),
            ).into_response()
        }
    }
}

/// Validates Ethereum address format
fn is_valid_ethereum_address(address: &str) -> bool {
    // Check length (0x + 40 hex characters)
    if address.len() != 42 {
        return false;
    }

    // Check prefix
    if !address.starts_with("0x") {
        return false;
    }

    // Check that remaining characters are hexadecimal
    address[2..].chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_ethereum_address() {
        assert!(is_valid_ethereum_address("0x0000000000000000000000000000000000000000"));
        assert!(is_valid_ethereum_address("0x1234567890abcdef1234567890abcdef12345678"));
    }

    #[test]
    fn test_invalid_ethereum_address() {
        assert!(!is_valid_ethereum_address("0x000000000000000000000000000000000000000")); // Too short
        assert!(!is_valid_ethereum_address("0000000000000000000000000000000000000000")); // No prefix
    }
} 
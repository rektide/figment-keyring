// SPDX-License-Identifier: MIT OR Apache-2.0

use thiserror::Error;

/// Result type for keyring operations.
pub type Result<T> = std::result::Result<T, KeyringError>;

/// Errors that can occur when accessing keyrings.
#[derive(Debug, Error)]
pub enum KeyringError {
    #[error("secret not found: {0}")]
    NotFound(String),

    #[error("keyring config error: {0}")]
    ConfigError(String),

    #[error("keyring service unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("permission denied")]
    PermissionDenied,

    #[error("backend error: {0}")]
    BackendError(String),
}

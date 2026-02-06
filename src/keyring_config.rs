// SPDX-License-Identifier: MIT OR Apache-2.0

use serde::{Deserialize, Serialize};

/// Identifies which keyring to use.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Keyring {
    /// Current user's keyring (default)
    User,
    /// System-wide keyring
    System,
    /// Custom named keyring
    #[serde(untagged)]
    Named(String),
}

impl Default for Keyring {
    fn default() -> Self {
        Keyring::User
    }
}

impl From<&str> for Keyring {
    fn from(s: &str) -> Self {
        match s {
            "user" => Keyring::User,
            "system" => Keyring::System,
            name => Keyring::Named(name.into()),
        }
    }
}

/// Configuration for keyring behavior.
/// Deserializable from any Figment source.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KeyringConfig {
    /// Application/service identifier for keyring entries
    pub service: String,

    /// Keyrings to search, in priority order.
    /// First keyring with the entry wins.
    #[serde(default = "default_keyrings")]
    pub keyrings: Vec<Keyring>,

    /// Don't fail if secret is not found in any keyring
    #[serde(default)]
    pub optional: bool,
}

fn default_keyrings() -> Vec<Keyring> {
    vec![Keyring::User]
}

pub mod backend {
    use crate::error::{KeyringError, Result};
    use crate::keyring_config::Keyring;

    extern crate keyring as keyring_crate;

    /// Get a secret from the specified keyring.
    pub fn get_secret(keyring: &Keyring, service: &str, username: &str) -> Result<String> {
        let entry = create_entry(keyring, service, username)?;
        let password = entry
            .get_password()
            .map_err(|e| KeyringError::BackendError(e.to_string()))?;
        Ok(password)
    }

    /// Create a keyring entry for the specified keyring type.
    fn create_entry(
        keyring: &Keyring,
        service: &str,
        username: &str,
    ) -> std::result::Result<keyring_crate::Entry, KeyringError> {
        let entry = match keyring {
            Keyring::User => keyring_crate::Entry::new(service, username)
                .map_err(|e| KeyringError::BackendError(e.to_string()))?,
            Keyring::System => {
                keyring_crate::Entry::new_with_target(&default_target(), service, username)
                    .map_err(|e| KeyringError::BackendError(e.to_string()))?
            }
            Keyring::Named(name) => keyring_crate::Entry::new_with_target(name, service, username)
                .map_err(|e| KeyringError::BackendError(e.to_string()))?,
        };
        Ok(entry)
    }

    fn default_target() -> String {
        #[cfg(target_os = "windows")]
        {
            "Windows Credential Manager".to_string()
        }
        #[cfg(target_os = "macos")]
        {
            "login.keychain".to_string()
        }
        #[cfg(target_os = "linux")]
        {
            "default".to_string()
        }
    }
}

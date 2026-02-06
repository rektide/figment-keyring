// SPDX-License-Identifier: MIT OR Apache-2.0

use serde::{Deserialize, Serialize};

/// Identifies which keyring to use.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Keyring {
    /// Current user's keyring (default)
    #[default]
    User,
    /// System-wide keyring
    System,
    /// Custom named keyring
    #[serde(untagged)]
    Named(String),
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
    use keyring_core::Entry;

    use std::sync::Once;
    static INIT: Once = Once::new();

    /// Get a secret from specified keyring.
    pub fn get_secret(keyring: &Keyring, service: &str, username: &str) -> Result<String> {
        ensure_native_store_initialized();
        let entry = create_entry(keyring, service, username)?;
        let password = entry
            .get_password()
            .map_err(|e| KeyringError::BackendError(e.to_string()))?;
        Ok(password)
    }

    fn ensure_native_store_initialized() {
        INIT.call_once(|| {
            keyring::use_native_store(false).expect("Failed to initialize native keyring store");
        });
    }

    /// Create a keyring entry for specified keyring type.
    fn create_entry(
        keyring: &Keyring,
        service: &str,
        username: &str,
    ) -> std::result::Result<Entry, KeyringError> {
        use std::collections::HashMap;

        let entry: Entry = match keyring {
            Keyring::User => Entry::new(service, username)
                .map_err(|e: keyring_core::Error| KeyringError::BackendError(e.to_string()))?,
            Keyring::System => {
                let target = default_target();
                let mut modifiers = HashMap::new();
                modifiers.insert("target", target.as_str());
                Entry::new_with_modifiers(service, username, &modifiers)
                    .map_err(|e: keyring_core::Error| KeyringError::BackendError(e.to_string()))?
            }
            Keyring::Named(name) => {
                let mut modifiers = HashMap::new();
                modifiers.insert("target", name.as_str());
                Entry::new_with_modifiers(service, username, &modifiers)
                    .map_err(|e: keyring_core::Error| KeyringError::BackendError(e.to_string()))?
            }
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

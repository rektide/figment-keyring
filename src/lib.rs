// SPDX-License-Identifier: MIT OR Apache-2.0

//! Figment Provider for Keyring Integration
//!
//! This crate provides a Figment2 provider that fetches secrets from system
//! keyrings (macOS Keychain, Windows Credential Manager, Linux Secret Service).
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use figment2::Figment;
//! use figment_keyring::KeyringProvider;
//!
//! // Create a Figment with your configuration sources
//! let config_figment = Figment::new();
//!
//! // Provider is configured by that Figment (late binding)
//! let api_key_provider = KeyringProvider::configured_by(config_figment, "api_key");
//!
//! // Final Figment merges everything
//! // let config: YourConfig = Figment::new()
//! //     .merge(config_figment)
//! //     .merge(api_key_provider)
//! //     .extract().unwrap();
//! ```
//!
//! # Configuration
//!
//! The provider is configured via a [`KeyringConfig`] which can come from
//! any Figment source (file, environment, etc.):
//!
//! ```toml
//! # config.toml
//! service = "myapp"
//! keyrings = ["user", "team-secrets"]
//! optional = false
//! ```

pub mod error;
pub mod keyring_config;

pub use error::KeyringError;
pub use keyring_config::{Keyring, KeyringConfig};

use figment2::{
    providers::Serialized,
    value::{Dict, Map, Value},
    Error, Figment, Metadata, Profile, Provider,
};
use std::sync::Arc;

/// Provider that fetches secrets from system keyrings.
///
/// This provider uses **late binding**: it holds a reference to a Figment
/// containing the configuration, but doesn't extract it until `.data()` is
/// called. This allows the configuration to be loaded from any Figment source
/// (files, environment, custom providers) that the application chooses.
///
/// # Example
///
/// ```rust,no_run
/// use figment2::Figment;
/// use figment_keyring::KeyringProvider;
///
/// // Create a Figment with your configuration sources
/// let config_figment = Figment::new();
///
/// let provider = KeyringProvider::configured_by(config_figment, "api_key");
/// ```
pub struct KeyringProvider {
    config_figment: Arc<Figment>,
    credential_name: String,
    config_key: Option<String>,
    profile: Option<Profile>,
}

impl KeyringProvider {
    pub fn configured_by(config_figment: Figment, credential_name: &str) -> Self {
        Self {
            config_figment: Arc::new(config_figment),
            credential_name: credential_name.into(),
            config_key: None,
            profile: None,
        }
    }

    pub fn new(service: &str, credential_name: &str) -> Self {
        let config = KeyringConfig {
            service: service.into(),
            keyrings: vec![Keyring::User],
            optional: false,
        };
        let figment = Figment::from(Serialized::defaults(config));
        Self::configured_by(figment, credential_name)
    }

    pub fn system(service: &str, credential_name: &str) -> Self {
        let config = KeyringConfig {
            service: service.into(),
            keyrings: vec![Keyring::System],
            optional: false,
        };
        let figment = Figment::from(Serialized::defaults(config));
        Self::configured_by(figment, credential_name)
    }

    pub fn as_key(mut self, key: &str) -> Self {
        self.config_key = Some(key.into());
        self
    }

    pub fn with_profile(mut self, profile: Profile) -> Self {
        self.profile = Some(profile);
        self
    }
}

impl Provider for KeyringProvider {
    fn metadata(&self) -> Metadata {
        Metadata::named("keyring")
    }

    fn data(&self) -> std::result::Result<Map<Profile, Dict>, Error> {
        let config: KeyringConfig = self
            .config_figment
            .extract()
            .map_err(|e| Error::from(format!("keyring config: {}", e)))?;

        let secret = self.search_keyrings(&config)?;

        let key = self.config_key.as_ref().unwrap_or(&self.credential_name);

        let profile = self.profile.clone().unwrap_or_default();
        let mut dict = Dict::new();

        match secret {
            Some(value) => {
                dict.insert(key.clone(), Value::from(value));
            }
            None if config.optional => {}
            None => {
                return Err(Error::from(format!(
                    "secret '{}' not found in any keyring",
                    self.credential_name
                )));
            }
        }

        let mut map = Map::new();
        map.insert(profile, dict);
        Ok(map)
    }
}

impl KeyringProvider {
    fn search_keyrings(
        &self,
        config: &KeyringConfig,
    ) -> std::result::Result<Option<String>, Error> {
        for keyring in &config.keyrings {
            match self.get_from_keyring(keyring, &config.service, &self.credential_name) {
                Ok(secret) => return Ok(Some(secret)),
                Err(KeyringError::NotFound(_)) => continue,
                Err(e) => {
                    if config.optional {
                        continue;
                    } else {
                        return Err(Error::from(e.to_string()));
                    }
                }
            }
        }
        Ok(None)
    }

    fn get_from_keyring(
        &self,
        keyring: &Keyring,
        service: &str,
        username: &str,
    ) -> std::result::Result<String, KeyringError> {
        keyring_config::backend::get_secret(keyring, service, username)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyring_from_str() {
        assert_eq!(Keyring::from("user"), Keyring::User);
        assert_eq!(Keyring::from("system"), Keyring::System);
        assert_eq!(
            Keyring::from("custom-keyring"),
            Keyring::Named("custom-keyring".into())
        );
    }

    #[test]
    fn test_keyring_default() {
        assert_eq!(Keyring::default(), Keyring::User);
    }

    #[test]
    fn test_keyring_provider_new() {
        let provider = KeyringProvider::new("test-app", "test-key");
        assert_eq!(provider.credential_name, "test-key");
    }

    #[test]
    fn test_keyring_provider_system() {
        let provider = KeyringProvider::system("test-app", "test-key");
        assert_eq!(provider.credential_name, "test-key");
    }

    #[test]
    fn test_keyring_provider_as_key() {
        let provider = KeyringProvider::new("test-app", "test-key").as_key("custom.config.key");
        assert_eq!(provider.config_key, Some("custom.config.key".into()));
    }

    #[test]
    fn test_keyring_provider_with_profile() {
        let profile = Profile::from("production");
        let provider = KeyringProvider::new("test-app", "test-key").with_profile(profile.clone());
        assert_eq!(provider.profile, Some(profile));
    }
}

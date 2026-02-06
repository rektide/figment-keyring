# figment-keyring

A Figment2 provider that fetches secrets from system keyrings.

[![Crates.io](https://img.shields.io/crates/v/figment-keyring)](https://crates.io/crates/figment-keyring)
[![docs.rs](https://img.shields.io/badge/docs.rs-figment-keyring)](https://docs.rs/figment-keyring)
[![MIT/Apache-2.0](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE-MIT%20OR%20Apache-2.0)
[![Rust 2021](https://img.shields.io/badge/rust-2021-orange)](https://www.rust-lang.org)

## Overview

`figment-keyring` provides a Figment2 provider that fetches secrets from system keyrings (macOS Keychain, Windows Credential Manager, Linux Secret Service). It uses **late binding** to configure keyring access via any Figment source (files, environment, custom providers).

## Features

- **Late binding**: Provider holds a Figment reference and extracts configuration at `.data()` time
- **Multi-keyring support**: Search user, system, and custom named keyrings in priority order
- **Flexible configuration**: Configure via TOML, JSON, environment variables, or any Figment provider
- **Optional secrets**: Gracefully handle missing secrets without errors
- **Profile support**: Target specific Figment profiles
- **Key mapping**: Map credential names to different config keys

## Installation

```toml
[dependencies]
figment-keyring = "0.1"
figment2 = { version = "0.11", features = ["env"] }
```

## Platform Support

The provider uses the `keyring` crate which supports multiple platforms:

| Platform      | Backend (keyring crate)                    | Status       |
|---------------|--------------------------------------------|--------------|
| macOS         | Keychain Services                          | Supported    |
| Windows       | Credential Manager                        | Supported    |
| Linux         | Secret Service / Keyutils                 | Supported    |
| iOS           | Keychain Services                          | Supported    |
| FreeBSD        | Secret Service                             | Supported    |
| OpenBSD        | Secret Service                             | Supported    |

## Usage

### Quick Start

```rust
use figment2::{Figment, providers::Serialized};
use figment_keyring::KeyringProvider;

// Simple: user keyring with defaults
let provider = KeyringProvider::new("myapp", "api_key");

let config: MyConfig = Figment::new()
    .merge(provider)
    .extract()
    .unwrap();
```

### Configuration File

Configure keyring behavior via any Figment source:

```toml
# config.toml
service = "myapp"
keyrings = ["user", "team-secrets", "system"]
optional = false
```

```rust
use figment2::{Figment, providers::{Format, Toml}};
use figment_keyring::KeyringProvider;

let config_figment = Figment::new()
    .merge(Toml::file("config.toml"));

let api_key_provider = KeyringProvider::configured_by(config_figment, "api_key");
```

### Multiple Secrets

```rust
use figment2::Figment;
use figment_keyring::KeyringProvider;

let config_figment = Figment::new()
    .merge(Toml::file("config.toml"));

let config = Figment::new()
    .merge(config_figment)
    .merge(KeyringProvider::configured_by(config_figment, "api_key"))
    .merge(KeyringProvider::configured_by(config_figment, "db_password"))
    .extract()
    .unwrap();
```

### Optional Secrets

```rust
use figment2::Figment;
use figment_keyring::KeyringProvider;

let config = KeyringConfig {
    service: "myapp".to_string(),
    keyrings: vec![Keyring::User],
    optional: true, // Don't fail if secret not found
};

let provider = KeyringProvider::configured_by(
    Figment::from(Serialized::defaults(config)),
    "api_key"
);
```

### Key Mapping

Map keyring entries to different config keys:

```rust
use figment2::Figment;
use figment_keyring::KeyringProvider;

let provider = KeyringProvider::configured_by(config_figment, "api_key")
    .as_key("credentials.password"); // Maps to "credentials.password" in config
```

## Configuration

### KeyringConfig

```rust
use serde::{Deserialize, Serialize};
use figment_keyring::{Keyring, KeyringConfig};

#[derive(Debug, Deserialize, Serialize)]
pub struct KeyringConfig {
    /// Application/service identifier for keyring entries
    pub service: String,

    /// Keyrings to search, in priority order
    #[serde(default = "default_keyrings")]
    pub keyrings: Vec<Keyring>,

    /// Don't fail if secret is not found in any keyring
    #[serde(default)]
    pub optional: bool,
}

fn default_keyrings() -> Vec<Keyring> {
    vec![Keyring::User]
}
```

### Keyring Types

```rust
use figment_keyring::Keyring;

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
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
```

## Platform Support

| Keyring      | macOS                         | Linux                          | Windows                   |
|--------------|--------------------------------|---------------------------------|----------------------------|
| User         | Login Keychain                | User Secret Service             | User Credential Manager     |
| System       | System Keychain                | System Secret Service            | Local Machine credentials     |
| Named(x)     | Keychain `x.keychain-db`    | Collection `x`                  | Target `x`                  |

## API

### KeyringProvider

```rust
pub struct KeyringProvider {
    /* private fields */
}

impl KeyringProvider {
    /// Create a provider configured by the given Figment
    pub fn configured_by(config_figment: Figment, credential_name: &str) -> Self;

    /// Simple constructor: user keyring, service name, credential name
    pub fn new(service: &str, credential_name: &str) -> Self;

    /// Use system keyring instead of user keyring
    pub fn system(service: &str, credential_name: &str) -> Self;

    /// Map keyring entry to a different config key name
    pub fn as_key(self, key: &str) -> Self;

    /// Target a specific Figment profile
    pub fn with_profile(self, profile: Profile) -> Self;
}

impl Provider for KeyringProvider {
    fn metadata(&self) -> Metadata;

    fn data(&self) -> Result<Map<Profile, Dict>, Error>;
}
```

## License

Licensed under either of

* MIT License ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>
* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>

at your option.

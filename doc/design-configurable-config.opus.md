# Configurable Configuration Keys Design

**Author**: Claude (opus)  
**Status**: Draft v1  
**Supersedes**: doc/design-configurable-config.md

---

## Problem Statement

The current `KeyringProvider` requires consumers to use our fixed field names:

```toml
# Consumer MUST use these exact names
service = "myapp"
keyrings = ["user"]
optional = false
```

**Consumers want to use their own configuration schema:**

```toml
# Their existing config structure
[secrets]
application = "myapp"
stores = ["user", "system"]
fail_silently = true
```

The core requirement from the original design still holds:

> App builders use whatever layered Figment config they want to drive our keyring provider.

We extended this to include: **App builders use whatever field names they want.**

---

## Design

### Insight

We already support late binding via `configured_by(figment)`. The natural extension is to also support **direct config injection** via `with_config(config)`.

```mermaid
flowchart TD
    subgraph "Path A: Our Schema"
        FIGMENT1[Consumer Figment]
        CB["configured_by(figment)"]
        EXTRACT["extract::<KeyringConfig>()"]
        FIGMENT1 --> CB --> EXTRACT
    end
    
    subgraph "Path B: Their Schema"
        FIGMENT2[Consumer Figment]
        THEIR["extract::<TheirConfig>()"]
        CONVERT["Into<KeyringConfig>"]
        WC["with_config(config)"]
        FIGMENT2 --> THEIR --> CONVERT --> WC
    end
    
    EXTRACT --> PROVIDER[KeyringProvider]
    WC --> PROVIDER
```

### API Addition

```rust
impl KeyringProvider {
    /// Create a provider with an explicit KeyringConfig.
    /// 
    /// Use this when you want full control over configuration schema.
    /// Extract config using your own field names, convert to KeyringConfig,
    /// and pass it here.
    /// 
    /// # Example
    /// 
    /// ```rust
    /// use figment2::Figment;
    /// use figment_keyring::{KeyringProvider, KeyringConfig, Keyring};
    /// 
    /// // Your own config structure with your own field names
    /// #[derive(Deserialize)]
    /// struct MySecretsConfig {
    ///     application: String,
    ///     stores: Vec<String>,
    ///     #[serde(default)]
    ///     fail_silently: bool,
    /// }
    /// 
    /// impl From<MySecretsConfig> for KeyringConfig {
    ///     fn from(c: MySecretsConfig) -> Self {
    ///         KeyringConfig {
    ///             service: c.application,
    ///             keyrings: c.stores.into_iter().map(|s| Keyring::from(s.as_str())).collect(),
    ///             optional: c.fail_silently,
    ///         }
    ///     }
    /// }
    /// 
    /// // Extract with your schema, convert, use
    /// let my_config: MySecretsConfig = figment.focus("secrets").extract()?;
    /// let provider = KeyringProvider::with_config(my_config.into(), "api_key");
    /// ```
    pub fn with_config(config: KeyringConfig, credential_name: &str) -> Self {
        let figment = Figment::from(Serialized::defaults(config));
        Self::configured_by(figment, credential_name)
    }
}
```

### Usage Patterns

#### Pattern 1: Use Our Schema (Existing)

For consumers who are happy with `service`, `keyrings`, `optional`:

```toml
# config.toml
[keyring]
service = "myapp"
keyrings = ["user"]
optional = false
```

```rust
let provider = KeyringProvider::configured_by(figment, "api_key")
    .focused("keyring");
```

#### Pattern 2: Bring Your Own Schema (New)

For consumers with existing config structures:

```toml
# config.toml
[secrets]
application = "myapp"
stores = ["user", "system"]
fail_silently = true
```

```rust
#[derive(Deserialize)]
struct SecretsConfig {
    application: String,
    stores: Vec<String>,
    #[serde(default)]
    fail_silently: bool,
}

impl From<SecretsConfig> for KeyringConfig {
    fn from(c: SecretsConfig) -> Self {
        KeyringConfig {
            service: c.application,
            keyrings: c.stores.into_iter()
                .map(|s| Keyring::from(s.as_str()))
                .collect(),
            optional: c.fail_silently,
        }
    }
}

let secrets: SecretsConfig = figment.focus("secrets").extract()?;
let provider = KeyringProvider::with_config(secrets.into(), "api_key");
```

#### Pattern 3: Inline Construction

For simple programmatic cases:

```rust
let provider = KeyringProvider::with_config(
    KeyringConfig {
        service: "myapp".into(),
        keyrings: vec![Keyring::User, Keyring::System],
        optional: false,
    },
    "api_key"
);
```

---

## Flow Comparison

```mermaid
sequenceDiagram
    participant App
    participant Figment
    participant Provider
    participant Keyring
    
    Note over App,Keyring: Path A: configured_by (our schema)
    App->>Figment: Build with File, Env, etc.
    App->>Provider: configured_by(figment, "api_key")
    Note over Provider: Stores figment reference
    App->>Provider: .data()
    Provider->>Figment: extract::<KeyringConfig>()
    Note over Provider: Uses our field names
    Provider->>Keyring: get_secret()
    
    Note over App,Keyring: Path B: with_config (their schema)
    App->>Figment: Build with File, Env, etc.
    App->>Figment: extract::<TheirConfig>()
    Note over App: Uses their field names
    App->>App: TheirConfig.into() -> KeyringConfig
    App->>Provider: with_config(config, "api_key")
    App->>Provider: .data()
    Provider->>Keyring: get_secret()
```

---

## Why This Works

### 1. Zero Breaking Changes

`configured_by()` continues to work exactly as before. Existing consumers are unaffected.

### 2. Full Schema Freedom

Consumers can use any field names, any nesting structure, any serde attributes:

```rust
#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
struct LegacyConfig {
    #[serde(alias = "app")]
    service_name: String,
    
    #[serde(default = "default_stores")]
    credential_stores: Vec<String>,
}
```

### 3. Type Safety

The `From` trait implementation is checked at compile time. No runtime string matching or reflection.

### 4. Composable

Works with any Figment configuration:

```rust
// From environment
let cfg: MyConfig = Figment::new()
    .merge(Env::prefixed("MYAPP_"))
    .extract()?;

// From CLI args
let cfg: MyConfig = Figment::from(Serialized::defaults(cli_args))
    .extract()?;

// From remote config service
let cfg: MyConfig = Figment::from(RemoteConfigProvider::new())
    .extract()?;

// All work the same way
let provider = KeyringProvider::with_config(cfg.into(), "api_key");
```

---

## Implementation

```rust
impl KeyringProvider {
    /// Create a provider with an explicit KeyringConfig.
    ///
    /// This is the "bring your own schema" constructor. Extract configuration
    /// using your own struct with your own field names, implement
    /// `From<YourConfig> for KeyringConfig`, and pass it here.
    ///
    /// The config is wrapped in a Figment internally to maintain the
    /// late-binding architecture, but no further extraction occurs.
    pub fn with_config(config: KeyringConfig, credential_name: &str) -> Self {
        let figment = Figment::from(Serialized::defaults(config));
        Self::configured_by(figment, credential_name)
    }
}
```

That's it. One method, three lines of implementation.

---

## Documentation Example

Add to README or lib.rs:

```rust
//! ## Custom Configuration Schema
//!
//! If you have an existing configuration structure and don't want to use
//! our field names (`service`, `keyrings`, `optional`), use [`with_config`]:
//!
//! ```rust
//! use serde::Deserialize;
//! use figment2::Figment;
//! use figment_keyring::{KeyringProvider, KeyringConfig, Keyring};
//!
//! #[derive(Deserialize)]
//! struct MyConfig {
//!     app_name: String,
//!     secret_backends: Vec<String>,
//!     #[serde(default)]
//!     ignore_missing: bool,
//! }
//!
//! impl From<MyConfig> for KeyringConfig {
//!     fn from(c: MyConfig) -> Self {
//!         KeyringConfig {
//!             service: c.app_name,
//!             keyrings: c.secret_backends.into_iter()
//!                 .map(|s| Keyring::from(s.as_str()))
//!                 .collect(),
//!             optional: c.ignore_missing,
//!         }
//!     }
//! }
//!
//! let my_cfg: MyConfig = figment.focus("secrets").extract()?;
//! let provider = KeyringProvider::with_config(my_cfg.into(), "database_password");
//! ```
```

---

## Implementation Checklist

### P0 (Required)

- [ ] Add `KeyringProvider::with_config(config, credential_name)` method
- [ ] Add documentation with custom schema example
- [ ] Add test for `with_config` constructor

### P1 (Documentation)

- [ ] README section on custom configuration schemas
- [ ] Example showing `From` trait implementation
- [ ] Example showing environment variable mapping

---

## Alternatives Considered

### Runtime Field Aliases

```rust
pub struct FieldAliases {
    pub service: Option<String>,
    pub keyrings: Option<String>,
    pub optional: Option<String>,
}
```

**Rejected**: Requires custom deserialization logic, runtime string matching, more complex error handling. The `with_config` approach is simpler and more flexible.

### Macro-Based Schema Generation

```rust
keyring_config! {
    service => "app_name",
    keyrings => "stores",
    optional => "fail_silently",
}
```

**Rejected**: Adds complexity, requires proc-macro, less flexible than letting consumers define their own structs.

### Generic Config Type

```rust
impl<C: Into<KeyringConfig>> KeyringProvider {
    pub fn configured_by_custom<C>(figment: Figment, credential_name: &str) -> Self
    where
        C: DeserializeOwned + Into<KeyringConfig>
    { ... }
}
```

**Rejected**: Requires turbofish syntax, less ergonomic than explicit `with_config`.

---

## Conclusion

Add `with_config(config, credential_name)` to let consumers use their own configuration schema. They define a struct with their field names, implement `From<TheirConfig> for KeyringConfig`, extract with Figment, convert, and pass to us.

Simple, type-safe, zero breaking changes.

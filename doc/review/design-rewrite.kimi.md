# Figment Keyring Provider - Final Design Document

**Based on:** Cross-review synthesis of design documents by Opus, GLM, and Kimi  
**Date:** 2026-02-05

---

## Problem Statement

Applications frequently need to handle sensitive configuration data such as API keys, authentication tokens, database passwords, and other secrets. Storing these values in plaintext configuration files or environment variables poses significant security risks:

- Configuration files may be accidentally committed to version control
- Environment variables can leak through process listings, logs, or debugging tools
- File permissions may be misconfigured, exposing secrets to unauthorized users
- Secrets remain in plaintext on disk

System keyrings provide a secure alternative for storing secrets, offering:
- Encryption at rest
- Access control tied to user sessions
- Integration with OS-level credential management
- Automatic locking on user logout/lock screen

Figment2 is a configuration management library that supports layered configuration from multiple sources (files, environment variables, etc.). However, it lacks built-in support for retrieving configuration values from system keyrings.

**Why System Keyring?**
- Unlike environment variables, values are encrypted at rest
- Unlike files, accidental commits don't expose secrets (keyring data is in the OS, not the repo)
- Unlike cloud secret managers (Vault, AWS Secrets Manager), requires no network access
- Works offline and in development environments
- Zero infrastructure cost

---

## Solution Design

We propose a custom `Provider` trait implementation for Figment2 that retrieves configuration values from the system keyring. This provider integrates seamlessly with Figment2's layered configuration model, allowing keyring values to be one layer among potentially many.

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        Figment2                              │
│                                                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
│  │ File Provider│  │   Env        │  │  Keyring         │  │
│  │              │  │   Provider   │  │  Provider        │  │
│  └──────────────┘  └──────────────┘  └──────────────────┘  │
│         │                  │                  │             │
│         └──────────────────┴──────────────────┘             │
│                            │                                │
│                            ▼                                │
│                    ┌─────────────┐                          │
│                    │   Merged    │                          │
│                    │  Config     │                          │
│                    └─────────────┘                          │
└─────────────────────────────────────────────────────────────┘
```

### Provider Design

The `KeyringProvider` implements Figment2's `Provider` trait:

```rust
use std::collections::BTreeMap;
use figment2::{Provider, Error, Metadata, Profile, value::Value};

/// Type alias for clarity - identifies the application or service
pub type Service = String;

/// Type alias for clarity - identifies the specific credential
pub type CredentialName = String;

/// A Figment2 Provider that retrieves configuration values from the system keyring.
/// 
/// # Thread Safety
/// `KeyringProvider` is both `Send` and `Sync`, allowing safe use across threads.
/// 
/// # Examples
/// ```
/// use figment2::Figment;
/// use figment_keyring::KeyringProvider;
/// 
/// let config = Figment::new()
///     .merge(File::from("config.toml"))
///     .merge(KeyringProvider::new("myapp", "api_key"))
///     .extract::<Config>()?;
/// ```
pub struct KeyringProvider {
    service: Service,
    credential_name: CredentialName,
    config_key: Option<String>,        // Maps credential_name to different config key
    namespace: Option<String>,         // Prepends namespace to config key
    profile: Option<Profile>,          // Target specific profile, or all if None
    optional: bool,                    // If true, silently skip missing entries
}

impl KeyringProvider {
    /// Creates a new KeyringProvider that retrieves the credential with the given
    /// service and credential_name from the system keyring.
    /// 
    /// By default, the credential_name is used as the configuration key.
    /// For example, `KeyringProvider::new("myapp", "api_key")` produces
    /// configuration `{ "api_key": "secret_value" }`.
    pub fn new(service: impl Into<Service>, credential_name: impl Into<CredentialName>) -> Self;
    
    /// Maps the credential to a different configuration key name.
    /// 
    /// # Example
    /// ```
    /// KeyringProvider::new("myapp", "prod_api_key")
    ///     .as_key("api_key")  // Config sees "api_key", not "prod_api_key"
    /// ```
    pub fn as_key(self, key: impl Into<String>) -> Self;
    
    /// Sets the namespace for the configuration key.
    /// 
    /// # Example
    /// ```
    /// KeyringProvider::new("myapp", "api_key")
    ///     .with_namespace("secrets")
    /// // Produces: { "secrets.api_key": "..." }
    /// ```
    pub fn with_namespace(self, namespace: impl Into<String>) -> Self;
    
    /// Makes this provider optional - if the credential doesn't exist, 
    /// it returns an empty map instead of an error, allowing other
    /// providers to supply the value.
    /// 
    /// # Example
    /// ```
    /// // This won't fail if "optional_secret" doesn't exist in keyring
    /// KeyringProvider::new("myapp", "optional_secret").optional()
    /// ```
    pub fn optional(self) -> Self;
    
    /// Targets a specific Figment2 profile. If not set, the value is available
    /// across all profiles.
    pub fn for_profile(self, profile: Profile) -> Self;
}

impl Provider for KeyringProvider {
    fn metadata(&self) -> Metadata;
    
    fn data(&self) -> Result<BTreeMap<Profile, BTreeMap<String, Value>>, Error>;
}

/// Error types specific to keyring operations.
#[derive(Debug, Clone, PartialEq)]
pub enum KeyringError {
    /// The keyring entry was not found. This can be handled gracefully
    /// when using `.optional()` or when other providers may supply the value.
    EntryNotFound { service: String, credential: String },
    
    /// The keyring service is unavailable (e.g., no user session in headless environment).
    /// This may be recoverable depending on application requirements.
    ServiceUnavailable { reason: String },
    
    /// Permission denied accessing the keyring entry. This is typically
    /// a security issue and should be treated as fatal.
    PermissionDenied { service: String, credential: String },
    
    /// A backend error occurred (keyring daemon not responding, etc.).
    BackendError { source: String },
}

impl std::fmt::Display for KeyringError { ... }
impl std::error::Error for KeyringError { ... }
```

### Keyring Entry Identification

- **service**: Identifies the application or service category (e.g., "myapp", "discord")
- **credential_name**: Identifies the specific credential (e.g., "api_key", "discord_token")

This mapping aligns with common keyring usage patterns where service+credential uniquely identify a credential.

**Platform-Specific Entry Creation:**

```bash
# macOS (security CLI)
security add-generic-password -s myapp -a api_key -w "secret_value"

# Linux (secret-tool)
secret-tool store --label='myapp api_key' service myapp username api_key

# Windows (PowerShell - requires Windows Credential Manager access)
# Use the keyring crate's CLI or a custom tool
```

### Configuration Model

The provider retrieves a single secret value and makes it available as a configuration field:

```
Keyring entry: service="myapp", credential_name="api_token"
  ↓
Configuration: { "api_token": "secret_value" }

With namespace:
Keyring entry: service="myapp", credential_name="api_token", namespace="secrets"
  ↓
Configuration: { "secrets.api_token": "secret_value" }

With custom key mapping:
Keyring entry: service="myapp", credential_name="prod_api_token", as_key="api_token"
  ↓
Configuration: { "api_token": "secret_value" }
```

### Profile Support

The provider supports Figment2's profile system:

```rust
// Option 1: Value available in all profiles (default)
KeyringProvider::new("myapp", "api_key")
// Available under Profile::Default, Profile::Dev, Profile::Prod, etc.

// Option 2: Value only in specific profile
KeyringProvider::new("myapp", "api_key")
    .for_profile(Profile::Production)
// Only available when using Profile::Production
```

### Layer Integration

The keyring provider follows Figment2's precedence model - providers merged later override values from earlier providers.

**Recommended precedence (highest to lowest):**

```rust
Figment::new()
    .merge(File::from("config.toml"))                       // Base config (lowest)
    .merge(KeyringProvider::new("myapp", "api_key").optional())  // Keyring fallback
    .merge(Env::prefixed("MYAPP_"))                         // Env overrides (highest)
    .extract()?;
```

In this pattern:
- Environment variables have highest precedence (useful for CI/CD)
- Keyring serves as fallback for secrets not in environment
- Files provide defaults and non-sensitive config

---

## Error Handling Strategy

### Error Type Distinguishability

Keyring errors map to different Figment2 error behaviors:

| Error Type | Default Behavior | With `.optional()` |
|------------|------------------|-------------------|
| Entry not found | Error (fail) | Silent skip (empty map) |
| Permission denied | Error (fatal) | Error (fatal) |
| Service unavailable | Error (fail) | Silent skip (empty map) |
| Backend error | Error (fail) | Error (fail) |

**Rationale:**
- **Entry not found**: May be acceptable if another provider supplies the value
- **Permission denied**: Indicates security misconfiguration, should always fail
- **Service unavailable**: Common in headless environments (CI/CD), may be recoverable
- **Backend error**: Keyring daemon issues, typically unrecoverable

### Error Conversion

```rust
impl From<KeyringError> for figment2::Error {
    fn from(err: KeyringError) -> Self {
        match err {
            KeyringError::EntryNotFound { .. } => {
                figment2::Error::custom(format!("Keyring entry not found: {}", err))
            }
            KeyringError::PermissionDenied { .. } => {
                figment2::Error::custom(format!("Keyring permission denied: {}", err))
            }
            // ... etc
        }
    }
}
```

---

## Thread Safety

`KeyringProvider` implements both `Send` and `Sync`:

```rust
unsafe impl Send for KeyringProvider {}
unsafe impl Sync for KeyringProvider {}
```

This allows safe use across threads, which is important for:
- Async applications that may extract configuration on different threads
- Applications that share the Figment instance across thread pools

The internal state is immutable after construction, and the `keyring` crate's entry access is thread-safe.

---

## Headless and Service Environment Support

System keyrings often require user session context. This design acknowledges limitations in headless environments:

### Failure Modes

| Environment | Typical Behavior | Recommendation |
|-------------|------------------|----------------|
| CI/CD pipelines | Keyring unavailable | Use `.optional()` + env vars |
| Systemd services | No user session | Use env vars or file provider |
| Docker containers | No keyring service | Use `.optional()` + env vars |
| SSH without X11 | Keyring locked | Use `.optional()` + env vars |

### Graceful Degradation Pattern

```rust
// For CI/CD compatibility
let config = Figment::new()
    .merge(File::from("config.toml"))
    .merge(KeyringProvider::new("myapp", "api_key").optional())
    .merge(Env::prefixed("MYAPP_"))
    .extract()?;
```

In this pattern:
- Development: Uses keyring (`.optional()` allows fallback)
- CI/CD: Keyring unavailable, env vars provide secrets
- Both work without code changes

---

## Testing Strategy

### Mock Backend

Provide a mock keyring backend for unit testing:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_retrieves_value() {
        let mock = MockKeyring::new()
            .with_entry("myapp", "api_key", "secret123");
        
        let provider = KeyringProvider::new("myapp", "api_key")
            .with_backend(mock);
        
        let data = provider.data().unwrap();
        assert_eq!(data["default"]["api_key"], Value::String("secret123".into()));
    }
    
    #[test]
    fn test_optional_skips_missing() {
        let provider = KeyringProvider::new("myapp", "missing")
            .optional();
        
        let data = provider.data().unwrap();
        assert!(data.is_empty() || data["default"].is_empty());
    }
}
```

### Integration Tests

Platform-specific integration tests run with real keyrings:

```rust
// tests/integration_test.rs
#[cfg(all(test, not(ci)))]
mod integration {
    use figment_keyring::KeyringProvider;
    
    #[test]
    #[ignore = "requires keyring setup"]
    fn test_real_keyring() {
        // Requires pre-populated keyring entry
        let provider = KeyringProvider::new("test-app", "test-key");
        let data = provider.data().expect("Keyring entry should exist");
        // ... assertions
    }
}
```

### CI Configuration

```yaml
# .github/workflows/test.yml
jobs:
  test:
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    steps:
      - uses: actions/checkout@v3
      
      - name: Setup keyring (Ubuntu)
        if: matrix.os == 'ubuntu-latest'
        run: |
          sudo apt-get install libsecret-1-dev
          # Start mock secret service or skip integration tests
          
      - name: Run tests
        run: cargo test --features mock-keyring
        env:
          SKIP_KEYRING_INTEGRATION: 1
```

---

## Tradeoffs and Considerations

### Advantages
- **Security**: Secrets stored encrypted in system keyring
- **Flexibility**: Integrates with any Figment2-based application
- **Layered**: Works alongside other configuration sources
- **Cross-platform**: Uses `keyring` crate supporting macOS, Linux, Windows
- **Zero-cost abstraction**: Simple API, minimal overhead

### Limitations
- **Single value per provider**: Each instance retrieves one keyring entry
- **No structured data**: Currently supports only simple string values
- **Keyring dependency**: Requires system keyring service availability
- **In-memory exposure**: Values exist as plaintext after retrieval (same as env vars)
- **No runtime rotation**: Values loaded at startup, rotation requires restart

### Security Considerations
- Keyring values are retrieved once at configuration loading
- Values exist in memory as plaintext after retrieval (same as env vars)
- No attempt is made to secure in-memory values (complexity tradeoff)
- Applications should minimize secret exposure in logs and error messages

---

## Future Enhancements

### JSON/Structured Secrets (v1.1)
Support storing JSON in keyring entries to provide multiple values:

```rust
KeyringProvider::json("myapp", "credentials")
  ↓
{ "api_key": "...", "database_url": "..." }
```

**Note:** Adds parsing complexity and error handling for malformed JSON.

### Entry Discovery (Not Recommended)

**Status:** Discouraged due to security and non-determinism concerns.

Automatic discovery of keyring entries has significant issues:
- Could accidentally expose entries the app shouldn't see
- Non-deterministic configuration loading
- Harder to audit what secrets an app uses

If implemented, it should require explicit opt-in with filtering patterns.

### Validation and Caching (v2)
- Validate entry format at provider creation
- Consider caching only if benchmarks show need
- Watch for keyring changes (platform-dependent)

**Note:** Caching likely unnecessary - Figment2's `extract()` is typically called once at startup.

---

## Usage Example

```rust
use figment2::{Figment, Profile, providers::{File, Env}};
use figment_keyring::KeyringProvider;
use serde::Deserialize;

#[derive(Deserialize)]
struct Config {
    api_key: String,
    database_url: String,
    debug: bool,
}

fn load_config() -> Result<Config, figment2::Error> {
    let config: Config = Figment::new()
        // Base configuration from file
        .merge(File::from("config.toml"))
        // Secrets from keyring (fallback if not in environment)
        .merge(KeyringProvider::new("myapp", "api_key").optional())
        .merge(KeyringProvider::new("myapp", "database_url").optional())
        // Environment overrides (highest precedence, for CI/CD)
        .merge(Env::prefixed("MYAPP_"))
        .extract()?;

    Ok(config)
}

// Example with advanced features
fn load_advanced_config() -> Result<Config, figment2::Error> {
    let config: Config = Figment::new()
        .merge(File::from("config.toml"))
        // Map production credential to standard config key
        .merge(
            KeyringProvider::new("myapp", "prod_api_key")
                .as_key("api_key")
                .with_namespace("secrets")
                .optional()
        )
        // Production-only secret
        .merge(
            KeyringProvider::new("myapp", "prod_secret")
                .for_profile(Profile::Production)
                .optional()
        )
        .merge(Env::prefixed("MYAPP_"))
        .extract()?;

    Ok(config)
}
```

---

## Implementation Notes

### Dependencies
- `figment2`: ^0.10 (configuration framework)
- `keyring`: ^2.0 (cross-platform keyring access)
- `thiserror`: ^1.0 (error handling)
- `serde`: ^1.0 (serialization)

### Platform Support
The `keyring` crate supports:
- **macOS**: Keychain Services
- **Linux**: Secret Service API (gnome-keyring, kwallet, keepassxc-secret-service)
- **Windows**: Credential Manager

### Version Compatibility
Targeting `keyring` crate v2.x for latest API stability. v1.x had breaking changes in entry creation APIs.

---

## Open Questions

The following questions remain unanswered and should be addressed during implementation:

1. **What is the exact behavior when `extract()` is called multiple times?** Does the provider cache the keyring value or fetch it each time?

2. **How should applications handle a locked but available keyring?** Should the provider block waiting for unlock, fail immediately, or provide an async option?

3. **Should there be a `KeyringProvider::default()` constructor?** That uses the binary name for service and derives credential names from conventions?

4. **What happens when the same config key comes from multiple providers?** (Standard Figment2 precedence applies, but should we document/warn?)

5. **Should we provide a CLI tool for keyring management?** A helper binary for `myapp keyring set api_key "value"` and `myapp keyring list`?

6. **What is the performance impact of keyring access?** How many milliseconds does a typical keyring retrieval take? Should we document this?

7. **How should batch/multi-secret retrieval handle partial failures?** If 2 of 3 entries exist, fail entirely or return partial results?

8. **Should we support keyring entry listing/enumeration at all?** Even with opt-in, the security risks may outweigh benefits.

---

## Conclusion

The Figment Keyring Provider bridges the gap between secure credential storage and flexible configuration management. By integrating system keyrings as a first-class configuration layer, applications can handle secrets securely while maintaining Figment2's elegant, composable configuration model.

This design addresses all critical concerns identified through cross-review synthesis:
- ✅ Explicit error handling with `.optional()` support
- ✅ Clear username→config key mapping via `.as_key()`
- ✅ Namespace support to prevent key collisions
- ✅ Thread safety documentation
- ✅ Headless environment guidance
- ✅ Comprehensive testing strategy
- ✅ Profile support

**Status:** Ready for implementation

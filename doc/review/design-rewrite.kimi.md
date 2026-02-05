# Figment Keyring Provider - Final Design Document (v2)

**Based on:** Cross-review synthesis + design-rewrite-critique  
**Date:** 2026-02-05  
**Key Change:** Multi-keyring support with user/system as first-class citizens

---

## Problem Statement

Applications frequently need to handle sensitive configuration data such as API keys, authentication tokens, database passwords, and other secrets. Storing these values in plaintext configuration files or environment variables poses significant security risks.

System keyrings provide a secure alternative for storing secrets. However, modern systems often have **multiple keyrings** available:
- **User keyring**: Per-user credentials (development, personal)
- **System keyring**: System-wide credentials (shared services, production)
- **Application-specific keyrings**: Scoped to specific services

Applications need a way to search across these keyrings with configurable precedence, while maintaining a simple, composable API.

**Why System Keyring?**
- Unlike environment variables, values are encrypted at rest
- Unlike files, accidental commits don't expose secrets
- Unlike cloud secret managers, requires no network access
- Works offline and in development environments
- Zero infrastructure cost

---

## Solution Design

We propose a custom `Provider` trait implementation for Figment2 that retrieves configuration values from **one or more** system keyrings. This provider integrates seamlessly with Figment2's layered configuration model and allows searching across multiple keyrings with configurable search order.

### Key Design Decisions

1. **Default to User Keyring**: By default, only search the user's keyring for security and predictability
2. **Composable Keyring Selection**: Use Figment2 itself to configure which keyrings to search
3. **User and System as First-Class**: Special, easy-to-use constants for the two most common keyrings
4. **Search Order Matters**: First match wins - order of keyrings in configuration determines precedence
5. **Simple First**: Primary provider is simple and easy to understand; lazy loading is a secondary/future enhancement

### Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│                        Figment2                                   │
│                                                                  │
│  ┌──────────────┐  ┌──────────────────────────────────────┐    │
│  │  Config      │  │        Keyring Provider               │    │
│  │  Sources     │  │                                      │    │
│  │              │  │  ┌──────────┐  ┌──────────┐          │    │
│  │  - Files     │  │  │  User    │  │  System  │  ...     │    │
│  │  - Env       │  │  │ Keyring  │  │ Keyring  │          │    │
│  │  - Keyring   │  │  └────┬─────┘  └────┬─────┘          │    │
│  │              │  │       └───────────┬──┘               │    │
│  └──────────────┘  │                   │                  │    │
│         │          │                   ▼                  │    │
│         └──────────┴──────────────┬──────────────┐        │    │
│                                   │   Merged   │        │    │
│                                   │   Config   │        │    │
│                                   └──────────────┘        │    │
└──────────────────────────────────────────────────────────────────┘
```

### Core Provider Design

The `KeyringProvider` implements Figment2's `Provider` trait:

```rust
use std::collections::BTreeMap;
use figment2::{Provider, Error, Metadata, Profile, value::Value};

/// Identifies a keyring to search for credentials.
/// 
/// # Special Keyrings
/// - `Keyring::User`: The current user's keyring (default)
/// - `Keyring::System`: The system-wide keyring
/// - `Keyring::Named("custom")`: Named application-specific keyring
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Keyring {
    /// The user's personal keyring (default)
    /// 
    /// On macOS: Login keychain
    /// On Linux: Default Secret Service collection (typically "session" or "default")
    /// On Windows: Current user's credential vault
    User,
    
    /// The system-wide keyring
    /// 
    /// On macOS: System keychain (requires admin)
    /// On Linux: System Secret Service collection
    /// On Windows: Local system credential vault
    System,
    
    /// A named keyring (platform-specific)
    /// 
    /// On Linux: Secret Service collection name
    /// Other platforms: May map to file-based or other storage
    Named(String),
}

impl Keyring {
    /// Returns the default keyring (User)
    pub fn default() -> Self {
        Keyring::User
    }
}

/// Type alias for clarity - identifies the application or service
pub type Service = String;

/// Type alias for clarity - identifies the specific credential
pub type CredentialName = String;

/// A Figment2 Provider that retrieves configuration values from system keyrings.
/// 
/// By default, searches only the User keyring. Use `.search()` to search
/// multiple keyrings in order - first match wins.
/// 
/// # Thread Safety
/// `KeyringProvider` is both `Send` and `Sync`, allowing safe use across threads.
/// 
/// # Examples
/// 
/// Simple usage (User keyring only):
/// ```
/// use figment2::Figment;
/// use figment_keyring::KeyringProvider;
/// 
/// let config = Figment::new()
///     .merge(KeyringProvider::new("myapp", "api_key"))
///     .extract::<Config>()?;
/// ```
/// 
/// Search multiple keyrings (User first, then System):
/// ```
/// use figment2::Figment;
/// use figment_keyring::{KeyringProvider, Keyring};
/// 
/// let config = Figment::new()
///     .merge(
///         KeyringProvider::new("myapp", "api_key")
///             .search(&[Keyring::User, Keyring::System])
///     )
///     .extract::<Config>()?;
/// ```
pub struct KeyringProvider {
    service: Service,
    credential_name: CredentialName,
    keyrings: Vec<Keyring>,       // Search order: first match wins
    config_key: Option<String>,   // Maps credential_name to different config key
    namespace: Option<String>,    // Prepends namespace to config key
    profile: Option<Profile>,     // Target specific profile, or all if None
    optional: bool,               // If true, silently skip missing entries
}

impl KeyringProvider {
    /// Creates a new KeyringProvider that searches the User keyring for the 
    /// credential with the given service and credential_name.
    /// 
    /// By default, only searches the User keyring. Use `.search()` to search
    /// additional keyrings.
    /// 
    /// The credential_name is used as the configuration key.
    /// For example, `KeyringProvider::new("myapp", "api_key")` produces
    /// configuration `{ "api_key": "secret_value" }`.
    pub fn new(service: impl Into<Service>, credential_name: impl Into<CredentialName>) -> Self {
        Self {
            service: service.into(),
            credential_name: credential_name.into(),
            keyrings: vec![Keyring::User],  // Default: User keyring only
            config_key: None,
            namespace: None,
            profile: None,
            optional: false,
        }
    }
    
    /// Sets the keyrings to search, in order of precedence.
    /// 
    /// The provider searches each keyring in order and returns the first match.
    /// If no keyring contains the credential, the behavior depends on the
    /// `.optional()` setting.
    /// 
    /// # Example
    /// ```
    /// // Search User keyring first, fall back to System
    /// KeyringProvider::new("myapp", "api_key")
    ///     .search(&[Keyring::User, Keyring::System])
    /// 
    /// // Search only System keyring
    /// KeyringProvider::new("myapp", "api_key")
    ///     .search(&[Keyring::System])
    /// 
    /// // Search multiple named keyrings (Linux Secret Service collections)
    /// KeyringProvider::new("myapp", "api_key")
    ///     .search(&[
    ///         Keyring::Named("production".to_string()),
    ///         Keyring::User,
    ///     ])
    /// ```
    pub fn search(self, keyrings: &[Keyring]) -> Self {
        self.keyrings = keyrings.to_vec();
        self
    }
    
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
    
    /// Makes this provider optional - if the credential doesn't exist in any
    /// searched keyring, it returns an empty map instead of an error, allowing
    /// other providers to supply the value.
    /// 
    /// # Example
    /// ```
    /// // This won't fail if "optional_secret" doesn't exist in any keyring
    /// KeyringProvider::new("myapp", "optional_secret").optional()
    /// ```
    pub fn optional(self) -> Self;
    
    /// Targets a specific Figment2 profile. If not set, the value is available
    /// across all profiles.
    pub fn for_profile(self, profile: Profile) -> Self;
}

impl Provider for KeyringProvider {
    fn metadata(&self) -> Metadata;
    
    fn data(&self) -> Result<BTreeMap<Profile, BTreeMap<String, Value>>, Error> {
        // Search keyrings in order, return first match
        for keyring in &self.keyrings {
            match self.try_get_from_keyring(keyring) {
                Ok(value) => return Ok(self.build_result(value)),
                Err(KeyringError::EntryNotFound { .. }) => continue, // Try next keyring
                Err(e) if self.optional => return Ok(BTreeMap::new()), // Optional, return empty
                Err(e) => return Err(e.into()), // Real error, propagate
            }
        }
        
        // No keyring had the entry
        if self.optional {
            Ok(BTreeMap::new())
        } else {
            Err(KeyringError::EntryNotFound {
                service: self.service.clone(),
                credential: self.credential_name.clone(),
                searched_keyrings: self.keyrings.clone(),
            }.into())
        }
    }
}

/// Error types specific to keyring operations.
#[derive(Debug, Clone, PartialEq)]
pub enum KeyringError {
    /// The keyring entry was not found in any searched keyring.
    /// This can be handled gracefully when using `.optional()` or when 
    /// other providers may supply the value.
    EntryNotFound { 
        service: String, 
        credential: String,
        searched_keyrings: Vec<Keyring>,
    },
    
    /// The specified keyring is unavailable (e.g., System keyring requires admin,
    /// or named keyring doesn't exist).
    KeyringUnavailable { 
        keyring: Keyring,
        reason: String,
    },
    
    /// Permission denied accessing the keyring entry. This is typically
    /// a security issue and should be treated as fatal.
    PermissionDenied { 
        keyring: Keyring,
        service: String, 
        credential: String,
    },
    
    /// A backend error occurred (keyring daemon not responding, etc.).
    BackendError { 
        keyring: Keyring,
        source: String,
    },
}
```

### Configuration via Figment

Users can configure which keyrings to search using Figment2's layered configuration:

```rust
use figment2::{Figment, providers::{File, Env}};
use figment_keyring::{KeyringProvider, Keyring};

// Option 1: Hardcoded search order
let config = Figment::new()
    .merge(KeyringProvider::new("myapp", "api_key")
        .search(&[Keyring::User, Keyring::System])
        .optional())
    .extract::<Config>()?;

// Option 2: Configure search order via Figment
// config.toml:
// [keyring]
// search_order = ["user", "system"]
// 
// Or environment:
// MYAPP_KEYRING_SEARCH=user,system

// The provider can read its own configuration from Figment:
let figment = Figment::new()
    .merge(File::from("config.toml"))
    .merge(Env::prefixed("MYAPP_"));

// Provider reads keyring configuration from the Figment instance
let config = figment
    .merge(KeyringProvider::from_figment(&figment, "myapp", "api_key"))
    .extract::<Config>()?;
```

### Keyring Entry Identification

Credentials are identified by:
- **service**: The application or service category (e.g., "myapp", "discord")
- **credential_name**: The specific credential (e.g., "api_key", "discord_token")
- **keyring**: Which keyring to search (User, System, or Named)

**Platform-Specific Entry Creation:**

```bash
# macOS - User keychain (default)
security add-generic-password -s myapp -a api_key -w "secret_value"

# macOS - System keychain (requires admin)
sudo security add-generic-password -s myapp -a api_key -w "secret_value" /Library/Keychains/System.keychain

# Linux (secret-tool) - Default collection (User)
secret-tool store --label='myapp api_key' service myapp username api_key

# Linux - Named collection
secret-tool store --label='myapp api_key' service myapp username api_key --collection=production

# Windows (PowerShell)
# User: Default credential vault
# System: Requires admin, stored in system vault
```

---

## Search Precedence and Resolution

### Search Order

Keyrings are searched in the order specified. First match wins:

```rust
KeyringProvider::new("myapp", "api_key")
    .search(&[Keyring::User, Keyring::System])
```

1. Search User keyring for "myapp"/"api_key"
2. If not found, search System keyring for "myapp"/"api_key"
3. If still not found:
   - If `.optional()`: return empty map
   - Otherwise: return EntryNotFound error

### Integration with Figment2 Layers

```rust
Figment::new()
    // 1. Base config from file (lowest precedence)
    .merge(File::from("config.toml"))
    
    // 2. Keyring provider searches User, then System
    .merge(KeyringProvider::new("myapp", "api_key")
        .search(&[Keyring::User, Keyring::System])
        .optional())
    
    // 3. Environment overrides (highest precedence)
    .merge(Env::prefixed("MYAPP_"))
    .extract()?;
```

In this pattern:
- Environment variables have highest precedence (useful for CI/CD overrides)
- Keyring serves as fallback for secrets not in environment
- Searches User keyring first (personal dev credentials), then System (shared/prod)

---

## Error Handling Strategy

### Error Type Distinguishability

| Error Type | Default Behavior | With `.optional()` | Notes |
|------------|------------------|-------------------|-------|
| Entry not found in any keyring | Error (fail) | Silent skip (empty map) | Most common case for optional secrets |
| Keyring unavailable (e.g., System requires admin) | Error (fail) | Silent skip (empty map) | Skip unavailable keyrings, try others |
| Permission denied | Error (fatal) | Error (fatal) | Security issue |
| Backend error | Error (fail) | Error (fail) | Unrecoverable |

### Detailed Error Information

```rust
match provider.data() {
    Ok(config) => config,
    Err(e) => {
        if let Some(keyring_err) = e.downcast_ref::<KeyringError>() {
            match keyring_err {
                KeyringError::EntryNotFound { searched_keyrings, .. } => {
                    eprintln!("Credential not found in keyrings: {:?}", searched_keyrings);
                    // Suggest commands to add the credential
                }
                KeyringError::KeyringUnavailable { keyring: Keyring::System, .. } => {
                    eprintln!("System keyring unavailable. Try running with sudo or use User keyring.");
                }
                _ => {}
            }
        }
        return Err(e);
    }
}
```

---

## Usage Patterns

### Pattern 1: Simple User Keyring (Default)

For development and single-user scenarios:

```rust
use figment2::Figment;
use figment_keyring::KeyringProvider;

// Searches only User keyring (the default)
let config = Figment::new()
    .merge(File::from("config.toml"))
    .merge(KeyringProvider::new("myapp", "api_key").optional())
    .merge(Env::prefixed("MYAPP_"))
    .extract::<Config>()?;
```

### Pattern 2: User + System Keyring (Recommended for Production)

For applications that may have user-specific or system-wide credentials:

```rust
use figment2::Figment;
use figment_keyring::{KeyringProvider, Keyring};

// Development: User keyring has the credential
// Production: System keyring has the credential (User keyring empty)
let config = Figment::new()
    .merge(File::from("config.toml"))
    .merge(
        KeyringProvider::new("myapp", "api_key")
            .search(&[Keyring::User, Keyring::System])
            .optional()
    )
    .merge(Env::prefixed("MYAPP_"))
    .extract::<Config>()?;
```

### Pattern 3: Figment-Configured Keyrings

Allow operators to configure keyring search order via configuration:

```rust
// config.toml
[keyring]
search_order = ["user", "system"]

// main.rs
use figment2::{Figment, providers::File};
use figment_keyring::KeyringProvider;

let figment = Figment::new()
    .merge(File::from("config.toml"));

// Provider reads keyring config from the same Figment instance
let provider = KeyringProvider::new("myapp", "api_key")
    .with_config_from(&figment);

let config = figment
    .merge(provider)
    .extract::<Config>()?;
```

### Pattern 4: Multiple Credentials

```rust
use figment2::Figment;
use figment_keyring::{KeyringProvider, Keyring};

let config = Figment::new()
    .merge(File::from("config.toml"))
    .merge(
        KeyringProvider::new("myapp", "api_key")
            .search(&[Keyring::User, Keyring::System])
            .optional()
    )
    .merge(
        KeyringProvider::new("myapp", "database_url")
            .search(&[Keyring::User, Keyring::System])
            .optional()
    )
    .merge(
        KeyringProvider::new("myapp", "jwt_secret")
            .with_namespace("secrets")
            .search(&[Keyring::System])  // Only in System keyring for production
            .optional()
    )
    .merge(Env::prefixed("MYAPP_"))
    .extract::<Config>()?;
```

---

## Thread Safety

`KeyringProvider` implements both `Send` and `Sync`:

```rust
unsafe impl Send for KeyringProvider {}
unsafe impl Sync for KeyringProvider {}
```

This allows safe use across threads. The internal state is immutable after construction, and the `keyring` crate's entry access is thread-safe.

---

## Headless and Service Environment Support

System keyrings often require user session context. This design acknowledges limitations:

### Failure Modes by Keyring

| Keyring | Environment | Typical Behavior |
|---------|-------------|------------------|
| User | CI/CD | May be unavailable or empty |
| User | Systemd service | Unavailable (no user session) |
| User | Docker | Unavailable |
| System | CI/CD | May be available if configured |
| System | Systemd service | Available (system context) |
| System | Docker | Unavailable unless specially configured |

### Graceful Degradation

```rust
// For maximum compatibility, search User then System, make optional
let config = Figment::new()
    .merge(File::from("config.toml"))
    .merge(
        KeyringProvider::new("myapp", "api_key")
            .search(&[Keyring::User, Keyring::System])
            .optional()  // Allows fallback to env vars
    )
    .merge(Env::prefixed("MYAPP_"))  // CI/CD provides secrets here
    .extract()?;
```

---

## Testing Strategy

### Mock Backend

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_searches_in_order() {
        let mock_user = MockKeyring::new()
            .with_entry("myapp", "api_key", "user_secret");
        let mock_system = MockKeyring::new()
            .with_entry("myapp", "api_key", "system_secret");
        
        // User keyring has it - should return user_secret
        let provider = KeyringProvider::new("myapp", "api_key")
            .search(&[Keyring::User, Keyring::System])
            .with_backends(vec![
                (Keyring::User, mock_user),
                (Keyring::System, mock_system),
            ]);
        
        let data = provider.data().unwrap();
        assert_eq!(data["default"]["api_key"], Value::String("user_secret".into()));
    }
    
    #[test]
    fn test_falls_back_to_system() {
        let mock_user = MockKeyring::new(); // Empty
        let mock_system = MockKeyring::new()
            .with_entry("myapp", "api_key", "system_secret");
        
        // User empty, System has it - should return system_secret
        let provider = KeyringProvider::new("myapp", "api_key")
            .search(&[Keyring::User, Keyring::System])
            .with_backends(vec![
                (Keyring::User, mock_user),
                (Keyring::System, mock_system),
            ]);
        
        let data = provider.data().unwrap();
        assert_eq!(data["default"]["api_key"], Value::String("system_secret".into()));
    }
    
    #[test]
    fn test_optional_skips_all_missing() {
        let provider = KeyringProvider::new("myapp", "missing")
            .search(&[Keyring::User, Keyring::System])
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
    use figment_keyring::{KeyringProvider, Keyring};
    
    #[test]
    #[ignore = "requires keyring setup"]
    fn test_real_user_keyring() {
        // Requires: security add-generic-password -s test-app -a test-key -w "test-value"
        let provider = KeyringProvider::new("test-app", "test-key")
            .search(&[Keyring::User]);
        let data = provider.data().expect("Keyring entry should exist");
        // ... assertions
    }
}
```

---

## Tradeoffs and Considerations

### Advantages
- **Multi-keyring support**: Search across User, System, and named keyrings
- **Composability**: Configure search order via Figment2 itself
- **Security**: Secrets stored encrypted in system keyrings
- **Flexibility**: Integrates with any Figment2-based application
- **Fallback support**: First-match semantics with optional mode
- **Cross-platform**: Uses `keyring` crate supporting macOS, Linux, Windows

### Limitations
- **Single value per provider**: Each instance retrieves one credential
- **No structured data**: Currently supports only simple string values
- **Keyring dependency**: Requires system keyring service availability
- **In-memory exposure**: Values exist as plaintext after retrieval
- **No runtime rotation**: Values loaded at startup

---

## Future Enhancements

### Lazy Provider (Secondary Objective)

A future enhancement could provide lazy loading:

```rust
/// A provider that fetches credentials on-demand rather than at startup.
/// 
/// This is useful for long-running applications that want to defer
/// keyring access until the credential is actually needed.
pub struct LazyKeyringProvider {
    // Same fields as KeyringProvider
}

impl LazyKeyringProvider {
    /// Creates a lazy provider that only accesses the keyring when
    /// the value is first requested (during config extraction).
    pub fn new(service: impl Into<Service>, credential_name: impl Into<CredentialName>) -> Self;
}

impl Provider for LazyKeyringProvider {
    fn data(&self) -> Result<BTreeMap<Profile, BTreeMap<String, Value>>, Error> {
        // Returns a placeholder that fetches from keyring on first access
        // during Figment2's extraction process
    }
}
```

### JSON/Structured Secrets (v1.1)
Support storing JSON in keyring entries:

```rust
KeyringProvider::json("myapp", "credentials")
```

### Batch Retrieval
Retrieve multiple credentials in one provider:

```rust
KeyringProvider::multi("myapp", &["api_key", "database_url", "jwt_secret"])
    .search(&[Keyring::User, Keyring::System])
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
- **macOS**: Keychain Services (User=Login, System=System keychain)
- **Linux**: Secret Service API collections (User=default, System=system-wide)
- **Windows**: Credential Manager (User=current user, System=local system)

### Version Compatibility
Targeting `keyring` crate v2.x for latest API stability.

---

## Open Questions

1. **Should we support keyring-specific credential names?** 
   Example: Different credential names in User vs System keyrings for the same logical secret.

2. **How should we handle keyring authentication prompts?** 
   On macOS/Linux, accessing certain keyrings may trigger GUI prompts. Should we document this or provide a non-interactive mode?

3. **Should Named keyrings support platform-specific semantics?**
   On Linux, Named maps to Secret Service collections. On macOS/Windows, should Named map to custom keychains/vaults or file-based storage?

4. **What is the performance impact of searching multiple keyrings?**
   Should we document expected latency per keyring search?

5. **Should we provide a CLI tool for keyring management?**
   `myapp keyring set --keyring=user api_key "value"` to simplify entry creation?

---

## Conclusion

The Figment Keyring Provider enables secure, composable secret management across multiple system keyrings. By supporting User and System keyrings as first-class citizens and allowing configurable search order, applications can gracefully handle development, staging, and production environments with a single configuration approach.

**Key Design Principles:**
- **Secure by default**: Only searches User keyring unless explicitly configured
- **Composable**: Use Figment2 to configure which keyrings to search
- **Simple first**: Easy-to-understand API for the common case
- **Flexible**: Supports complex multi-keyring scenarios when needed

**Status:** Ready for implementation

# Figment Keyring Provider - Design Document v3

**Date:** 2026-02-05  
**Based on:** Cross-review synthesis with critique feedback  

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

**Multiple Keyring Support:**
Different applications and deployment scenarios require different keyring types:
- **User keyring:** Per-user credentials (default, typical for desktop applications)
- **System keyring:** System-wide credentials (requires admin/root privileges)
- **Platform-specific backends:** gnome-keyring, kwallet, macOS Keychain, Windows Credential Manager

Figment2 is a configuration management library that supports layered configuration from multiple sources (files, environment variables, etc.). However, it lacks built-in support for retrieving configuration values from system keyrings with flexible keyring type selection.

---

## Solution Design

We propose a custom `Provider` trait implementation for Figment2 that retrieves configuration values from system keyrings. This provider integrates seamlessly with Figment2's layered configuration model, allowing keyring values to be one layer among potentially many, with support for multiple keyring types configurable via Figment itself.

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

**Keyring Types Supported:**

```
┌─────────────────────────────────────────────────────────────┐
│                   Keyring Backends                       │
│                                                             │
│  ┌─────────────┐  ┌─────────────┐  ┌──────────────┐ │
│  │ User        │  │   System    │  │ Platform      │ │
│  │ Keyring    │  │   Keyring   │  │ Specific     │ │
│  │ (default)  │  │   (admin)  │  │ Backends     │ │
│  └─────────────┘  └─────────────┘  └──────────────┘ │
│         │                  │                  │              │
│         │                  │                  │              │
│         │  gnome-keyring  │  Keychain      │              │
│         │  kwallet         │  (macOS)       │              │
│         │  keepassxc       │  Cred Man      │              │
│         │                  │  (Windows)      │              │
│         └──────────────────┴─────────────────┘              │
└─────────────────────────────────────────────────────────────┘
```

### Provider Design

The `KeyringProvider` implements Figment2's `Provider` trait:

```rust
use figment2::{Provider, Metadata, value::{Map, Value}};
use std::collections::BTreeMap;

/// Specifies which keyring backend to use for credential storage/retrieval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyringTarget {
    /// User-specific keyring (default).
    /// This is the most common and safest choice for desktop applications.
    /// Credentials are stored per-user and accessible without elevated privileges.
    #[default]
    User,

    /// System-wide keyring.
    /// Requires administrator/root privileges. Suitable for daemon services or
    /// system-wide applications where credentials should be shared across users.
    System,

    /// Platform-specific keyring backend.
    /// Allows selecting a specific implementation on platforms that support multiple backends.
    #[cfg(target_os = "linux")]
    PlatformSpecific(LinuxBackend),
}

/// Linux-specific keyring backend options.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxBackend {
    /// GNOME Keyring (default on GNOME desktops)
    SecretService,

    /// KDE Wallet Manager (default on KDE desktops)
    KWallet,

    /// KeepassXC secret service integration
    KeepassXC,
}

/// A Figment2 Provider that retrieves configuration values from system keyring.
///
/// # Keyring Type Selection
///
/// The keyring backend (user, system, or platform-specific) can be configured
/// in three ways:
///
/// 1. **Default:** Uses `KeyringTarget::User` (safest, most common)
/// 2. **Builder method:** Use `.with_target()` to specify explicitly
/// 3. **Environment variable:** Set `MYAPP_KEYRING_TARGET` to configure via Figment
///
/// # Thread Safety
/// `KeyringProvider` is both `Send` and `Sync`, allowing safe use across threads.
///
/// # Examples
/// ```
/// use figment2::Figment;
/// use figment_keyring::{KeyringProvider, KeyringTarget};
///
/// // Default: user keyring
/// let config = Figment::new()
///     .merge(KeyringProvider::new("myapp", "api_key"))
///     .extract::<Config>()?;
///
/// // Explicit: system keyring
/// let config = Figment::new()
///     .merge(KeyringProvider::new("myapp", "api_key")
///         .with_target(KeyringTarget::System))
///     .extract::<Config>()?;
///
/// // Via environment: MYAPP_KEYRING_TARGET=system
/// let config = Figment::new()
///     .merge(KeyringProvider::from_env("myapp", "api_key", "MYAPP_KEYRING_TARGET"))
///     .extract::<Config>()?;
/// ```
pub struct KeyringProvider {
    target: KeyringTarget,
    service: String,
    credential_name: String,
    config_key: Option<String>,        // Maps credential_name to different config key
    namespace: Option<String>,         // Prepends namespace to config key
    optional: bool,                    // If true, silently skip missing entries
    profile: Option<Profile>,          // Target specific profile, or all if None
}

impl KeyringProvider {
    /// Creates a new KeyringProvider that retrieves credential with given
    /// service and credential_name from the **user keyring** (default).
    ///
    /// # Arguments
    /// - `service`: Application identifier (e.g., "myapp", "discord")
    /// - `credential_name`: Entry name in keyring (e.g., "api_key", "token")
    ///
    /// By default, credential_name is used as the configuration key.
    /// For example, `KeyringProvider::new("myapp", "api_key")` produces
    /// configuration `{ "api_key": "secret_value" }`.
    ///
    /// To use a different keyring type, chain `.with_target()` after construction.
    pub fn new(service: impl Into<String>, credential_name: impl Into<String>) -> Self;

    /// Creates a new KeyringProvider with the keyring target configured
    /// from an environment variable via Figment.
    ///
    /// This allows the keyring type to be specified in deployment configuration
    /// without code changes.
    ///
    /// # Arguments
    /// - `service`: Application identifier
    /// - `credential_name`: Entry name in keyring
    /// - `env_var`: Environment variable name that contains the keyring target
    ///
    /// Supported environment variable values (case-insensitive):
    /// - `"user"` or empty → `KeyringTarget::User` (default)
    /// - `"system"` → `KeyringTarget::System`
    /// - `"secret-service"` or `"gnome"` → `KeyringTarget::PlatformSpecific(LinuxBackend::SecretService)` (Linux only)
    /// - `"kwallet"` or `"kde"` → `KeyringTarget::PlatformSpecific(LinuxBackend::KWallet)` (Linux only)
    /// - `"keepassxc"` → `KeyringTarget::PlatformSpecific(LinuxBackend::KeepassXC)` (Linux only)
    ///
    /// # Example
    /// ```rust
    /// // In shell: export MYAPP_KEYRING_TARGET=system
    /// let config = Figment::new()
    ///     .merge(KeyringProvider::from_env("myapp", "api_key", "MYAPP_KEYRING_TARGET"))
    ///     .extract::<Config>()?;
    /// ```
    pub fn from_env(
        service: impl Into<String>,
        credential_name: impl Into<String>,
        env_var: impl Into<String>,
    ) -> Self;

    /// Sets the keyring target (user, system, or platform-specific).
    ///
    /// # Example
    /// ```
    /// KeyringProvider::new("myapp", "api_key")
    ///     .with_target(KeyringTarget::System)
    /// ```
    pub fn with_target(mut self, target: KeyringTarget) -> Self;

    /// Maps keyring entry to a different configuration key name.
    ///
    /// # Example
    /// ```
    /// KeyringProvider::new("myapp", "prod_api_key")
    ///     .map_to("api_key")  // Config sees "api_key", not "prod_api_key"
    /// ```
    pub fn map_to(self, key: impl Into<String>) -> Self;

    /// Sets namespace for configuration key.
    ///
    /// # Example
    /// ```
    /// KeyringProvider::new("myapp", "api_key")
    ///     .with_namespace("secrets")
    /// // Produces: { "secrets.api_key": "..." }
    /// ```
    pub fn with_namespace(self, namespace: impl Into<String>) -> Self;

    /// Makes this provider optional - if credential doesn't exist,
    /// it returns an empty map instead of an error, allowing other
    /// providers to supply value.
    ///
    /// # Example
    /// ```
    /// // This won't fail if "optional_secret" doesn't exist in keyring
    /// KeyringProvider::new("myapp", "optional_secret").optional()
    /// ```
    pub fn optional(self) -> Self;

    /// Targets a specific Figment2 profile. If not set, value is available
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
    /// when using `.optional()` or when other providers may supply value.
    EntryNotFound {
        target: KeyringTarget,
        service: String,
        credential: String,
    },

    /// The keyring service is unavailable (e.g., no user session in headless environment).
    /// This may be recoverable depending on application requirements.
    ServiceUnavailable {
        target: KeyringTarget,
        reason: String,
    },

    /// Permission denied accessing the keyring or entry.
    /// This is typically a security issue and should be treated as fatal.
    PermissionDenied {
        target: KeyringTarget,
        service: String,
        credential: String,
    },

    /// A backend error occurred (keyring daemon not responding, etc.).
    BackendError {
        target: KeyringTarget,
        source: String,
    },
}

impl std::fmt::Display for KeyringError { ... }
impl std::error::Error for KeyringError { ... }

// Thread safety: KeyringProvider is Send + Sync
unsafe impl Send for KeyringProvider {}
unsafe impl Sync for KeyringProvider {}
```

### Keyring Entry Identification

- **target:** Which keyring to search (user, system, or platform-specific)
- **service:** Application identifier (e.g., "myapp", "discord")
- **credential_name:** Specific credential entry name (e.g., "api_key", "discord_token")

This mapping aligns with the `keyring` crate's API, where `Entry::new_with_target(service, credential, target)` uniquely identifies a credential across different keyring types.

### Configuration Model

The provider retrieves a single secret value from the specified keyring and makes it available as a configuration field:

```
Keyring entry: target=User, service="myapp", credential_name="api_token"
  ↓
Configuration: { "api_token": "secret_value" }

With system keyring:
Keyring entry: target=System, service="myapp", credential_name="api_token"
  ↓
Configuration: { "api_token": "secret_value" }

With namespace:
Keyring entry: target=User, service="myapp", credential_name="api_token", namespace="secrets"
  ↓
Configuration: { "secrets.api_token": "secret_value" }
```

### Layer Integration with Multiple Keyrings

Multiple keyring providers can be composed to search multiple keyring sources:

```rust
let config: Config = Figment::new()
    .merge(File::from("config.toml"))                       // Base config
    // Try user keyring first (most common)
    .merge(KeyringProvider::new("myapp", "api_key").optional())
    // Fall back to system keyring for daemon/service contexts
    .merge(KeyringProvider::new("myapp", "api_key")
        .with_target(KeyringTarget::System)
        .optional())
    // Environment overrides (highest precedence)
    .merge(Env::prefixed("MYAPP_"))
    .extract()?;
```

In this pattern:
- User keyring checked first (default for desktop apps)
- System keyring provides fallback for service contexts
- Environment variables have highest precedence (CI/CD compatibility)
- Both keyring providers are optional to allow graceful degradation

### Environment-Configured Keyring Selection

Keyring type can be configured via environment variables, enabling deployment-specific behavior without code changes:

```rust
// In configuration file (config.toml):
// keyring_target = "system"  // or "user", "gnome", "kwallet"

// In code:
let config: Config = Figment::new()
    .merge(File::from("config.toml"))
    .merge(KeyringProvider::from_env("myapp", "api_key", "MYAPP_KEYRING_TARGET"))
    .extract()?;
```

**Environment mappings:**

| Env Value | Keyring Target | Notes |
|-----------|----------------|-------|
| `"user"` or unset | `KeyringTarget::User` | Default, safest choice |
| `"system"` | `KeyringTarget::System` | For daemon services |
| `"secret-service"` or `"gnome"` | `KeyringTarget::PlatformSpecific(SecretService)` (Linux) | GNOME desktop |
| `"kwallet"` or `"kde"` | `KeyringTarget::PlatformSpecific(KWallet)` (Linux) | KDE desktop |
| `"keepassxc"` | `KeyringTarget::PlatformSpecific(KeepassXC)` (Linux) | KeepassXC integration |

### Error Handling

The provider distinguishes between different error types:

| Error Type | Behavior | Figment2 Error Mapping |
|------------|----------|------------------------|
| Entry not found | Errors unless `.optional()` is set | `Error::MissingField` |
| Entry not found (optional) | Returns empty map, allows fallback | N/A (no error) |
| Permission denied | Fatal - indicates security issue | `Error::Custom` |
| Keyring service unavailable | Fatal - no keyring backend available | `Error::Custom` |
| Backend error | Fatal - keyring operation failed | `Error::Custom` |

**Optional Secrets:**
Mark secrets as optional to allow fallback to other providers:

```rust
// Optional secret - doesn't fail if missing
Figment::new()
    .merge(File::from("config.toml"))
    .merge(KeyringProvider::new("myapp", "api_key").optional())
    .extract()?;
```

### Profile Support

By default, keyring values are provided to all profiles. For per-profile secrets:

```rust
// All profiles receive the same value
KeyringProvider::new("myapp", "api_key")

// Only production profile receives this value
KeyringProvider::new("myapp", "api_key").for_profile(Profile::Production)
```

---

## Lazy Provider (Secondary Objective)

For advanced use cases, a lazy provider that defers keyring access until configuration extraction:

```rust
use std::sync::{Arc, OnceLock};

/// A wrapper around any Provider that defers `data()` computation
/// until it's first called. Subsequent calls return cached result.
///
/// This is useful for:
/// - Delaying expensive operations until needed
/// - Avoiding early failure during configuration setup
/// - Sharing provider instances with deferred initialization
pub struct LazyProvider<P: Provider> {
    provider: OnceLock<P>,
    constructor: Arc<dyn Fn() -> P + Send + Sync>,
}

impl<P: Provider> LazyProvider<P> {
    /// Creates a lazy provider that will construct the inner provider
    /// on the first call to `data()`.
    ///
    /// # Example
    /// ```
    /// let lazy = LazyProvider::new(|| {
    ///     KeyringProvider::new("myapp", "api_key")
    ///         .with_target(KeyringTarget::System)
    /// });
    ///
    /// let config = Figment::new()
    ///     .merge(lazy)
    ///     .extract::<Config>()?;
    /// ```
    pub fn new(constructor: impl Fn() -> P + Send + Sync + 'static) -> Self {
        Self {
            provider: OnceLock::new(),
            constructor: Arc::new(constructor),
        }
    }
}

impl<P: Provider> Provider for LazyProvider<P> {
    fn metadata(&self) -> Metadata {
        // Call constructor to get metadata
        let provider = self.provider.get_or_init(|| (self.constructor)());
        provider.metadata()
    }

    fn data(&self) -> Result<BTreeMap<Profile, BTreeMap<String, Value>>, Error> {
        // Construct provider on first call and cache result
        let provider = self.provider.get_or_init(|| (self.constructor)());
        provider.data()
    }
}

impl<P: Provider> Clone for LazyProvider<P> {
    fn clone(&self) -> Self {
        Self {
            provider: OnceLock::new(),
            constructor: Arc::clone(&self.constructor),
        }
    }
}
```

**Usage Example:**

```rust
// Primary provider (simple, eager)
let provider1 = KeyringProvider::new("myapp", "api_key");

// Lazy provider (deferred initialization)
let provider2 = LazyProvider::new(|| {
    KeyringProvider::new("myapp", "database_url")
        .with_target(KeyringTarget::System)
});

let config: Config = Figment::new()
    .merge(File::from("config.toml"))
    .merge(provider1)
    .merge(provider2)
    .extract::<Config>()?;
```

**When to use LazyProvider:**
- Keyring access may be expensive (slow IPC, encrypted backend)
- Configuration is set up but may not be extracted (e.g., help commands)
- Multiple configuration variants where only one is actually used
- Want to delay keyring lock prompts until absolutely necessary

---

## Testing Strategy

### Unit Tests with Mock Backend

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use mock_keyring::MockKeyring;

    #[test]
    fn test_user_keyring_default() {
        let mock = MockKeyring::new();
        mock.set(KeyringTarget::User, "myapp", "api_key", "secret123");

        let provider = KeyringProvider::new("myapp", "api_key");
        let data = provider.data().unwrap();

        assert_eq!(data[&Profile::Default]["api_key"], "secret123");
    }

    #[test]
    fn test_system_keyring() {
        let mock = MockKeyring::new();
        mock.set(KeyringTarget::System, "myapp", "api_key", "system_secret");

        let provider = KeyringProvider::new("myapp", "api_key")
            .with_target(KeyringTarget::System);
        let data = provider.data().unwrap();

        assert_eq!(data[&Profile::Default]["api_key"], "system_secret");
    }

    #[test]
    fn test_optional_skips_missing() {
        let mock = MockKeyring::new(); // Empty

        let provider = KeyringProvider::new("myapp", "missing").optional();
        let data = provider.data().unwrap();

        assert!(data.is_empty());
    }

    #[test]
    fn test_multi_keyring_fallback() {
        let mock = MockKeyring::new();
        mock.set(KeyringTarget::System, "myapp", "api_key", "system_secret");
        // User keyring doesn't have this entry

        let config = Figment::new()
            .merge(KeyringProvider::new("myapp", "api_key").optional())
            .merge(KeyringProvider::new("myapp", "api_key")
                .with_target(KeyringTarget::System)
                .optional())
            .extract::<serde_json::Value>()
            .unwrap();

        assert_eq!(config["api_key"], "system_secret");
    }
}
```

---

## Usage Examples

### Basic User Keyring (Default)

```rust
use figment2::{Figment, providers::File, providers::Env};
use figment_keyring::KeyringProvider;

#[derive(Deserialize)]
struct Config {
    api_key: String,
    database_url: String,
    debug: bool,
}

fn load_config() -> Result<Config, figment2::Error> {
    let config: Config = Figment::new()
        .merge(File::from("config.toml"))
        // User keyring (default)
        .merge(KeyringProvider::new("myapp", "api_key"))
        .merge(KeyringProvider::new("myapp", "database_url").optional())
        .merge(Env::prefixed("MYAPP_"))
        .extract()?;

    Ok(config)
}
```

### System Keyring for Daemon

```rust
fn load_daemon_config() -> Result<Config, figment2::Error> {
    let config: Config = Figment::new()
        .merge(File::from("/etc/myapp/config.toml"))
        // System keyring for daemon/service
        .merge(KeyringProvider::new("myapp", "api_key")
            .with_target(KeyringTarget::System))
        .extract()?;

    Ok(config)
}
```

### Multi-Keyring Fallback Pattern

```rust
fn load_config_with_fallback() -> Result<Config, figment2::Error> {
    let config: Config = Figment::new()
        .merge(File::from("config.toml"))
        // Try user keyring first (desktop app)
        .merge(KeyringProvider::new("myapp", "api_key").optional())
        // Fall back to system keyring (service context)
        .merge(KeyringProvider::new("myapp", "api_key")
            .with_target(KeyringTarget::System)
            .optional())
        // Environment as final fallback (CI/CD)
        .merge(Env::prefixed("MYAPP_"))
        .extract()?;

    Ok(config)
}
```

### Environment-Configured Keyring Type

```rust
// Shell: export MYAPP_KEYRING_TARGET=system
// Shell: export MYAPP_DAEMON_KEYRING_TARGET=system

fn load_config_from_env() -> Result<Config, figment2::Error> {
    let config: Config = Figment::new()
        .merge(File::from("config.toml"))
        // Keyring type configured via environment
        .merge(KeyringProvider::from_env("myapp", "api_key", "MYAPP_KEYRING_TARGET"))
        .merge(KeyringProvider::from_env("myapp", "database_url", "MYAPP_DAEMON_KEYRING_TARGET"))
        .extract()?;

    Ok(config)
}
```

### Platform-Specific Backend (Linux)

```rust
#[cfg(target_os = "linux")]
fn load_linux_config() -> Result<Config, figment2::Error> {
    let config: Config = Figment::new()
        .merge(File::from("config.toml"))
        // Explicitly use GNOME Keyring
        .merge(KeyringProvider::new("myapp", "api_key")
            .with_target(KeyringTarget::PlatformSpecific(LinuxBackend::SecretService)))
        .extract()?;

    Ok(config)
}
```

### Lazy Provider for Deferred Initialization

```rust
use figment_keyring::{KeyringProvider, KeyringTarget, LazyProvider};

fn load_config_lazy() -> Result<Config, figment2::Error> {
    let config: Config = Figment::new()
        .merge(File::from("config.toml"))
        // Defer expensive system keyring access until needed
        .merge(LazyProvider::new(|| {
            KeyringProvider::new("myapp", "database_url")
                .with_target(KeyringTarget::System)
        }))
        .merge(KeyringProvider::new("myapp", "api_key"))
        .extract()?;

    Ok(config)
}
```

---

## Platform-Specific Considerations

### Keyring Availability by Platform

| Platform | User Keyring | System Keyring | Platform-Specific Backends |
|----------|---------------|----------------|--------------------------|
| macOS | ✅ Keychain | ✅ Keychain (requires admin) | N/A |
| Linux | ✅ Secret Service | ✅ Secret Service (root) | Secret Service, KWallet, KeepassXC |
| Windows | ✅ Credential Manager | ✅ Credential Manager (admin) | N/A |

### Entry Creation by Platform

```bash
# macOS - User keyring
security add-generic-password -a api_key -s myapp -w "secret_value"

# macOS - System keyring (requires sudo)
sudo security add-generic-password -a api_key -s myapp -w "secret_value" -U

# Linux - User keyring (secret-tool)
secret-tool store --label='myapp api_key' service myapp username api_key

# Linux - System keyring (as root)
sudo secret-tool store --label='myapp api_key' service myapp username api_key

# Windows - User keyring
cmdkey /generic:myapp/api_key /user:api_key /pass:secret_value

# Windows - System keyring (requires admin)
cmdkey /generic:myapp/api_key /user:api_key /pass:secret_value /admin
```

### Headless Environments

System keyrings typically require a user session. Recommendations for headless contexts:

| Environment | Recommendation |
|-------------|----------------|
| CI/CD pipelines | Use environment variables; make all keyring providers `.optional()` |
| Docker containers | Mount secrets as files or use env vars; keyring unavailable |
| Systemd services (user) | Use user keyring if user session available; otherwise env vars |
| Systemd services (system) | Use system keyring with `.optional()` fallback to env vars |
| SSH sessions without X11 | Use env vars; keyring likely unavailable |

---

## Tradeoffs and Considerations

### Advantages
- **Security:** Secrets stored encrypted in system keyring
- **Flexibility:** Integrates with any Figment2-based application
- **Layered:** Works alongside other configuration sources
- **Multi-keyring:** Support for user, system, and platform-specific backends
- **Configurable:** Keyring type configurable via environment variables
- **Cross-platform:** Uses `keyring` crate supporting macOS, Linux, Windows
- **Fallback patterns:** Easy to compose multiple keyring providers for graceful degradation

### Limitations
- **Single value per provider:** Each instance retrieves one keyring entry
- **No structured data:** Currently supports only simple string values
- **Keyring dependency:** Requires system keyring service availability
- **Headless limitations:** Not suitable for CI/CD without `.optional()` fallback
- **Platform differences:** System keyring requires admin privileges on some platforms

### Design Decisions

**Why support multiple keyring types?**
- Desktop applications use user keyring (per-user, safe default)
- Daemon/services may need system keyring (shared credentials)
- Linux desktop environments have multiple backends (GNOME, KDE)
- Deployment-specific requirements (dev uses user, prod uses system)

**Why default to user keyring?**
- Safest choice for most applications
- Works without elevated privileges
- Matches user expectations
- Desktop applications typically run as unprivileged users

**Why environment-configurable keyring type?**
- Enables deployment-specific configuration without code changes
- CI/CD can override to environment variables only
- Services can specify system keyring via config file
- Allows A/B testing of keyring configurations

**Why multi-provider fallback pattern?**
- Single provider can't gracefully handle missing keyring service
- Multiple providers enable progressive fallback: user → system → env
- Matches real-world deployment scenarios (desktop vs server vs CI)
- Composability is a Figment2 strength

**Why LazyProvider as secondary objective?**
- Primary use case (eager provider) covers 90% of scenarios
- Lazy initialization adds complexity (caching, thread safety)
- Performance gain minimal for single-call Figment patterns
- Provides escape hatch for advanced use cases when needed

---

## Implementation Notes

### Dependencies
```toml
[dependencies]
figment2 = "0.10"
keyring = "2"  # Supports Entry::new_with_target()

[dev-dependencies]
# Mock keyring for testing
```

### Platform Support

The `keyring` crate's `Entry::new_with_target(service, credential, target)` API supports:

| Target Type | macOS | Linux | Windows |
|-------------|---------|--------|----------|
| `Entry::new_with_target(..., keyring::Entry::User)` | ✅ Keychain (user) | ✅ Secret Service (user) | ✅ Cred Man (user) |
| `Entry::new_with_target(..., keyring::Entry::System)` | ✅ Keychain (system) | ✅ Secret Service (system) | ✅ Cred Man (system) |
| Platform-specific | N/A | ✅ via backend selector | N/A |

---

## Open Questions

The following questions remain unresolved:

1. **LazyProvider caching semantics:** Should `LazyProvider` cache the constructed provider, or reconstruct it on each `data()` call? Current design caches.

2. **Platform-specific backend enumeration:** Should we provide a method to list available backends on Linux, or hardcode known options?

3. **System keyring permissions:** Should we provide explicit methods for checking if we have admin/root privileges for system keyring, or let it fail at runtime?

4. **Keyring type validation:** Should `KeyringProvider::from_env()` validate the environment variable value eagerly, or defer validation until `data()` is called?

5. **LazyProvider vs optional semantics:** If a lazy provider fails during construction, should it propagate error or return empty (like `.optional()`)?

These questions can be addressed during implementation based on real-world usage.

---

## Conclusion

The Figment Keyring Provider v3 design adds critical support for multiple keyring types, configurable via environment variables and builder methods. The design maintains simplicity of the converged builder pattern while enabling:

- **User keyring** (default, safe, per-user credentials)
- **System keyring** (for daemon services and system-wide credentials)
- **Platform-specific backends** (Linux desktop environment selection)
- **Environment-configurable keyring type** (deployment flexibility)
- **Multi-keyring fallback patterns** (progressive degradation)
- **Lazy providers** (secondary objective for advanced use cases)

The design addresses the critique requirements:
- ✅ Multiple keyring support (user, system, platform-specific)
- ✅ Figment-configurable keyring selection
- ✅ Easy user/system keyring access
- ✅ Default to user keyring
- ✅ Lazy provider support (secondary objective)

The design is ready for implementation.

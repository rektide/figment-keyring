# Figment Keyring Provider - Design Document v4

**Date:** 2026-02-05  
**Based on:** Cross-review synthesis with critique feedback (revised)

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
- **Custom named keyrings:** Team-specific credential collections, project-specific vaults

Figment2 is a configuration management library that supports layered configuration from multiple sources (files, environment variables, etc.). However, it lacks built-in support for retrieving configuration values from system keyrings with flexible keyring type selection and application configuration.

---

## Solution Design

We propose a custom `Provider` trait implementation for Figment2 that retrieves configuration values from system keyrings. The key insight is: **the application uses Figment to configure the keyring provider, not the other way around**.

### Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                            Figment2 Application                      │
│                                                                     │
│  ┌─────────────────────────────────────────────────────────────────┐  │
│  │ Step 1: Load Keyring Configuration from Figment         │  │
│  │                                                            │  │
│  │ Step 2: Build KeyringProvider(s) using config          │  │
│  │                                                            │  │
│  │ Step 3: Join provider(s) to final Figment         │  │
│  │                                                            │  │
│  └─────────────────────────────────────────────────────────────────┘  │
│                            ┌──────────────────────────────────────────────────────┐  │
│  │                    Final Figment Instance               │  │
│  │                                                             │  │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │  │ File Provider│  │   Env        │  │  KeyringProvider(s)        │  │
│  │  │              │  │   Provider   │  │  │                          │  │
│  │  └──────────────┘  └──────────────┘  └──────────────┘  │
│  │         │                  │                  │              │
│  │         └──────────────────┴──────────────────┴              │
│  │                            │                                │
│  │                            ▼                                │
│  │                    ┌─────────────┐                          │
│  │                    │  Final Config  │                          │
│  │                    └─────────────┘                          │
│  └─────────────────────────────────────────────────────────────────────┘
└─────────────────────────────────────────────────────────────────────────────────────┘
```

---

## Two-Phase Configuration Loading

The design uses a **two-phase pattern** where Figment configuration is extracted first, then used to build dynamic keyring providers:

1. **Phase 1:** Extract keyring configuration from Figment
2. **Phase 2:** Build providers using the extracted configuration
3. **Phase 3:** Extract final configuration from complete Figment

### Why Two Phases?

This approach solves your requirements:

1. ✅ **Figment configures keyrings:** Configuration file contains which keyrings to search, app name, account names
2. ✅ **Application code doesn't hardcode:** Users don't need to maintain code for different deployments
3. ✅ **Separation of concerns:** Configuration (what to search) is separate from secrets (how to fetch)
4. ✅ **Ops manages config:** Dev teams can update config files without touching application code

---

## Keyring Configuration Schema

The application's configuration file defines keyring settings:

```toml
# config.toml
[keyring_config]

# Application identifier for all keyring entries
app_name = "myapp"

# Which keyrings to search, in priority order
keyrings = ["user", "team-secrets", "org-secrets"]

# For named keyrings, the account/identifier
[named_keyrings.team-secrets]
account = "dev-team"

[named_keyrings.org-secrets]
account = "production"

# System keyring service name override (optional)
# If not set, uses app_name
# This allows ops to use a separate keyring for different deployments
system_keyring_service = "myapp-prod"
```

---

## Provider Design

### KeyringProvider with Figment Input

```rust
use figment2::{Figment, Provider, Metadata, value::{Map, Value}};
use figment2::Profile;
use std::collections::BTreeMap;

/// Configuration for keyring behavior, extracted from Figment
#[derive(Debug, Deserialize)]
pub struct KeyringConfig {
    /// Application identifier (service name for keyring)
    app_name: String,
    
    /// Which keyrings to search, in order
    keyrings: Vec<KeyringSource>,
    
    /// Which account to use for named keyrings
    #[serde(default)]
    account: String,
    
    /// Override system keyring service name
    #[serde(default)]
    system_keyring_service: Option<String>,
}

/// Identifies a keyring source
#[derive(Debug, Clone, Deserialize)]
pub enum KeyringSource {
    /// Default user keyring
    User,
    
    /// System-wide keyring (requires admin/root)
    System,
    
    /// Named keyring (e.g., "team-secrets" where account is "dev-team")
    Named(String),
}

/// Specifies which keyring backend to use
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyringTarget {
    #[default]
    User,
    
    System,
    
    #[cfg(target_os = "linux")]
    PlatformSpecific(LinuxBackend),
}

/// Linux-specific keyring backend options
#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxBackend {
    SecretService,
    KWallet,
    KeepassXC,
}

/// A Figment2 Provider that retrieves configuration values from system keyrings.
///
/// # Construction Pattern
///
/// ## Two-Phase Initialization
///
/// ```rust,no_run
/// // Phase 1: Load configuration
/// let keyring_config: KeyringConfig = Figment::new()
///     .merge(File::from("config.toml"))
///     .extract()?;
///
/// // Phase 2: Build providers using config
/// let provider = KeyringProvider::from_figment(
///     figment,
///     "myapp",         // app_name from config
///     "api_key"        // credential_name
/// )?;
///
/// // Phase 3: Load final config
/// let config: AppConfig = Figment::new()
///     .merge(File::from("config.toml"))
///     .merge(provider)
///     .extract()?;
/// ```
///
/// # Using Existing Figment as Input
///
/// The provider accepts an existing Figment instance to read keyring
/// configuration dynamically. This allows:
///
/// - Ops teams to update config files without code changes
/// - Different deployments (dev/staging/prod) to use different keyrings
/// - Application code to remain deployment-agnostic
///
/// # Thread Safety
///
/// `KeyringProvider` is both `Send` and `Sync`, allowing safe use
/// across threads.
///
/// # Examples
///
/// ```rust,no_run
/// // Config file specifies: keyrings = ["user", "team-secrets"]
/// // App name: "myapp"
/// 
/// use figment2::Figment;
/// use figment_keyring::KeyringProvider;
///
/// fn load_config() -> Result<AppConfig, figment2::Error> {
///     // Phase 1: Load base Figment with config
///     let config_figment = Figment::new()
///         .merge(File::from("config.toml"));
///
///     // Extract keyring configuration
///     let keyring_config: KeyringConfig = config_figment.extract()?;
///
///     // Build provider using extracted config
///     let provider = KeyringProvider::from_figment(
///         config_figment,
///         &keyring_config.app_name,  // "myapp"
///         "api_key",                   // credential_name
///     )?;
///
///     // Phase 3: Load final config with provider
///     let config: AppConfig = Figment::new()
///         .merge(File::from("config.toml"))
///         .merge(provider)
///         .extract()?;
///
///     Ok(config)
/// }
/// ```
pub struct KeyringProvider {
    /// Existing Figment instance for configuration
    figment: Figment,
    
    /// Application/service name for keyring
    app_name: String,
    
    /// Credential name in keyring
    credential_name: String,
    
    /// Keyring configuration extracted from Figment
    config: KeyringConfig,
    
    /// Optional: Map to different config key name
    config_key: Option<String>,
    
    /// Optional: Namespace prefix for config key
    namespace: Option<String>,
    
    /// Optional: Target specific profile
    profile: Option<Profile>,
    
    /// Optional: Don't fail if entry is missing
    optional: bool,
}

impl KeyringProvider {
    /// Creates a new KeyringProvider that retrieves a single credential
    /// from a configured keyring path.
    ///
    /// The keyring path is built from configuration:
    /// - For `User` or `System`: `[app_name, credential_name]`
    /// - For `Named("name")`: `[account, credential_name]`
    ///
    /// # Arguments
    ///
    /// - `figment`: Existing Figment instance with keyring config loaded
    /// - `credential_name`: The secret to retrieve (e.g., "api_key")
    ///
    /// # Returns
    ///
    /// A configured provider that will search keyrings in the
    /// configured order.
    pub fn from_figment(
        figment: &Figment,
        app_name: &str,
        credential_name: &str,
    ) -> Self {
        let config = figment.extract::<KeyringConfig>()?;
        
        Self {
            figment: figment.clone(),
            app_name: app_name.to_string(),
            credential_name: credential_name.to_string(),
            config,
            config_key: None,
            namespace: None,
            profile: None,
            optional: false,
        }
    }
    
    /// Explicitly sets which keyring backend to use (User, System, or Platform-Specific)
    ///
    /// Overrides what's configured in the keyring config.
    pub fn with_target(mut self, target: KeyringTarget) -> Self {
        Self {
            target,
            ..self
        }
    }
    
    /// Maps keyring entry to a different configuration key name
    pub fn map_to(mut self, key: impl Into<String>) -> Self {
        Self {
            config_key: Some(key.into()),
            ..self
        }
    }
    
    /// Adds a namespace prefix to the configuration key
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        Self {
            namespace: Some(namespace.into()),
            ..self
        }
    }
    
    /// Targets a specific Figment2 profile
    pub fn for_profile(mut self, profile: Profile) -> Self {
        Self {
            profile: Some(profile),
            ..self
        }
    }
    
    /// Makes this provider optional - if credential doesn't exist,
    /// returns empty map instead of an error
    pub fn optional(mut self) -> Self {
        Self {
            optional: true,
            ..self
        }
    }
}

impl Provider for KeyringProvider {
    fn metadata(&self) -> Metadata {
        Metadata::named(format!(
            "KeyringProvider({} -> {}/{})",
            self.app_name,
            self.credential_name
        ))
    }
    
    fn data(&self) -> Result<BTreeMap<Profile, BTreeMap<String, Value>>, figment2::Error> {
        // Search keyrings in configured order until credential is found
        for keyring_source in self.config.keyrings.iter() {
            // Build keyring entry path for this source
            let (service, username) = match keyring_source {
                KeyringSource::User => {
                    (self.config.app_name.clone(), self.credential_name.clone())
                }
                KeyringSource::System => {
                    // Use configured service name or default to app_name
                    let service = self.config.system_keyring_service
                        .as_ref()
                        .unwrap_or(&self.app_name);
                    (service, self.credential_name.clone())
                }
                KeyringSource::Named(account_name) => {
                    // Build named keyring: account::credential_name
                    let account = match self.config.account.as_str() {
                        "dev-team" => "dev-team",
                        "production" => "production",
                        _ => &self.config.account,
                    };
                    (account, self.credential_name.clone())
                }
            };
            
            // Attempt to retrieve credential from this keyring
            let entry = keyring::Entry::new_with_target(
                &service,
                &username,
                Self::target_to_keyring_entry(&keyring_source),
            );
            
            match entry.get_password() {
                Ok(password) => {
                    let mut map = BTreeMap::new();
                    let mut dict = BTreeMap::new();
                    
                    // Build config key name
                    let key = self.config_key.as_ref()
                        .unwrap_or(&self.credential_name);
                    
                    // Apply namespace if configured
                    let key = match &self.namespace {
                        Some(ns) => format!("{}.{}", ns, key),
                        None => key.clone(),
                    };
                    
                    // Insert into dict
                    dict.insert(key, Value::String(password));
                    
                    // Determine which profile(s) to use
                    let profiles = match &self.profile {
                        Some(p) => vec![p.clone()],
                        None => vec![Profile::Global],
                    };
                    
                    // Insert into map for each profile
                    for profile in profiles {
                        map.insert(profile, dict);
                    }
                    
                    return Ok(map);
                }
                Err(_) => {
                    if self.optional {
                        // Optional: return empty maps for all profiles
                        let mut map = BTreeMap::new();
                        let profiles = match &self.profile {
                            Some(p) => vec![p.clone()],
                            None => vec![Profile::Global],
                        };
                        for profile in profiles {
                            map.insert(profile, BTreeMap::new());
                        }
                        return Ok(map);
                    } else {
                        // Required: propagate error
                        return Err(figment2::Error::custom(format!(
                            "Keyring entry not found for credential '{}' in any configured keyring",
                            self.credential_name
                        )));
                    }
                }
            }
        }
        
        // All keyrings exhausted without finding entry
        if self.optional {
            Ok(BTreeMap::new())
        } else {
            Err(figment2::Error::custom(format!(
                "Keyring entry '{}' not found in any configured keyring",
                self.credential_name
            )))
        }
    }
    
    /// Converts our KeyringTarget enum to keyring crate's Entry::Target
    fn target_to_keyring_entry(&self, source: &KeyringTarget) -> keyring::Entry {
        match (self, source) {
            KeyringTarget::User => keyring::Entry::new(),
            KeyringTarget::System => {
                let service = self.config.system_keyring_service.as_ref()
                    .unwrap_or(&self.app_name);
                keyring::Entry::new_with_target(&service)
            }
            #[cfg(target_os = "linux")]
            KeyringTarget::PlatformSpecific(backend) => {
                let backend = match backend {
                    LinuxBackend::SecretService => keyring::Entry::new_with_target(
                        &keyring::secret_service::DEFAULT
                    ),
                    LinuxBackend::KWallet => keyring::Entry::new_with_target(
                        &keyring::secret_service::KWALLET
                    ),
                    LinuxBackend::KeepassXC => keyring::Entry::new_with_target(
                        &keyring::secret_service::KEEPASSXC
                    ),
                }
            }
            #[cfg(not(target_os = "linux"))]
            KeyringTarget::PlatformSpecific(_) => {
                keyring::Entry::new()
            }
        }
    }
}

// Thread safety: KeyringProvider is Send + Sync
unsafe impl Send for KeyringProvider {}
unsafe impl Sync for KeyringProvider {}
```

### Keyring Entry Resolution

The provider searches keyrings in the configured order until it finds a credential:

```
Configured keyrings: ["user", "team-secrets", "org-secrets"]
    ↓
Search 1: User keyring for "myapp" → "api_key"
    ↓ Not found (optional=false) → Continue
    ↓
Search 2: team-secrets (account="dev-team") → "api_key"
    ↓ Found! → Return value for all configured profiles
```

**Keyring Path Construction:**
- `User` keyring: `service="myapp", username="api_key"`
- `System` keyring: `service="myapp-prod", username="api_key"` (uses configured service name)
- `Named` keyring: `service="dev-team", username="api_key"` (uses configured account name)

### Configuration Model

The provider returns configuration for each profile:

```
For credential "api_key":

Profile::Global:
    { "api_key": "secret_from_keyring" }

Profile::Development:
    { "api_key": "secret_from_keyring" }

Profile::Production:
    { "api_key": "secret_from_keyring" }
```

### Layer Integration

Two-phase pattern integrates cleanly with Figment:

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
    // Phase 1: Load configuration Figment
    let config_figment = Figment::new()
        .merge(File::from("config.toml"))
        .select(Profile::Production);  // Select production profile if configured
    
    // Phase 2: Extract keyring configuration
    let keyring_config = config_figment.extract::<KeyringConfig>()?;
    println!("Keyrings to search: {:?}", keyring_config.keyrings);
    println!("App name: {}", keyring_config.app_name);
    
    // Phase 3: Build provider and load final config
    let config: Config = Figment::new()
        .merge(File::from("config.toml"))
        .merge(KeyringProvider::from_figment(
            config_figment,
            &keyring_config.app_name,
            "api_key",  // credential name
        ))
        .merge(Env::prefixed("MYAPP_"))  // Still can override
        .extract()?;

    Ok(config)
}
```

**Configuration flow:**
1. `File::from("config.toml")` provides base config
2. `KeyringConfig::extract()` reads which keyrings to search
3. `KeyringProvider::from_figment()` uses that config to search keyrings
4. `Env::prefixed("MYAPP_")` provides final override

---

## Tradeoffs and Considerations

### Advantages
- **Security:** Secrets stored encrypted in system keyring
- **Flexibility:** Integrates with any Figment2-based application
- **Layered:** Works alongside other configuration sources
- **Configurable:** Keyring paths managed via configuration file
- **Deployment-agnostic:** Application code doesn't hardcode keyring setup
- **Ops-friendly:** Dev teams can update config without code changes

### Limitations
- **Single value per provider:** Each provider retrieves one keyring entry
- **No structured data:** Currently supports only simple string values
- **Keyring dependency:** Requires system keyring service availability
- **Two-phase loading:** Requires separate config extraction before final load
- **Headless limitations:** Not suitable for CI/CD without `.optional()` fallback

### Design Decisions

**Why two-phase configuration?**
- **Configuration is metadata:** Keyring search paths are not secrets themselves
- **Figment reads configuration once:** In phase 1, extracted config drives provider creation
- **Separation of concerns:** Ops manages config, app code uses Figment
- **Clean API:** Application builds one Figment, no complex provider factories

**Why use Figment as input to KeyringProvider?**
- **Dynamic provider construction:** Keyring providers configured at runtime
- **No circular dependency:** Provider uses Figment only for config, not secrets
- **Type-safe configuration:** Struct-based config extracted from Figment
- **Easier to debug:** Config file shows which keyrings are being searched

**Why not use `join()` to add multiple providers?**
- `join()` returns a **new** Figment instance - can't build providers from extracted data
- Would require storing providers in intermediate structures
- Two-phase pattern is clearer: extract config, build providers, extract again

---

## Implementation Notes

### Dependencies
```toml
[dependencies]
figment2 = "0.10"
keyring = "2"  # Supports Entry::new_with_target()
serde = "1.0"

[dev-dependencies]
# Mock keyring for testing - implementation TBD
```

### Platform Support

The `keyring` crate's `Entry::new_with_target(service, credential, target)` API supports:

| Target Type | macOS | Linux | Windows |
|-------------|---------|--------|----------|
| `Entry::new_with_target(..., User)` | ✅ Keychain (user) | ✅ Secret Service (user) | ✅ Cred Man (user) |
| `Entry::new_with_target(..., System)` | ✅ Keychain (system) | ✅ Secret Service (system) | ✅ Cred Man (local) |
| Platform-specific backend | N/A | ✅ Secret Service (backend) | N/A | N/A |

---

## Usage Examples

### Basic User Keyring (From Config)

**config.toml:**
```toml
[keyring_config]
app_name = "myapp"
keyrings = ["user"]
```

```rust
fn load_config() -> Result<Config, figment2::Error> {
    let config: Config = Figment::new()
        .merge(File::from("config.toml"))
        .merge(KeyringProvider::from_figment(
            &Figment::new(),
            "myapp",
            "api_key",
        ))
        .merge(Env::prefixed("MYAPP_"))
        .extract()?;

    Ok(config)
}
```

### Using Named Team Keyring

**config.toml:**
```toml
[keyring_config]
app_name = "myapp"
keyrings = ["user", "team-secrets"]

[[named_keyrings.team-secrets]]
account = "dev-team"
```

```rust
fn load_config() -> Result<Config, figment2::Error> {
    let config: Config = Figment::new()
        .merge(File::from("config.toml"))
        .merge(KeyringProvider::from_figment(
            &Figment::new(),
            "myapp",
            "api_key",  // Will search user, then team-secrets
        ))
        .extract()?;

    Ok(config)
}
```

### Production Deployment with System Keyring

**config.toml:**
```toml
[keyring_config]
app_name = "myapp"
keyrings = ["system"]
system_keyring_service = "myapp-prod"
```

```rust
fn load_production_config() -> Result<Config, figment2::Error> {
    let config: Config = Figment::new()
        .select(Profile::Production)
        .merge(File::from("/etc/myapp/config.toml"))
        .merge(KeyringProvider::from_figment(
            &Figment::new(),
            "myapp",
            "api_key",
        ))
        .merge(Env::prefixed("MYAPP_"))
        .extract()?;

    Ok(config)
}
```

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
        mock.set(&("myapp", "api_key"), "user_secret");
        
        let config = KeyringConfig {
            app_name: "myapp".into(),
            keyrings: vec!["user".into()],
        };
        
        let provider = KeyringProvider::from_figment(
            &Figment::new(),
            &config,
            "myapp",
            "api_key",
        );
        
        let data = provider.data().unwrap();
        assert_eq!(data[&Profile::Default]["api_key"], "user_secret");
    }

    #[test]
    fn test_named_team_keyring() {
        let mock = MockKeyring::new();
        mock.set(&("dev-team", "api_key"), "team_secret");
        
        let config = KeyringConfig {
            app_name: "myapp".into(),
            keyrings: vec!["user".into(), "team-secrets".into()],
        };
        
        let provider = KeyringProvider::from_figment(
            &Figment::new(),
            &config,
            "myapp",
            "api_key",  // Will search user, then team-secrets
        );
        
        let data = provider.data().unwrap();
        assert_eq!(data[&Profile::Default]["api_key"], "team_secret");
    }

    #[test]
    fn test_system_keyring() {
        let mock = MockKeyring::new();
        mock.set(&("myapp-prod", "api_key"), "prod_secret");
        
        let config = KeyringConfig {
            app_name: "myapp".into(),
            keyrings: vec!["system".into()],
        };
        
        let provider = KeyringProvider::from_figment(
            &Figment::new(),
            &config,
            "myapp",
            "api_key",
        );
        
        let data = provider.data().unwrap();
        assert_eq!(data[&Profile::Default]["api_key"], "prod_secret");
    }

    #[test]
    fn test_optional_returns_empty_on_missing() {
        let mock = MockKeyring::new();
        // Empty mock
        
        let config = KeyringConfig {
            app_name: "myapp".into(),
            keyrings: vec!["user".into()],
        };
        
        let provider = KeyringProvider::from_figment(
            &Figment::new(),
            &config,
            "myapp",
            "api_key",
        )
        .optional();
        
        let data = provider.data().unwrap();
        assert!(data.is_empty());
    }

    #[test]
    fn test_search_respects_order() {
        let mock = MockKeyring::new();
        mock.set(&("myapp", "api_key"), "user_secret");
        mock.set(&("dev-team", "api_key"), "team_secret");
        
        let config = KeyringConfig {
            app_name: "myapp".into(),
            keyrings: vec!["user".into(), "team-secrets".into()],
        };
        
        let provider = KeyringProvider::from_figment(
            &Figment::new(),
            &config,
            "myapp",
            "api_key",
        );
        
        let data = provider.data().unwrap();
        assert_eq!(data[&Profile::Default]["api_key"], "user_secret");
    }
}
```

### Integration Tests

Platform-specific tests with real keyring, gated by environment:

```rust
#[cfg(all(test, not(ci)))]
mod integration {
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "Requires keyring setup"]
    fn test_real_keyring() {
        // Requires pre-populated keyring entry
        let provider = KeyringProvider::from_figment(
            &Figment::new(),
            &KeyringConfig {
                app_name: "myapp".into(),
                keyrings: vec!["user".into()],
            },
            "myapp",
            "api_key",
        );
        let data = provider.data().expect("Keyring entry should exist");
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "Requires macOS Keychain entry"]
    fn test_macos_keychain() {
        // Requires: security add-generic-password -s myapp -a api_key -w "secret"
        let provider = KeyringProvider::from_figment(
            &Figment::new(),
            &KeyringConfig {
                app_name: "myapp".into(),
                keyrings: vec!["user".into()],
            },
            "myapp",
            "api_key",
        );
        let data = provider.data().expect("Keyring entry should exist");
    }
}
```

---

## Operational Considerations

### Configuration Management

**Deployment scenarios:**

| Scenario | Config keyrings | System Service | Notes |
|----------|------------------|----------------|--------|
| Local development | `["user"]` | Uses user keyring | Default setup |
| Development team | `["user", "team-secrets"]` | User + team keyring | Team account in config |
| Production | `["system"]` | Uses separate service | `system_keyring_service` override |
| CI/CD | `[]` | N/A | Set env vars instead |

**Keyring entry creation examples:**

```bash
# Development - User keyring
security add-generic-password -s myapp -a api_key -w "your_api_key_here"

# Development team - Named keyring
secret-tool store --label='myapp api_key' service myapp username api_key
# Enter: dev-team
# Enter password: your_team_secret

# Production - System keyring
sudo security add-generic-password -s myapp-prod -a api_key -w "prod_secret"

# Using named keyring with custom account
secret-tool store --label='myapp api_key' --collection='custom-vault' service myapp username custom_account
```

### Headless Environments

System keyrings require a user session. Recommendations for headless contexts:

| Environment | Recommendation |
|-------------|----------------|
| CI/CD pipelines | Set env vars; make all keyring providers `.optional()` |
| Docker containers | Mount secrets as files or use env vars |
| Systemd services (user) | Use user keyring if session available; otherwise env vars |
| Systemd services (system) | Use system keyring with `.optional()` fallback to env vars |
| SSH without session | Use env vars or file-based secrets |

---

## Open Questions

The following questions remain unresolved and may be addressed during implementation or future design iterations:

1. **System keyring service name format:** Should we document format requirements (e.g., must match platform conventions)?

2. **Named keyring discovery:** Should we provide methods to list available named keyrings, or require manual configuration only?

3. **Configuration validation:** Should we validate that configured keyrings exist at provider creation time (deferred), or only at data() time?

4. **Keyring fallback behavior:** Should there be a way to configure per-credential optional vs global optional flag?

5. **Testing for multiple keyrings:** Should we test combinations of keyring availability (e.g., user missing but system available)?

6. **Service name conflicts:** What happens if app_name conflicts with a named keyring's service requirement?

7. **Account selection:** Should account be per-named-keyring or global for all named keyrings?

These questions can be addressed during implementation based on real-world usage.

---

## Conclusion

The Figment Keyring Provider v4 design introduces a **two-phase configuration pattern** where Figment configures keyring behavior, enabling:

- ✅ **Figment-managed keyring selection**: Configuration file defines search paths, app names, accounts
- ✅ **Deployment-agnostic application code**: No hardcoded keyring or service names
- ✅ **Ops-friendly configuration**: Dev teams can update config without touching code
- ✅ **Support for named keyrings**: Team-specific credential vaults
- ✅ **System keyring override**: Configurable service name for production deployments
- ✅ **Two-phase loading**: Clear separation between configuration extraction and secret loading

The design addresses your requirements:
- ✅ Figment configures which keyrings to get data from (solves "where to get data from")
- ✅ Application code specifies what app name to use (solves "what app name")
- ✅ Account configuration for named keyrings (solves "what accounts to use")
- ✅ Uses existing Figment as input (solves "can we use config that already exists")

This is a **factory pattern** where configuration (metadata) drives provider creation, enabling the keyring provider to focus solely on secret retrieval while Figment handles configuration management.

**Status:** Ready for implementation

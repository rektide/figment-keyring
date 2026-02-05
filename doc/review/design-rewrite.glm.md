# Figment Keyring Provider - Design Document v2

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
use figment2::{Provider, Metadata, value::{Map, Value}};
use std::collections::BTreeMap;

pub struct KeyringProvider {
    service: String,
    username: String,
    config_key: Option<String>,  // If None, username becomes config key
    namespace: Option<String>,    // Prefix for config key
    optional: bool,               // If true, missing entry doesn't fail
    profile: Option<Profile>,     // Target specific profile (None = all)
}

impl KeyringProvider {
    pub fn new(service: &str, username: &str) -> Self;

    // Builder methods
    pub fn map_to(mut self, key: &str) -> Self;
    pub fn with_namespace(mut self, namespace: &str) -> Self;
    pub fn optional(mut self) -> Self;
    pub fn with_profile(mut self, profile: Profile) -> Self;
}

impl Provider for KeyringProvider {
    fn metadata(&self) -> Metadata;
    fn data(&self) -> Result<BTreeMap<Profile, BTreeMap<String, Value>>, Error>;
}

// Thread safety: KeyringProvider is Send + Sync
unsafe impl Send for KeyringProvider {}
unsafe impl Sync for KeyringProvider {}
```

**Keyring Entry Identification:**
- `service`: Identifies the application or service category (e.g., "myapp", "discord")
- `username`: Identifies the specific credential in the keyring (e.g., "api_key", "discord_token")

This mapping aligns with the `keyring` crate's API, where service+username uniquely identify a credential.

### Configuration Model

The provider retrieves a single secret value and makes it available as a configuration field. By default, the `username` parameter becomes the configuration key name:

```
Keyring entry: service="myapp", username="api_token"
  ↓
Configuration: { "api_token": "secret_value" }
```

**Key Mapping:**
- **Implicit (default):** `username` becomes the config key name
- **Explicit (via `.map_to()`):** Custom config key name independent of keyring entry name

```rust
// Implicit mapping
KeyringProvider::new("myapp", "prod_api_key")
// Produces: { "prod_api_key": "secret" }

// Explicit mapping
KeyringProvider::new("myapp", "prod_api_key").map_to("api_key")
// Produces: { "api_key": "secret" }
```

**Namespace Support:**
To prevent key collisions between keyring secrets and file-based configuration, use a namespace prefix:

```rust
KeyringProvider::new("myapp", "api_key").with_namespace("secrets")
// Produces: { "secrets.api_key": "secret" }
```

**Profile Handling:**
By default, keyring values are provided to all profiles. For per-profile secrets:

```rust
// All profiles receive the same value
KeyringProvider::new("myapp", "api_key")

// Only production profile receives this value
KeyringProvider::new("myapp", "api_key").with_profile(Profile::Production)
```

### Layer Integration

The keyring provider follows Figment2's precedence model - providers merged later override values from earlier providers. Typical usage:

```rust
let config: Config = Figment::new()
    .merge(File::from("config.toml"))           // Base config (lowest precedence)
    .merge(KeyringProvider::new("myapp", "api_key"))  // Keyring overrides
    .merge(Env::prefixed("MYAPP_"))             // Env has highest precedence
    .extract()?;
```

In this pattern:
- Environment variables have highest precedence
- Keyring provides secure overrides not in environment
- Files provide defaults and non-sensitive config

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

## Testing Strategy

Testing keyring code is challenging due to platform-specific system services. The testing strategy is:

### Unit Tests
Use a mock keyring backend for platform-independent unit tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use mock_keyring::MockKeyring;

    #[test]
    fn test_retrieves_single_value() {
        let mock = MockKeyring::with_entry("myapp", "api_key", "secret123");
        let provider = KeyringProvider::with_backend(mock, "myapp", "api_key");
        let data = provider.data().unwrap();

        assert!(data.contains_key(&Profile::Default));
        assert_eq!(data[&Profile::Default]["api_key"], "secret123");
    }

    #[test]
    fn test_optional_secret_skips_on_missing() {
        let mock = MockKeyring::empty();
        let provider = KeyringProvider::with_backend(mock, "myapp", "api_key")
            .optional();
        let data = provider.data().unwrap();

        assert!(data.is_empty());  // No error, just empty
    }

    #[test]
    fn test_map_to_renames_config_key() {
        let mock = MockKeyring::with_entry("myapp", "prod_key", "secret");
        let provider = KeyringProvider::with_backend(mock, "myapp", "prod_key")
            .map_to("api_key");
        let data = provider.data().unwrap();

        assert!(data[&Profile::Default].contains_key("api_key"));
        assert!(!data[&Profile::Default].contains_key("prod_key"));
    }

    #[test]
    fn test_namespace_prefixes_config_key() {
        let mock = MockKeyring::with_entry("myapp", "api_key", "secret");
        let provider = KeyringProvider::with_backend(mock, "myapp", "api_key")
            .with_namespace("secrets");
        let data = provider.data().unwrap();

        assert!(data[&Profile::Default].contains_key("secrets.api_key"));
    }

    #[test]
    fn test_missing_entry_fails_by_default() {
        let mock = MockKeyring::empty();
        let provider = KeyringProvider::with_backend(mock, "myapp", "api_key");

        assert!(provider.data().is_err());
    }

    #[test]
    fn test_profile_targeting() {
        let mock = MockKeyring::with_entry("myapp", "api_key", "secret");
        let provider = KeyringProvider::with_backend(mock, "myapp", "api_key")
            .with_profile(Profile::Production);
        let data = provider.data().unwrap();

        assert!(!data.contains_key(&Profile::Default));
        assert!(!data.contains_key(&Profile::Development));
        assert!(data.contains_key(&Profile::Production));
    }
}
```

### Integration Tests
Platform-specific integration tests run on actual keyring backends:

```rust
#[cfg(all(test, not(feature = "mock-keyring")))]
mod integration_tests {
    use super::*;

    #[test]
    #[cfg(target_os = "linux")]
    fn test_linux_secret_service() {
        // Requires running under a user session with gnome-keyring/kwallet
        // Skipped in CI by default
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_macos_keychain() {
        // Requires macOS with accessible Keychain
    }

    #[test]
    #[cfg(windows)]
    fn test_windows_credential_manager() {
        // Requires Windows Credential Manager
    }
}
```

### CI/CD Configuration
- Unit tests with mock backend run on all platforms
- Integration tests run on platform-specific jobs
- Use feature flags: `--features mock-keyring` for CI-only tests

### Manual Testing
For development, populate the keyring with test entries:

```bash
# macOS
security add-generic-password -s myapp-test -a api_key -w "test_secret_123"
security add-generic-password -s myapp-test -a db_password -w "test_pass_456"

# Linux (secret-tool)
secret-tool store --label='myapp-test api_key' service myapp-test username api_key
echo "test_secret_123" | secret-tool store --label='myapp-test api_key' service myapp-test username api_key

# Windows (cmd)
cmdkey /generic:myapp-test/api_key /pass:test_secret_123
```

## Operational Considerations

### Headless and Service Environments

System keyrings typically require an active user session and GUI context. The following environments may not have a functional keyring:

- **CI/CD pipelines:** No persistent keyring, often no user session
- **Docker containers:** Headless, no keyring service by default
- **Systemd services:** May run under different user context
- **SSH sessions:** No keyring agent forwarding by default

**Recommendations:**
- Use `.optional()` for non-critical secrets in headless environments
- Provide environment variable fallbacks for critical secrets
- Document that keyring provider may fail in these contexts
- Consider using file-based encryption (e.g., sops, age) for deployment

### Keyring Entry Setup

Operators must populate the keyring with entries before the application runs. The library does not provide entry creation utilities (this is an operational concern).

**Platform-specific setup commands:**

**macOS (Keychain):**
```bash
# Add entry
security add-generic-password -s myapp -a api_key -w "your_api_key_here"

# Verify entry exists
security find-generic-password -s myapp -a api_key

# Delete entry
security delete-generic-password -s myapp -a api_key
```

**Linux (Secret Service API):**
```bash
# Using secret-tool (gnome-keyring/kwallet)
secret-tool store --label='myapp api_key' service myapp username api_key
# (Enter password when prompted)

# Verify
secret-tool search service myapp username api_key

# Delete
secret-tool clear service myapp username api_key
```

**Windows (Credential Manager):**
```powershell
# Using cmd
cmdkey /generic:myapp/api_key /pass:your_api_key_here

# Using PowerShell
cmdkey /generic:myapp/api_key /pass:your_api_key_here

# List entries
cmdkey /list | Select-String myapp

# Delete entry
cmdkey /delete:myapp/api_key
```

### Entry Permissions and Access Control

- Keyring entries are owned by the creating user
- Applications can only read entries for the current user context
- On Linux, ensure proper D-Bus permissions for Secret Service access
- On macOS, Keychain may prompt for permission on first access

### Security Considerations

- **In-memory plaintext:** After retrieval, secrets exist in memory as plaintext (same as environment variables)
- **Logging:** Applications should avoid logging secrets or including them in error messages
- **Keyring locking:** The provider does not unlock locked keyrings - unlock must happen before application starts
- **Access audit:** The provider does not implement access logging (platform-dependent feature)
- **Secret rotation:** Values are cached at configuration load time; rotation requires application restart

### Performance

- Keyring access involves IPC calls and encryption/decryption
- Typical access latency: 5-50ms depending on platform and keyring state
- The provider retrieves values once when `data()` is called (at `extract()` time)
- No caching is implemented beyond Figment2's natural single-call pattern
- Multiple `KeyringProvider` instances cause multiple keyring accesses (one per provider)

## Tradeoffs and Considerations

### Advantages
- **Security:** Secrets stored encrypted in system keyring
- **Flexibility:** Integrates with any Figment2-based application
- **Layered:** Works alongside other configuration sources
- **Cross-platform:** Uses `keyring` crate supporting macOS, Linux, Windows
- **Explicit:** Clear API that mirrors keyring semantics

### Limitations
- **Single value per provider:** Each instance retrieves one keyring entry
- **No structured data:** Currently supports only simple string values (JSON support deferred)
- **Keyring dependency:** Requires system keyring service availability
- **Headless limitations:** Not suitable for CI/CD or containerized environments without configuration

### Design Decisions

**Why single value per provider?**
- Simple, predictable API
- Clear mapping between keyring entries and config keys
- Follows Figment2's compositional model
- Batch API considered but deferred (partial-failure semantics are complex)

**Why not implement entry discovery?**
- Non-deterministic configuration loading
- Security risk: could expose unintended entries
- Harder to audit what secrets an application uses
- Applications should explicitly declare their dependencies

**Why not implement secret rotation at runtime?**
- Figment2 is a startup-time configuration library, not a runtime secrets manager
- Applications needing rotation should use dedicated services (AWS Secrets Manager, Vault)
- Simplicity over complexity

**Why no caching beyond Figment2's natural behavior?**
- `Figment::extract()` is typically called once at startup
- Caching is premature optimization
- If someone calls `extract()` repeatedly, that's a usage bug

## Future Enhancements (Deferred to v1.1+)

### Batch Retrieval API
Reduce verbosity for multiple secrets:

```rust
KeyringProvider::for_service("myapp")
    .with_entries(&["api_key", "database_url", "jwt_secret"])
// All entries fetched in one provider instance
```

*Considerations:* Partial-failure semantics if some entries exist and others don't.

### JSON/Structured Secrets
Support storing JSON in keyring entries:

```rust
KeyringProvider::json("myapp", "credentials")
// Parses JSON and merges nested structure into config
```

*Considerations:* Malformed JSON handling, nested path support, type conversion.

### Profile-Aware Keyring Access
Support different secrets per profile with automatic selection based on active profile:

```rust
KeyringProvider::new("myapp", "api_key")
    .with_profile_mode(ProfileMode::CurrentActive)
```

### CLI Tool for Development
Helper utility for managing keyring entries during development:

```bash
figment-keyring set myapp api_key
figment-keyring get myapp api_key
figment-keyring list myapp
figment-keyring delete myapp api_key
```

## Implementation Notes

### Dependencies
- `figment2`: ^0.10 (configuration framework)
- `keyring`: ^2.0 (cross-platform keyring access)
- Standard library: `std::collections::BTreeMap`

**Note:** `keyring` v2.0 has different error types than v1.x. Target v2.0+.

### Platform Support
The `keyring` crate supports:
- **macOS:** Keychain (requires user session)
- **Linux:** Secret Service API (gnome-keyring, kwallet; requires D-Bus session)
- **Windows:** Credential Manager (requires user session)

### Why System Keyring Over Alternatives?

| Alternative | Chosen Over System Keyring Because | System Keyring Advantages |
|------------|-----------------------------------|--------------------------|
| Environment variables | No encryption, leakable | Encryption at rest, OS-level access control |
| Environment file with paths | File permissions issues, still plaintext | Encrypted storage, automatic locking |
| File encryption (sops, age) | Requires file management, rotation complexity | Built-in keyring integration, no file distribution |
| Cloud secret services (Vault, AWS) | External dependency, network, cost | Local-only, no network, zero cost |
| Secrets in source control | Security anti-pattern | Never in version control |

System keyrings provide the best balance of security, simplicity, and local deployment for desktop and laptop applications.

## Usage Example

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
        .merge(File::from("config.toml"))           // Base config
        .merge(KeyringProvider::new("myapp", "api_key"))  // Required secret
        .merge(KeyringProvider::new("myapp", "database_url").optional())  // Optional
        .merge(Env::prefixed("MYAPP_"))             // Env overrides
        .extract()?;

    Ok(config)
}

// With namespace to avoid key collisions:
fn load_config_namespaced() -> Result<Config, figment2::Error> {
    let config: Config = Figment::new()
        .merge(File::from("config.toml"))
        .merge(KeyringProvider::new("myapp", "api_key")
            .with_namespace("secrets"))
        .merge(Env::prefixed("MYAPP_"))
        .extract()?;

    Ok(config)
}

// Per-profile secrets:
fn load_production_config() -> Result<Config, figment2::Error> {
    let config: Config = Figment::new()
        .merge(File::from("config.toml"))
        .select(Profile::Production)
        .merge(KeyringProvider::new("myapp", "prod_api_key")
            .map_to("api_key")
            .with_profile(Profile::Production))
        .extract()?;

    Ok(config)
}
```

## Conclusion

The keyring provider for Figment2 bridges the gap between secure credential storage and flexible configuration management. By integrating system keyrings as a first-class configuration layer, applications can handle secrets securely while maintaining Figment2's elegant, composable configuration model.

The design addresses critical concerns from three independent reviews:
- Clear error handling with optional/fallback semantics
- Comprehensive testing strategy with mock backend
- Explicit key mapping and namespace support
- Thread safety guarantees
- Operational guidance for headless environments
- Platform-specific setup documentation

This design is ready for implementation with a clear path forward and well-defined scope for future enhancements.

## Open Questions

The following questions remain unresolved and may be addressed during implementation or future design iterations:

1. **Mock keyring implementation:** Should the mock backend be a separate crate or a test-only module within the main crate?

2. **Batch API semantics:** If batch retrieval is implemented in v1.1, should missing entries cause partial success or fail-fast?

3. **Profile default behavior:** Is providing the same value to all profiles the right default, or should keyring values only go to the default profile?

4. **Keyring versioning:** Should the library support multiple `keyring` crate versions, or mandate a specific minimum version?

5. **Async keyring access:** Should there be an async variant of the provider for environments where keyring operations may block?

6. **Config key validation:** Should the provider validate that the resulting config key (with namespace) is valid for Figment2's config model?

7. **CI/CD testing strategy:** What is the minimum acceptable test coverage for platforms without keyring access in CI?

These questions can be deferred to implementation or addressed iteratively based on real-world usage feedback.

# Figment Keyring Provider - Design Document

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
pub struct KeyringProvider {
    service: String,
    username: String,
}

impl KeyringProvider {
    pub fn new(service: &str, username: &str) -> Self;
}

impl Provider for KeyringProvider {
    fn metadata(&self) -> Metadata;
    fn data(&self) -> Result<BTreeMap<Profile, BTreeMap<String, Value>>, Error>;
}
```

**Keyring Entry Identification:**
- `service`: Identifies the application or service category (e.g., "myapp", "discord")
- `username`: Identifies the specific credential (e.g., "api_key", "discord_token")

This mapping aligns with common keyring usage patterns where service+username uniquely identify a credential.

### Configuration Model

The provider retrieves a single secret value and makes it available as a configuration field:

```
Keyring entry: service="myapp", username="api_token"
  ↓
Configuration: { "api_token": "secret_value" }
```

For multiple secrets, applications can either:
1. Use multiple `KeyringProvider` instances with different usernames
2. Store JSON or structured data in a single keyring entry and parse it (future enhancement)

### Layer Integration

The keyring provider follows Figment2's precedence model - providers merged later override values from earlier providers. Typical usage:

```rust
Figment::new()
    .merge(File::from("config.toml"))           // Base config
    .merge(Env::prefixed("MYAPP_"))             // Env overrides
    .merge(KeyringProvider::new("myapp", "api_key"))  // Keyring fallback
    .extract()?;
```

In this pattern:
- Environment variables have highest precedence
- Keyring serves as fallback for secrets not in environment
- Files provide defaults and non-sensitive config

## Tradeoffs and Considerations

### Advantages
- **Security**: Secrets stored encrypted in system keyring
- **Flexibility**: Integrates with any Figment2-based application
- **Layered**: Works alongside other configuration sources
- **Cross-platform**: Uses `keyring` crate supporting macOS, Linux, Windows

### Limitations
- **Single value per provider**: Each instance retrieves one keyring entry
- **No structured data**: Currently supports only simple string values
- **Keyring dependency**: Requires system keyring service availability
- **Runtime errors**: Missing keyring entries cause runtime failures (as with any provider)

### Error Handling
The provider returns Figment2 errors for:
- Keyring service unavailable
- Entry not found
- Permission denied
- Keyring backend errors

Applications can handle these like any other configuration error, providing fallbacks or helpful error messages.

## Future Enhancements

### JSON/Structured Secrets
Support storing JSON in keyring entries to provide multiple values:

```rust
KeyringProvider::new("myapp", "credentials")
  ↓
{ "api_key": "...", "database_url": "..." }
```

### Entry Discovery
Automatically discover multiple keyring entries for a service:

```rust
KeyringProvider::discover("myapp")
  ↓
All entries with service="myapp"
```

### Namespace Support
Add namespace prefix to configuration keys:

```rust
KeyringProvider::new("myapp", "api_key")
  .with_namespace("secrets")
  ↓
{ "secrets.api_key": "..." }
```

### Validation and Caching
- Validate entry format at provider creation
- Cache keyring values to avoid repeated keyring access
- Watch for keyring changes (platform-dependent)

## Implementation Notes

### Dependencies
- `figment2`: Configuration framework
- `keyring`: Cross-platform keyring access
- Standard library: `std::collections::BTreeMap`

### Platform Support
The `keyring` crate supports:
- macOS: Keychain
- Linux: Secret Service API (gnome-keyring, kwallet)
- Windows: Credential Manager

### Security Considerations
- Keyring values are retrieved once at configuration loading
- Values exist in memory as plaintext after retrieval (same as env vars)
- No attempt is made to secure in-memory values (complexity tradeoff)
- Applications should minimize secret exposure in logs and error messages

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
        .merge(File::from("config.toml"))
        .merge(Env::prefixed("MYAPP_"))
        .merge(KeyringProvider::new("myapp", "api_key"))
        .merge(KeyringProvider::new("myapp", "database_url"))
        .extract()?;

    Ok(config)
}
```

## Conclusion

The keyring provider for Figment2 bridges the gap between secure credential storage and flexible configuration management. By integrating system keyrings as a first-class configuration layer, applications can handle secrets securely while maintaining Figment2's elegant, composable configuration model.

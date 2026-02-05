# Figment Keyring Provider - Revised Design

**Author**: Claude (opus)  
**Based on**: Cross-review synthesis from Opus, GLM, and Kimi reviews  
**Status**: Draft

---

## Problem Statement

Applications need secure storage for sensitive configuration: API keys, tokens, database passwords. Plaintext storage in files or environment variables risks exposure through version control, process listings, logs, or file permissions.

System keyrings offer encryption at rest, session-based access control, and OS-level credential management. This provider bridges Figment2's layered configuration with system keyring storage.

---

## Design

### Core API

```rust
use figment2::providers::Provider;

pub struct KeyringProvider {
    service: String,
    credential_name: String,
    config_key: Option<String>,
    profile: Option<Profile>,
    optional: bool,
}

impl KeyringProvider {
    /// Create a provider for a single keyring entry.
    /// 
    /// - `service`: Application identifier (e.g., "myapp")
    /// - `credential_name`: Entry name in keyring (e.g., "api_key")
    pub fn new(service: &str, credential_name: &str) -> Self;
    
    /// Map keyring entry to a different config key name.
    /// Default: uses `credential_name` as the config key.
    pub fn as_key(self, key: &str) -> Self;
    
    /// Target a specific Figment2 profile.
    /// Default: uses the default profile.
    pub fn with_profile(self, profile: Profile) -> Self;
    
    /// Don't fail if entry is missing; allow other providers to supply value.
    /// Default: missing entry is an error.
    pub fn optional(self) -> Self;
}

impl Provider for KeyringProvider {
    fn metadata(&self) -> Metadata;
    fn data(&self) -> Result<Map<Profile, Dict>, Error>;
}
```

### Configuration Mapping

```rust
// Keyring entry: service="myapp", credential_name="prod_api_key"
// Config key defaults to credential_name:
KeyringProvider::new("myapp", "prod_api_key")
// → { "prod_api_key": "secret_value" }

// Explicit key mapping:
KeyringProvider::new("myapp", "prod_api_key")
    .as_key("api_key")
// → { "api_key": "secret_value" }
```

### Layer Integration

Providers merged later override earlier values. Typical patterns:

```rust
// Pattern A: Keyring as secure fallback (env takes precedence)
Figment::new()
    .merge(File::from("config.toml"))
    .merge(KeyringProvider::new("myapp", "api_key").optional())
    .merge(Env::prefixed("MYAPP_"))
    .extract()?;

// Pattern B: Keyring as authoritative source for secrets
Figment::new()
    .merge(File::from("config.toml"))
    .merge(Env::prefixed("MYAPP_"))
    .merge(KeyringProvider::new("myapp", "api_key"))
    .extract()?;
```

### Error Handling

Errors are categorized by recoverability:

| Error Type | Behavior | Figment2 Mapping |
|------------|----------|------------------|
| Entry not found | Recoverable if `.optional()` | `Missing` kind |
| Permission denied | Fatal (security issue) | Custom error with context |
| Keyring unavailable | Recoverable if `.optional()` | `Missing` kind with warning |
| Backend error | Fatal | Custom error with context |

```rust
// Optional: missing entry is not an error
KeyringProvider::new("myapp", "optional_key").optional()

// Required (default): missing entry fails configuration
KeyringProvider::new("myapp", "required_key")
```

### Profile Support

By default, keyring values appear under the default profile. Use `.with_profile()` to target specific profiles:

```rust
// Same secret, different profiles
Figment::new()
    .merge(KeyringProvider::new("myapp", "dev_db_url")
        .as_key("database_url")
        .with_profile(Profile::Dev))
    .merge(KeyringProvider::new("myapp", "prod_db_url")
        .as_key("database_url")
        .with_profile(Profile::Prod))
```

### Thread Safety

`KeyringProvider` is `Send + Sync`. The underlying `keyring` crate handles platform-specific synchronization. Multiple providers can be created and used across threads safely.

---

## Multi-Secret Convenience (Optional)

For applications with many secrets, a batch constructor reduces verbosity:

```rust
KeyringProvider::multi("myapp", &["api_key", "db_password", "jwt_secret"])
// Equivalent to three separate providers merged together
// → { "api_key": "...", "db_password": "...", "jwt_secret": "..." }
```

**Partial failure semantics**: If any entry is missing, the entire provider fails unless all entries are marked optional via a separate `.all_optional()` method.

**Open question**: Is this complexity worth it? Multiple `.merge()` calls are idiomatic Figment2.

---

## Platform Support

### Supported Backends

| Platform | Backend | Notes |
|----------|---------|-------|
| macOS | Keychain | May prompt for access |
| Linux | Secret Service (libsecret) | Requires gnome-keyring or kwallet |
| Windows | Credential Manager | |

### Headless Environments

System keyrings typically require a user session. In environments without one:

| Environment | Recommendation |
|-------------|----------------|
| CI/CD | Use environment variables; keyring provider with `.optional()` |
| Docker | Mount secrets as files or use env vars |
| Systemd services | Use `systemd-creds` or environment files |
| SSH without agent | Use env vars or file-based secrets |

```rust
// Graceful degradation for headless:
Figment::new()
    .merge(KeyringProvider::new("myapp", "api_key").optional())
    .merge(Env::prefixed("MYAPP_"))  // Fallback in headless
```

### Entry Management

Users must populate keyring entries before application use:

```bash
# macOS
security add-generic-password -s myapp -a api_key -w "secret_value"

# Linux (secret-tool)
secret-tool store --label='myapp api_key' service myapp username api_key

# Windows (PowerShell)
cmdkey /generic:myapp:api_key /user:api_key /pass:secret_value
```

---

## Testing Strategy

### Unit Tests

Use a mock keyring backend via feature flag:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn retrieves_value_from_keyring() {
        let mock = MockKeyring::new();
        mock.set("myapp", "api_key", "test_secret");
        
        let provider = KeyringProvider::new("myapp", "api_key")
            .with_backend(mock);
        
        let data = provider.data().unwrap();
        assert_eq!(data[&Profile::Default]["api_key"], "test_secret");
    }
    
    #[test]
    fn optional_returns_empty_on_missing() {
        let mock = MockKeyring::new(); // Empty
        
        let provider = KeyringProvider::new("myapp", "missing")
            .optional()
            .with_backend(mock);
        
        let data = provider.data().unwrap();
        assert!(data.is_empty());
    }
    
    #[test]
    fn required_fails_on_missing() {
        let mock = MockKeyring::new();
        
        let provider = KeyringProvider::new("myapp", "missing")
            .with_backend(mock);
        
        assert!(provider.data().is_err());
    }
}
```

### Integration Tests

Platform-specific tests with real keyring, gated by feature or environment:

```rust
#[cfg(all(test, feature = "integration-tests"))]
mod integration {
    #[test]
    #[ignore] // Run manually: cargo test --features integration-tests -- --ignored
    fn real_keyring_roundtrip() {
        // Requires keyring entry to exist
    }
}
```

### CI Configuration

```yaml
# GitHub Actions example
jobs:
  test:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - name: Run unit tests (mock keyring)
        run: cargo test
      # Integration tests skipped in CI (no keyring session)
```

---

## Dependencies

```toml
[dependencies]
figment2 = "0.10"
keyring = "3"  # Specify major version for API stability

[dev-dependencies]
# Mock keyring for testing - implementation TBD
```

---

## Security Considerations

1. **Plaintext in memory**: After retrieval, secrets exist as `String` in memory. Same exposure as environment variables.

2. **No in-memory protection**: No `secrecy` crate integration. Complexity tradeoff; applications needing this should wrap values themselves.

3. **Logging**: Provider never logs secret values. Errors include service/credential_name but never the secret.

4. **Access prompts**: macOS Keychain may prompt users for access. Document this for CLI applications.

---

## Open Questions

1. **Batch API partial failure**: If `KeyringProvider::multi()` is implemented, what happens when some entries exist and others don't?

2. **Reload mechanism**: Should long-running applications be able to refresh keyring values? Or is this out of scope for Figment2's startup-time model?

3. **Namespace prefix**: Should `.with_namespace("secrets")` be v1 to prevent key collisions with file configs?

4. **Default service name**: Should there be a `KeyringProvider::for_crate()` that uses the crate name as service?

---

## Alternatives Considered

| Alternative | Why Not |
|-------------|---------|
| `MYAPP_API_KEY_FILE` env var | Requires file management; keyring is simpler for single-user apps |
| HashiCorp Vault | External service dependency; overkill for local development |
| age/sops encrypted files | Requires key management; keyring uses OS-level auth |

System keyring is the right choice for desktop applications and development environments where OS-level credential storage is available and appropriate.

---

## Implementation Checklist

### P0 (Required for v0.1)

- [ ] `KeyringProvider::new(service, credential_name)`
- [ ] `.as_key(key)` for config key remapping
- [ ] `.optional()` for graceful missing-entry handling
- [ ] `.with_profile(profile)` for profile targeting
- [ ] Distinguishable error types (missing vs permission vs unavailable)
- [ ] `Send + Sync` implementation
- [ ] Mock keyring backend for testing
- [ ] Platform entry management docs

### P1 (Should have for v0.1)

- [ ] Headless environment documentation
- [ ] Security considerations in README
- [ ] CI configuration examples

### P2 (Nice to have)

- [ ] `KeyringProvider::multi()` batch API
- [ ] `.with_namespace()` for key prefixing
- [ ] Performance benchmarks

### Out of Scope

- Entry discovery (security risk)
- Runtime secret rotation (use Vault for this)
- In-memory value caching (premature optimization)

# Configurable Keyring Provider Design

## Problem Statement

The current `KeyringProvider` implementation supports flexible configuration through Figment2's late-binding pattern. However, there are **hardcoded aspects** that cannot be customized:

1. **Fixed credential identifier mapping** - The provider uses `credential_name` as both:
   - The username/key in keyring lookup
   - The default config key in Figment output

   Example:
   ```toml
   # config.toml
   service = "myapp"
   keyrings = ["user", "system"]
   ```

   This results in:
   - Keyring lookup: `Entry::new("myapp", "api_key")`
   - Config output: `{ "api_key": "secret" }`

   **Limitation**: Only `credential_name` can be customized. If you need different keys for different profiles or environments, you're out of luck.

## Current Hardcoded Behaviors

### 1. Username/Key Identifier

The `KeyringConfig` defines:
- `service` - The service name (first part of keyring entry)
- `keyrings` - Which keyrings to search (user, system, named)
- `optional` - Whether to fail on missing secrets

**What's NOT configurable:**
- **Username** - Always uses `self.credential_name` for lookup
- **Target** - System keyring uses hardcoded target string; Named keyrings use their name as target

### 2. Config Key Mapping

The provider always returns:
```rust
dict.insert(key, value);
```

Where `key = self.config_key.unwrap_or(&self.credential_name)`

**Limitation**: Cannot map keyring credentials to multiple config keys without creating multiple providers.

## Use Cases Needing Flexibility

### Use Case 1: Per-Keyring Credentials

**Scenario**: You need to fetch multiple credentials from the same keyring with different usernames:

```rust
// What you want to do:
let providers = vec![
    KeyringProvider::configured_by(figment, "api_key"),
    KeyringProvider::configured_by(figment, "db_password"),
];

// Current limitation: Both use same credential_name
// They would BOTH return { "api_key": "..." }
// Figment would merge and you'd get only one value
```

**Problem**: Provider `metadata()` doesn't distinguish between providers. All use same `keyring` metadata name.

### Use Case 2: Environment-Specific Keys

**Scenario**: Different environments need different config keys:

```rust
// Development
let dev_config = KeyringConfig {
    service: "myapp-dev".to_string(),
    keyrings: vec![Keyring::User],
    optional: true,
};

// Production
let prod_config = KeyringConfig {
    service: "myapp-prod".to_string(),
    keyrings: vec![Keyring::System],
    optional: false,
};
```

**Problem**: Can't switch between these without creating different providers for each environment.

### Use Case 3: Complex Lookup Rules

**Scenario**: Application needs custom lookup logic based on multiple criteria:

- Different username patterns for different credential types
- Fallback chains across multiple keyrings
- Custom entry selection logic

**Current limitation**: Provider only supports:
- Fixed list of keyrings to search in order
- Boolean `optional` flag
- Single `credential_name` for all lookups

## Possible Solutions

### Solution A: Per-Keyring Configuration Map

Allow each keyring to specify its own username and config key:

```toml
# config.toml
[keyrings.user]
username = "db_user"
config_key = "db.api_key"

[keyrings.system]
username = "db_admin"
config_key = "db.admin_password"
```

**Pros:**
- Clear, declarative configuration
- No code changes needed
- Supports many-to-many mapping

**Cons:**
- Significant config file schema change
- More complex deserialization
- Requires changes to `KeyringConfig` structure

### Solution B: Nested Config Structure

Allow nested configuration to specify per-keyring settings:

```toml
# config.toml
[keyrings]
[[keyrings.user]]
name = "db"
username = "db_user"
config_key = "api_key"

[[keyrings.user]]
name = "secrets"
username = "secrets_user"
config_key = "secrets_api_key"
```

**Pros:**
- Supports many-to-many per-keyring configuration
- Flexible and extensible
- Declarative

**Cons:**
- Config file becomes complex
- Requires major refactoring
- More complex error messages

### Solution C: Provider Factory Pattern

Allow users to build custom providers:

```rust
trait KeyringProviderFactory {
    fn create_provider(
        config: &KeyringConfig,
        credential_name: &str,
        username: &str,
        config_key: Option<&str>,
    ) -> impl Provider;
}

// User implementation
struct MyProvider {
    provider: impl Provider,
}

impl KeyringProviderFactory for MyProvider {
    fn create_provider(/* ... */) -> impl Provider {
        // Custom lookup logic
    Box::new(MyProvider { provider })
    }
}

// Usage
let factory = MyProvider;
let provider = factory.create_provider(
    &config,
    "api_key",
    "db_user",
    Some("db.api_key")
);
```

**Pros:**
- Maximum flexibility
- Allows arbitrary custom logic
- No breaking changes to core API

**Cons:**
- Significantly more complex
- Requires trait object and heap allocation
- Loses simplicity of direct struct

### Solution D: Do Nothing (Release v1.0 with Current Design)

**Recommendation**: Document current limitations and release as v1.0.

**Rationale:**
- Current implementation is correct for the documented use cases in Opus spec
- `KeyringConfig` with `service`, `keyrings`, `optional` is sufficient
- Users wanting advanced patterns can use the provider factory pattern (Solution C) externally
- We provide a simple, well-documented API

**What users can do today:**
```rust
// Multiple credentials - use focused Figment for different paths
let config_figment = Figment::new()
    .merge(Toml::file("config.toml"));

let db_api_key = KeyringProvider::focused(
    config_figment.focused("keyrings.user.config_key"),
    "api_key"
);

let db_admin_password = KeyringProvider::focused(
    config_figment.focused("keyrings.user.username"),
    "admin_password"
);

// Environment-specific
let staging_config = Figment::new()
    .merge(Toml::file("config-staging.toml"))
    .merge(Env::prefixed("APP_"));

let staging_provider = KeyringProvider::configured_by(
    staging_config,
    "api_key"
);
```

## Decision Needed

Before implementing any of these solutions, we need to decide:

1. **Scope creep** - Are we trying to be a universal configuration framework?
2. **API stability** - Do we break the simple, documented API that matches the Opus spec?
3. **Use cases** - What real-world problems are users encountering that require these features?

**Questions to answer:**
1. Are there specific, documented user requirements for configurable usernames/keys?
2. Is this out of scope for v1.0?
3. Should we implement a factory trait pattern in v1.1 instead?
4. Which solution (A-D) best balances flexibility and complexity?

## Implementation Complexity

| Solution | API Changes | Breaking Changes | Complexity | Risk |
|-----------|-------------|-----------------|------------|------|
| A: Per-Keyring Config | Medium | No | Medium | Low |
| B: Nested Config | High | No | High | High |
| C: Factory Pattern | High | No | High | Low |
| D: Do Nothing | None | None | Low | None |

## Conclusion

The current `KeyringProvider` with `service`, `keyrings`, and `optional` fields is **simple and sufficient** for the use cases defined in the Opus specification:

- Single credential lookup with configurable service name
- Multi-keyring search in priority order
- Optional secrets support
- Late-binding configuration via Figment

For advanced use cases, users should use the **provider factory pattern** (Solution C) or create custom providers that build on `KeyringProvider`.

This keeps the core API:
- Simple
- Well-documented
- Stable
- Easy to understand

## References

- [Opus Specification](doc/review/design-rewrite.opus.md) - Original design specification
- [Current README](README.md) - Documented API and usage
- [Figment2 Documentation](https://docs.rs/figment2/) - For understanding Figment's capabilities

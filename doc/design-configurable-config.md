# Configurable Config Schema Design

## Problem Statement

**Core Issue**: Applications using figment-keyring are forced to use our hardcoded field names (`service`, `keyrings`, `optional`) in their configuration files. They cannot use their own naming conventions.

---

## Current State (Fixed Schema)

```toml
# config.toml - CONSUMER MUST USE THESE EXACT FIELD NAMES
[keyring]
service = "myapp"
keyrings = ["user", "system"]
optional = false
```

**Problem**: If the consumer prefers:
```toml
# What consumer wants to use
[secrets]
app_name = "myapp"           # instead of service
stores = ["user"]           # instead of keyrings
allow_missing = false       # instead of optional
```

They **cannot** do this with the current API.

---

## Root Cause Analysis

The `KeyringProvider::configured_by()` constructor uses Figment extraction with hardcoded field names:

```rust
impl KeyringProvider {
    pub fn configured_by(figment: Arc<Figment>, credential_name: &str) -> Self {
        // Later, in data():
        let config: KeyringConfig = figment.extract()?;  // ← Hardcoded field names!
    }
}

#[derive(Deserialize)]
pub struct KeyringConfig {
    service: String,           // ← Hardcoded
    keyrings: Vec<Keyring>,    // ← Hardcoded
    optional: bool,            // ← Hardcoded
}
```

**Problem**: `figment.extract::<KeyringConfig>()` requires exact field name match.

---

## Solutions

### Solution A: Consumer Provides Config Directly (Recommended)

Add a constructor that accepts `KeyringConfig` directly, bypassing Figment extraction.

```rust
impl KeyringProvider {
    /// Create provider with explicit configuration.
    /// 
    /// For consumers who want full control over configuration schema.
    /// This bypasses Figment extraction entirely.
    /// 
    /// # Example
    /// 
    /// ```ignore
    /// // Consumer uses their own config schema
    /// #[derive(Deserialize)]
    /// struct MySecretsConfig {
    ///     app_name: String,
    ///     secret_stores: Vec<Keyring>,
    ///     allow_missing: bool,
    /// }
    /// 
    /// let my_cfg: MySecretsConfig = figment.focus("secrets").extract()?;
    /// let keyring_cfg = KeyringConfig {
    ///     service: my_cfg.app_name,
    ///     keyrings: my_cfg.secret_stores,
    ///     optional: my_cfg.allow_missing,
    /// };
    /// 
    /// let provider = KeyringProvider::with_config(keyring_cfg, "api_key");
    /// ```
    pub fn with_config(config: KeyringConfig, credential_name: &str) -> Self {
        let figment = Arc::new(Figment::from(Serialized::defaults(config)));
        Self::configured_by(figment, credential_name)
    }
}
```

**Pros:**
- Zero complexity
- Full schema flexibility
- Type safe
- No breaking changes
- Composable with any config structure

**Cons:**
- Consumer must manually map their config to `KeyringConfig`
- Requires extra code for custom schema users

---

### Solution B: Builder with Field Name Mapping (Alternative)

Allow runtime field name remapping via builder pattern.

```rust
#[derive(Debug, Clone, Default)]
pub struct FieldAliases {
    pub service: Option<String>,
    pub keyrings: Option<String>,
    pub optional: Option<String>,
}

impl KeyringProvider {
    pub fn with_field_aliases(mut self, aliases: FieldAliases) -> Self {
        self.field_aliases = Some(aliases);
        self
    }
}
```

**Usage:**
```rust
let aliases = FieldAliases {
    service: Some("app_name".into()),
    keyrings: Some("secret_stores".into()),
    optional: Some("allow_missing".into()),
};

let provider = KeyringProvider::configured_by(figment, "api_key")
    .with_field_aliases(aliases);
```

**Pros:**
- Declarative configuration
- No manual mapping code

**Cons:**
- Requires manual Figment extraction logic
- Runtime string matching
- More complex implementation
- Harder to maintain

---

### Solution C: Generic Config with Into Trait (Alternative)

Allow any config type that can convert to `KeyringConfig`.

```rust
impl<C> KeyringProvider 
where
    C: Into<KeyringConfig> + serde::de::DeserializeOwned,
{
    pub fn configured_by_custom<C>(
        figment: Arc<Figment>,
        credential_name: &str,
    ) -> Result<Self, Error> 
    {
        let custom_config: C = figment.extract()?;
        let keyring_config = custom_config.into();
        Ok(Self::with_config(keyring_config, credential_name))
    }
}
```

**Usage:**
```rust
#[derive(Deserialize)]
struct MyConfig {
    app_name: String,
    stores: Vec<Keyring>,
    allow_missing: bool,
}

impl From<MyConfig> for KeyringConfig {
    fn from(cfg: MyConfig) -> Self {
        Self {
            service: cfg.app_name,
            keyrings: cfg.stores,
            optional: cfg.allow_missing,
        }
    }
}

let provider = KeyringProvider::configured_by_custom::<MyConfig>(
    figment,
    "api_key"
)?;
```

**Pros:**
- Type safe
- Clean consumer code

**Cons:**
- Generic complexity
- Still requires `Into` implementation
- Less discoverable than Solution A

---

## Recommendation

**Solution A (Consumer Provides Config Directly)** is the best choice:

1. **Simplicity** - Single new constructor, minimal implementation
2. **Flexibility** - Complete control over config schema
3. **Type Safety** - No runtime string matching
4. **No Breaking Changes** - Existing API unchanged
5. **Discoverability** - Clear use case in documentation

**Tradeoff:** Consumers with custom schemas write 5-10 lines of mapping code. This is acceptable and keeps our library simple.

---

## Implementation Plan

### Phase 1: Core API (P0)

- [ ] Add `KeyringProvider::with_config(config, credential_name)` constructor
- [ ] Add unit tests for `with_config()` constructor
- [ ] Verify existing tests still pass

### Phase 2: Documentation (P0)

- [ ] Document `with_config()` with examples
- [ ] Add "Bring Your Own Config" pattern section to README
- [ ] Show example with consumer-defined config schema
- [ ] Document conversion from custom config to `KeyringConfig`

### Phase 3: Quality (P1)

- [ ] Add integration test showing custom schema usage
- [ ] Add doctests demonstrating the pattern
- [ ] Run clippy and fix warnings

---

## Usage Examples

### Example 1: Consumer Uses Their Own Schema

```rust
// Consumer's config file
// secrets.toml
[secrets]
app_name = "mycompany"
stores = ["user", "custom_vault"]
allow_missing = true

// Consumer's code
use figment2::providers::Toml;
use figment_keyring::{KeyringProvider, KeyringConfig, Keyring};

// Consumer's config struct (their naming convention)
#[derive(Deserialize)]
struct SecretsConfig {
    app_name: String,
    stores: Vec<Keyring>,
    allow_missing: bool,
}

// Load config
let figment = Figment::new()
    .merge(Toml::file("secrets.toml"));

// Extract with their schema
let secrets: SecretsConfig = figment.focus("secrets").extract()?;

// Map to our config type
let keyring_config = KeyringConfig {
    service: secrets.app_name,
    keyrings: secrets.stores,
    optional: secrets.allow_missing,
};

// Create provider
let provider = KeyringProvider::with_config(keyring_config, "api_token");
```

### Example 2: Multiple Credentials from Custom Schema

```rust
// Consumer's config
// config.toml
[database]
service = "myapp-db"
keyrings = ["user", "vault"]
optional = false

[api]
service = "myapp-api"
keyrings = ["system"]
optional = true

// Consumer's code
#[derive(Deserialize)]
struct AppConfig {
    database: KeyringConfig,
    api: KeyringConfig,
}

let app_config: AppConfig = figment.extract()?;

let db_provider = KeyringProvider::with_config(
    app_config.database,
    "db_password"
);

let api_provider = KeyringProvider::with_config(
    app_config.api,
    "api_key"
);
```

### Example 3: Existing API Still Works

```rust
// Consumers happy with our schema can still use configured_by()
let provider = KeyringProvider::configured_by(figment, "api_key")
    .focused("keyring");
```

---

## Comparison Matrix

| Solution | Complexity | Breaking Changes | Flexibility | Implementation Effort |
|----------|------------|------------------|-------------|---------------------|
| A: Direct Config | Low | None | High | ~10 lines |
| B: Field Mapping | Medium | None | High | ~50 lines |
| C: Generic Into | Medium | None | High | ~30 lines |

---

## Conclusion

The problem is **config schema inflexibility**, not credential mapping complexity.

**Solution A** (`with_config()`) provides the right balance:
- Simple API addition
- Full consumer control
- Zero breaking changes
- Clear documentation path

Consumers get complete flexibility with minimal library complexity.

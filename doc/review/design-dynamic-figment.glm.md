# Dynamic Figment Configuration: Advanced Patterns

**Purpose:** Exploring Figment2's dynamic composition capabilities beyond static `.merge()` chains

---

## The Problem with Static Configuration

The standard pattern builds Figment once:

```rust
let figment = Figment::new()
    .merge(File::from("config.toml"))
    .merge(KeyringProvider::new("myapp", "api_key"))
    .merge(Env::prefixed("MYAPP_"));

let config: Config = figment.extract()?;
```

**Limitations:**
- All providers decided at compile time
- Can't add providers dynamically based on config values
- Can't conditionally include/exclude providers
- Can't populate providers from extracted configuration

---

## Figment2's Dynamic Methods

### 1. `Figment::join()` - Add Providers After Creation

The `join()` method merges a provider into an **existing** Figment instance:

```rust
pub fn join<T: Provider>(self, provider: T) -> Figment;
```

**Key Insight:** This returns a **NEW** Figment instance, it doesn't modify the original.

```rust
let mut figment = Figment::new()
    .merge(File::from("config.toml"));

// Later, add keyring provider
figment = figment.join(KeyringProvider::new("myapp", "api_key"));

// figment is now NEW instance with both providers
let config: Config = figment.extract()?;
```

### 2. `Figment::merge()` - In-Place Modification

The `merge()` method modifies the **original** Figment instance:

```rust
pub fn merge<T: Provider>(self, provider: T) -> Figment;
```

**Key Insight:** This returns a **MODIFIED** version of the same instance.

```rust
let mut figment = Figment::new()
    .merge(File::from("config.toml"));

// In-place addition
figment = figment.merge(KeyringProvider::new("myapp", "api_key"));

// Same instance now has both providers
let config: Config = figment.extract()?;
```

### 3. Provider Factory Pattern - Create Providers from Config

Create providers dynamically based on extracted configuration:

```rust
use figment2::{Figment, providers::Serialized};
use serde::Deserialize;

#[derive(Deserialize)]
struct KeyringPaths {
    keyrings: Vec<String>,  // ["user", "team-secrets", "system"]
    entries: Vec<KeyringEntry>,
}

#[derive(Deserialize)]
struct KeyringEntry {
    service: String,
    credential: String,
    optional: bool,
}

fn load_config() -> Result<Config, figment2::Error> {
    // Step 1: Build base Figment with config file
    let mut figment = Figment::new()
        .merge(File::from("config.toml"))
        .merge(Env::prefixed("MYAPP_"));

    // Step 2: Extract keyring paths from SAME Figment
    let paths: KeyringPaths = figment.extract()?;

    // Step 3: Create providers dynamically based on config
    for entry in &paths.entries {
        let provider = KeyringProvider::new(&entry.service, &entry.credential)
            .maybe_optional(entry.optional);

        figment = figment.merge(provider);
    }

    // Step 4: Extract final config
    let config: Config = figment.extract()?;
    Ok(config)
}
```

**Config file:**
```toml
# config.toml
[keyring_paths]
keyrings = ["user", "team-secrets", "system"]

[[keyring_paths.entries]]
service = "myapp"
credential = "api_key"
optional = false

[[keyring_paths.entries]]
service = "myapp"
credential = "database_url"
optional = true
```

### 4. Conditional Merging - `Serialized` with Dynamic Values

Use `Serialized` with values computed at runtime:

```rust
use figment2::{Figment, providers::Serialized};

fn load_config_dynamic(deployment: &str) -> Result<Config, figment2::Error> {
    let mut figment = Figment::new()
        .merge(File::from("config.toml"));

    // Dynamically add keyring providers based on deployment
    match deployment {
        "development" => {
            figment = figment.merge(
                KeyringProvider::new("myapp", "dev_api_key")
                    .named("team-secrets")
            );
            figment = figment.merge(
                KeyringProvider::new("myapp", "dev_db_url")
            );
        }
        "production" => {
            figment = figment.merge(
                KeyringProvider::new("myapp", "prod_api_key")
                    .system()
            );
            figment = figment.merge(
                KeyringProvider::new("myapp", "prod_db_url")
                    .system()
            );
        }
        _ => {
            figment = figment.merge(
                KeyringProvider::new("myapp", "api_key").optional()
            );
        }
    }

    let config: Config = figment.extract()?;
    Ok(config)
}
```

### 5. Two-Phase Configuration - Extract Config, Build Providers

The Opus pattern uses two Figment instances:

```rust
fn load_config_two_phase() -> Result<Config, figment2::Error> {
    // Phase 1: Extract configuration paths
    let config_figment = Figment::new()
        .merge(File::from("config.toml"))
        .merge(Env::prefixed("MYAPP_SECRETS_"));

    let keyring_config: KeyringConfig = config_figment.extract()?;

    // Phase 2: Build secret providers using extracted config
    let mut secrets_figment = Figment::new()
        .merge(File::from("config.toml"));

    for keyring_path in &keyring_config.keyrings {
        let provider = KeyringSearch::new("myapp", "api_key")
            .with_keyrings(&[keyring_path.clone()]);

        secrets_figment = secrets_figment.merge(provider);
    }

    // Phase 3: Extract final config
    let config: Config = secrets_figment.extract()?;
    Ok(config)
}
```

### 6. Join Multiple Figments - Combine Different Configs

Combine multiple independent Figment instances:

```rust
fn load_config_combined() -> Result<Config, figment2::Error> {
    let base_figment = Figment::new()
        .merge(File::from("base.toml"));

    let user_figment = Figment::new()
        .merge(File::from("user.toml"));

    let system_figment = Figment::new()
        .merge(File::from("system.toml"));

    // Merge all figments together
    let combined_figment = Figment::new()
        .merge(base_figment)
        .merge(user_figment)
        .merge(system_figment);

    let config: Config = combined_figment.extract()?;
    Ok(config)
}
```

---

## Comparison: Static vs Dynamic Patterns

| Pattern | Providers Configured | Flexibility | Complexity | When to Use |
|---------|---------------------|------------|------------|--------------|---------------|
| **Static chain** | Compile time | Low | Simple | Most common case |
| **`join()`** | Runtime (stepwise) | Medium | Medium | Add providers conditionally |
| **`merge()`** | Runtime (in-place) | Medium | Medium | Modify existing instance |
| **Factory pattern** | From extracted config | High | High | Complex multi-phase |
| **Two-phase** | From extracted config | High | High | Separation of concerns |
| **Multiple join** | Multiple instances | Very High | High | Combine separate configs |

---

## Complete Example: Configurable Keyring Search

Here's a complete implementation using dynamic configuration:

```rust
use figment2::{Figment, providers::{File, Serialized}};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct AppConfig {
    debug: bool,
    deployment: String,
}

#[derive(Debug, Deserialize)]
struct KeyringConfig {
    /// Which keyrings to search, in order
    keyrings: Vec<String>,
    /// Optional: only user, system keyrings?
}

#[derive(Debug, Deserialize)]
struct Config {
    api_key: String,
    database_url: String,
    debug: bool,
}

fn load_config() -> Result<Config, figment2::Error> {
    // Phase 1: Load app configuration
    let app_figment = Figment::new()
        .merge(File::from("config.toml"))
        .merge(Env::prefixed("MYAPP_"));

    let app_config: AppConfig = app_figment.extract()?;

    // Phase 2: Load keyring configuration from extracted paths
    let keyring_config: KeyringConfig = Figment::new()
        .merge(File::from(app_config.deployment.to_lowercase() + ".toml"))
        .extract()?;

    // Phase 3: Build dynamic provider configuration
    let mut config_figment = Figment::new()
        .merge(File::from("config.toml"));

    for keyring in &keyring_config.keyrings {
        // For each keyring type, search for all secrets
        for secret in ["api_key", "database_url", "jwt_secret"] {
            let provider = KeyringSearch::new("myapp", secret)
                .also(&keyring.parse().unwrap())  // Add to search path
                .optional();  // Don't fail if missing

            config_figment = config_figment.merge(provider);
        }
    }

    // Phase 4: Extract final configuration
    let config: Config = config_figment.extract()?;
    Ok(config)
}
```

**Config files:**
```toml
# config.toml
[app]
debug = false
deployment = "development"
```

```toml
# development.toml
[keyring_config]
keyrings = ["user", "team-secrets"]
```

```toml
# production.toml
[keyring_config]
keyrings = ["system"]
```

---

## Visualizing the Flow

### Static Chain (Original Pattern)

```
Application Code
     │
     └─ Figment::new()
         ├─ .merge(File)
         ├─ .merge(KeyringProvider::new(...))      ← Hardcoded
         ├─ .merge(KeyringProvider::new(...))      ← Hardcoded
         ├─ .merge(Env::prefixed(...))
         └─ .extract()
              │
              └─ Calls all .data() methods once
```

### Dynamic Factory Pattern

```
Application Code
     │
     ├─ Phase 1: Figment::new().merge(File).extract()
     │   Returns: KeyringConfig { keyrings: ["user", "team"] }
     │
     ├─ Phase 2: For each keyring in config:
     │   └─ Create KeyringSearch(keyrings)
     │
     └─ Phase 3: Figment::new().merge(File).merge(SearchProviders)
         └─ .extract()
              │
                 └─ Calls each SearchProvider.data()
                     └─ Searches keyrings in configured order
```

### Two-Phase (Opus Pattern)

```
Application Code
     │
     ├─ Phase 1: Figment::new().merge(File).extract()
     │   Returns: KeyringConfig { keyrings: [...] }
     │
     └─ Phase 2: Figment::new().merge(SearchProviders)
         └─ .extract()
              │
                 └─ SearchProviders use extracted config
```

---

## Key Decision Framework

### When to Use Static Chain

✅ **Use when:**
- Simple application with one deployment type
- All keyrings known at compile time
- No need for configurable search paths
- Want simplest, most explicit code

❌ **Don't use when:**
- Need to support multiple deployments
- Keyring selection depends on user configuration
- Want to dynamically add/remove providers

### When to Use `join()`

✅ **Use when:**
- Want to add providers conditionally after Figment is created
- Building configuration in multiple steps
- Provider selection depends on earlier extraction

❌ **Don't use when:**
- All providers known up front
- Simple single-pass configuration loading

### When to Use Factory Pattern

✅ **Use when:**
- Keyring search paths need to be configurable
- Different deployments use different keyring setups
- Want to extract configuration once, build providers, extract again
- Accept complexity of two-phase loading

❌ **Don't use when:**
- Simple static configuration is sufficient
- Don't want users to configure keyring behavior

### When to Use Two-Phase Pattern

✅ **Use when:**
- Want clear separation between configuration extraction and secret loading
- Using the Opus `KeyringSearch` provider approach
- Can tolerate slight complexity for better organization

❌ **Don't use when:**
- Simple one-pass loading is acceptable
- Don't need phase separation

---

## Practical Recommendations

### For Your Use Case

Based on your critique, **you want Figment-managed keyring selection**. This is the factory pattern.

**Recommended approach:**

```rust
use figment2::{Figment, providers::{File, Serialized, Env}};
use serde::Deserialize;

// Configuration for keyring management
#[derive(Deserialize, Debug)]
struct KeyringConfig {
    /// Which keyrings to search, in priority order
    /// Examples: ["user"], ["user", "system"], ["team", "system"]
    search_path: Vec<String>,
}

fn load_config() -> Result<Config, figment2::Error> {
    // Extract keyring search path from configuration
    let config_figment = Figment::new()
        .merge(File::from("config.toml"))
        .merge(Env::prefixed("MYAPP_"));

    let keyring_config: KeyringConfig = config_figment.extract()?;

    // Build keyring provider dynamically using extracted config
    let mut secrets_figment = Figment::new()
        .merge(File::from("config.toml"));

    for keyring_name in &keyring_config.search_path {
        let provider = KeyringSearch::new("myapp", "api_key")
            .with_keyrings(&[keyring_name.parse()?])
            .optional();

        secrets_figment = secrets_figment.merge(provider);
    }

    let config: Config = secrets_figment.extract()?;
    Ok(config)
}
```

**Why this works for your needs:**
1. ✅ **Figment configures itself:** Keyring search paths are configuration, not code
2. ✅ **Well-managed by Figment:** Uses standard extraction, no special hacks
3. ✅ **Flexible:** Different deployments can have completely different keyring setups
4. ✅ **No environment variable hacks:** Doesn't need `from_env()` or similar

---

## Comparing to Existing Designs

| Feature | GLM design | Opus design | Factory pattern |
|----------|--------------|--------------|----------------|
| Static keyring selection | Via `from_env()` | Via `KeyringSearch` | Via config file |
| Compile-time configuration | Yes | No | No |
| Runtime configuration | No | Yes | Yes |
| Code complexity | Low | Medium | High |
| Figment usage | Standard | Two-phase | Two-phase |
| Flexibility | Limited | High | Very High |
| Ease of understanding | High | Medium | Low (two phases) |

---

## Conclusion

Figment2 provides powerful dynamic capabilities beyond static `.merge()` chains:

1. **`join()`** - Add providers stepwise (new instance each time)
2. **`merge()`** - Modify in place (same instance)
3. **Factory pattern** - Extract config, build providers dynamically
4. **Two-phase loading** - Separate config extraction from secret loading

**For your use case** (configurable keyring management), the **factory pattern with `Serialized`** or `File` providers is the right approach. This allows Figment to manage the keyring search paths as first-class configuration, with the KeyringProvider using that configuration to search.

**Your critique is correct:** Using a single static `from_env()` option is very limited and doesn't truly make Figment the source of truth for keyring configuration.

# Review: GLM Design v4 (design-rewrite.glm.md)

**Reviewer**: Claude (opus)  
**Date**: 2026-02-05

---

## Summary

The GLM design correctly identifies the goal (Figment-configured keyring selection) and proposes a two-phase loading pattern. However, **the design conflates two different problems** and the `from_figment()` API is awkward. The fundamental architecture is sound, but the API needs rethinking.

**Verdict**: Partially approve. Core insight is correct; execution needs revision.

---

## What GLM Gets Right

### 1. Two-Phase Loading is Correct

The insight that configuration loading requires two phases is correct:

```
Phase 1: Extract keyring configuration (which keyrings to search)
Phase 2: Use that configuration to fetch secrets
```

This is the only way to achieve "Figment configures keyrings" within Figment's provider model.

### 2. KeyringConfig Struct is the Right Abstraction

```rust
#[derive(Deserialize)]
pub struct KeyringConfig {
    app_name: String,
    keyrings: Vec<KeyringSource>,
    // ...
}
```

Making the keyring configuration a deserializable struct is exactly right. This allows it to come from any Figment source (file, env, serialized).

### 3. Named Keyrings with Account Mapping

The `named_keyrings` table in TOML is a good design for mapping named keyrings to accounts:

```toml
[named_keyrings.team-secrets]
account = "dev-team"
```

---

## What GLM Gets Wrong

### 1. `from_figment()` is Confusing

```rust
let provider = KeyringProvider::from_figment(
    figment,
    "myapp",
    "api_key",
)?;
```

This API is problematic:

- **What does the Figment parameter do?** It extracts `KeyringConfig` internally, but the user also passes `"myapp"` separately. Redundant.
- **Why pass Figment instead of KeyringConfig?** If the user already extracted `KeyringConfig`, why re-extract?
- **Hidden extraction**: The provider does extraction internally, which is surprising.

**Better API**:
```rust
// User extracts config explicitly
let keyring_config: KeyringConfig = figment.extract()?;

// Provider takes config, not Figment
let provider = KeyringProvider::from_config(keyring_config, "api_key");
```

### 2. Two Separate Figment Instances is Awkward

The design requires:
```rust
// Figment #1 for config
let config_figment = Figment::new().merge(File::from("config.toml"));
let keyring_config: KeyringConfig = config_figment.extract()?;

// Figment #2 for secrets
let secrets_figment = Figment::new()
    .merge(File::from("config.toml"))  // Same file again!
    .merge(provider);
```

Merging the same file twice is redundant and error-prone. If the file changes between extractions, you get inconsistent state.

### 3. No True Late Binding

The design claims to support late binding, but configuration is still resolved at provider creation time. Once you call `from_figment()`, the keyrings are fixed.

**True late binding** would resolve configuration at `.data()` time, not at provider construction.

### 4. Testing Examples Use Wrong API

The test examples show:
```rust
let provider = KeyringProvider::from_figment(
    &Figment::new(),  // Empty Figment!
    &config,          // But also passing config directly?
    "myapp",
    "api_key",
);
```

This is inconsistent with the design's stated API. If passing config directly, why also pass Figment?

---

## The Core Problem

GLM's design tries to solve two problems with one API:

1. **Configuration source**: Where does keyring config come from? (Figment, file, env)
2. **Late binding**: When is keyring config resolved? (construction time vs `.data()` time)

These are orthogonal concerns. The design conflates them by making `from_figment()` both extract config AND construct the provider.

---

## Missing: True Late Binding

The user's critique mentions wanting **late binding** and **dynamic behavior**. GLM's design doesn't achieve this.

**What late binding means**:
- Provider is constructed without knowing which keyrings to search
- At `.data()` time, provider reads current configuration
- Configuration can change between calls to `.data()`

**How to achieve it**:
```rust
// Provider holds a config source, not resolved config
let provider = KeyringProvider::new("myapp", "api_key")
    .config_from_file("config.toml");

// At .data() time, provider:
// 1. Reads config.toml
// 2. Extracts KeyringConfig
// 3. Searches configured keyrings
// 4. Returns secret
```

This is more complex but achieves true late binding.

---

## Alternative Architecture

Instead of `from_figment()`, I propose separating concerns:

### 1. KeyringConfig is Just Data

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KeyringConfig {
    pub service: String,
    pub keyrings: Vec<Keyring>,
    #[serde(default)]
    pub optional: bool,
}
```

### 2. Provider Takes Config

```rust
// Static config (resolved at construction)
let provider = KeyringProvider::from_config(config, "api_key");

// Or: late-bound config (resolved at .data() time)
let provider = KeyringProvider::new("api_key")
    .config_source(ConfigSource::File("config.toml"));
```

### 3. ConfigSource Enum for Late Binding

```rust
pub enum ConfigSource {
    /// Config is resolved now
    Static(KeyringConfig),
    
    /// Read config from file at .data() time
    File(PathBuf),
    
    /// Read config from env var at .data() time
    Env(String),
    
    /// Read config from multiple sources at .data() time
    Layered(Vec<ConfigSource>),
}
```

This cleanly separates:
- **What** to fetch (credential_name)
- **Where** to get config (ConfigSource)
- **When** to resolve config (construction vs .data())

---

## Comparison Table

| Aspect | GLM Design | My Proposal |
|--------|------------|-------------|
| Config extraction | Hidden in `from_figment()` | Explicit by user |
| Late binding | No | Yes, via ConfigSource |
| API clarity | Confusing (Figment + app_name) | Clear (config OR config source) |
| Two-phase loading | Required | Optional (only if using Static) |
| Dynamic reconfiguration | No | Yes, with File/Env sources |

---

## Recommendations

### For GLM Design to Work

1. **Replace `from_figment()` with `from_config()`**
   - User extracts config explicitly
   - Provider just uses it

2. **Add `ConfigSource` for late binding**
   - Allow provider to read config at `.data()` time
   - Enables dynamic reconfiguration

3. **Don't require two Figment instances**
   - If using explicit extraction, user handles it
   - If using ConfigSource, provider handles it internally

4. **Fix test examples**
   - Consistent API throughout

### My Revised Design Will

1. Keep `KeyringConfig` as a plain deserializable struct
2. Offer both static (`from_config`) and late-bound (`config_source`) construction
3. Support ConfigSource::Figment for true Figment integration
4. Avoid requiring users to create two Figment instances

---

## Conclusion

GLM's design has the right goal but awkward execution. The `from_figment()` API conflates extraction with construction and doesn't achieve true late binding. The two-Figment pattern is redundant.

My revised design will separate:
1. **Config definition** (KeyringConfig struct)
2. **Config sourcing** (static vs late-bound)
3. **Secret fetching** (the actual keyring access)

This enables both simple static usage and advanced late-bound dynamic configuration.

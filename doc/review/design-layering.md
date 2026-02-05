# Figment Layering and Keyring Configuration Examples

**Purpose:** Clarify how Figment layering works and how KeyringProvider can be configured via Figment itself

---

## Understanding Figment Layering

### The Core Model

Figment uses a **layered merge model**:

```
┌─────────────────────────────────────────────────────────┐
│                    Figment Instance                         │
│                                                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │ Provider 1  │  │   Provider 2  │  │   Provider 3  │  │
│  │              │  │              │  │              │  │
│  └──────────────┘  └──────────────┘  └──────────────┘  │
│         │                  │                  │             │
│         ▼                  ▼                  ▼             │
│         ┌──────────────────────────────────────┐            │
│         │         Merged Data             │            │
│         └──────────────────────────────────────┘            │
│                            │                                │
│                            ▼                                │
│                    ┌─────────────┐                          │
│                    │   Config     │                          │
│                    └─────────────┘                          │
└─────────────────────────────────────────────────────────────┘
```

### Key Principles

1. **Providers are independent:** Each provider doesn't know about other providers
2. **Merge order matters:** Providers merged later override values from earlier providers
3. **Single extraction:** Calling `.extract()` calls `.data()` on each provider once
4. **No cross-provider awareness:** Provider A cannot see what Provider B returned

### Simple Layering (No Figment-as-Input)

The most common pattern is straightforward merging:

```rust
use figment2::{Figment, providers::File, providers::Env};
use figment_keyring::KeyringProvider;

let config: Config = Figment::new()
    .merge(File::from("config.toml"))           // Layer 1 (lowest)
    .merge(KeyringProvider::new("myapp", "api_key"))  // Layer 2
    .merge(Env::prefixed("MYAPP_"))             // Layer 3 (highest)
    .extract()?;
```

**What happens:**
1. `File::from("config.toml").data()` is called → returns `{"api_key": "from_file", "debug": true}`
2. `KeyringProvider::new(...).data()` is called → returns `{"api_key": "from_keyring"}`
3. `Env::prefixed("MYAPP_").data()` is called → returns `{"API_KEY": "from_env"}`
4. Figment merges all data:
   - Start with file data: `{"api_key": "from_file", "debug": true}`
   - Merge keyring data: `{"api_key": "from_keyring", "debug": true}` (keyring overrides file)
   - Merge env data: `{"API_KEY": "from_env", "api_key": "from_keyring", "debug": true}` (env overrides keyring)
5. Result: `api_key = "from_keyring"`, `debug = true`

**Important:** The providers never see each other. Only Figment sees all their data.

---

## Pattern 1: GLM Design (design-rewrite-v3.glm.md)

### Keyring Selection via Environment Variable

The GLM design uses `KeyringProvider::from_env()` which reads an environment variable to decide which keyring to use:

```rust
// KeyringProvider reads MYAPP_KEYRING_TARGET at data() time
let provider = KeyringProvider::from_env("myapp", "api_key", "MYAPP_KEYRING_TARGET");
```

**Complete Example:**

```rust
use figment2::{Figment, providers::File, providers::Env};
use figment_keyring::KeyringProvider;

#[derive(Deserialize)]
struct Config {
    api_key: String,
    database_url: String,
}

fn load_config() -> Result<Config, figment2::Error> {
    // Shell: export MYAPP_KEYRING_TARGET=system
    // Or: unset (defaults to user keyring)
    
    let config: Config = Figment::new()
        .merge(File::from("config.toml"))
        // Provider reads MYAPP_KEYRING_TARGET at .data() time
        .merge(KeyringProvider::from_env("myapp", "api_key", "MYAPP_KEYRING_TARGET"))
        .merge(KeyringProvider::from_env("myapp", "database_url", "MYAPP_KEYRING_TARGET"))
        .merge(Env::prefixed("MYAPP_"))  // Still can override with env vars
        .extract()?;

    Ok(config)
}
```

**Data Flow:**

```
Time 0: Application code builds Figment with providers
         ↓
Time 1: User calls figment.extract::<Config>()
         ↓
Time 2: Figment calls FileProvider.data()
         Returns: {"api_key": "from_file", "database_url": "file_db_url"}
         ↓
Time 3: Figment calls KeyringProvider::from_env(...).data()
         ↓
         Provider reads MYAPP_KEYRING_TARGET environment variable
         ↓
         If MYAPP_KEYRING_TARGET == "system":
             → Fetches from system keyring
         If MYAPP_KEYRING_TARGET is unset or "user":
             → Fetches from user keyring
         ↓
         Returns: {"api_key": "secret_from_keyring"}  (no database_url - only one key per provider)
         ↓
Time 4: Figment calls EnvProvider.data()
         Returns: {} (empty if no MYAPP_* vars set)
         ↓
Time 5: Figment merges all data:
         {"api_key": "secret_from_keyring", "database_url": "file_db_url"}
         ↓
Time 6: Figment deserializes into Config struct
         Config { api_key: "secret_from_keyring", database_url: "file_db_url" }
```

**When to use this pattern:**
- Different deployment scenarios (dev uses user keyring, prod uses system keyring)
- CI/CD needs different keyring than local dev
- Want to configure keyring type without code changes

---

## Pattern 2: Multi-Provider Fallback (All Designs)

Search multiple keyrings by creating multiple provider instances:

```rust
use figment2::{Figment, providers::File, providers::Env};
use figment_keyring::KeyringProvider;

fn load_config_with_fallback() -> Result<Config, figment2::Error> {
    let config: Config = Figment::new()
        .merge(File::from("config.toml"))
        // Try user keyring first - returns empty if not found
        .merge(KeyringProvider::new("myapp", "api_key").optional())
        // Try system keyring - returns empty if not found
        .merge(KeyringProvider::new("myapp", "api_key")
            .with_target(KeyringTarget::System)
            .optional())
        // Environment overrides all
        .merge(Env::prefixed("MYAPP_"))
        .extract()?;

    Ok(config)
}
```

**Data Flow:**

```
Time 0: Build Figment
         ↓
Time 1: FileProvider.data()
         Returns: {"api_key": "from_file"}
         ↓
Time 2: KeyringProvider (user).data()
         User keyring has the entry
         Returns: {"api_key": "user_secret"}
         ↓
Time 3: KeyringProvider (system).data()
         System keyring DOES NOT have the entry
         .optional() means: don't fail, return empty
         Returns: {}  (empty map)
         ↓
Time 4: EnvProvider.data()
         Returns: {} (no MYAPP_API_KEY set)
         ↓
Time 5: Figment merges all:
         Start: {"api_key": "from_file"}
         Merge user keyring: {"api_key": "user_secret"}  (overrides file)
         Merge system keyring: {"api_key": "user_secret"}  (no change - empty)
         Merge env vars: {"api_key": "user_secret"}
         ↓
Time 6: Result: Config { api_key: "user_secret" }
```

**If user keyring didn't have the entry:**

```
Time 2: KeyringProvider (user).data()
         User keyring DOES NOT have the entry
         .optional() means: don't fail, return empty
         Returns: {}  (empty map)
         ↓
Time 3: KeyringProvider (system).data()
         System keyring has the entry
         Returns: {"api_key": "system_secret"}
         ↓
Time 5: Figment merges all:
         Start: {"api_key": "from_file"}
         Merge user keyring: {"api_key": "from_file"}  (no change - empty)
         Merge system keyring: {"api_key": "system_secret"}  (overrides file)
         Merge env vars: {"api_key": "system_secret"}
         ↓
Time 6: Result: Config { api_key: "system_secret" }
```

---

## Pattern 3: Opus Design (design-rewrite.opus.md) - Figment as Input

The Opus design introduces `KeyringSearch` which reads its configuration from Figment:

```rust
// config.toml
[secrets]
keyrings = ["user", "team-secrets", "system"]

// In code
let base_figment = Figment::new()
    .merge(File::from("config.toml"));

// Extract KeyringConfig from Figment
let keyring_config: KeyringConfig = base_figment.extract()?;

// Use config to create KeyringSearch
let search_provider = KeyringSearch::new("myapp", "api_key")
    .with_keyrings(keyring_config.keyrings);

// Now merge search_provider into a NEW Figment instance
let config: Config = Figment::new()
    .merge(File::from("config.toml"))
    .merge(search_provider)
    .extract()?;
```

**This is TWO Figment instances:**
- First instance: Extracts `keyrings` array from config file
- Second instance: Uses that array to configure the search provider

**Data Flow:**

```
Time 0: Create first Figment instance
         Figment1 = Figment::new().merge(File::from("config.toml"))
         ↓
Time 1: Extract KeyringConfig
         config = Figment1.extract::<KeyringConfig>()
         Reads config.toml → KeyringConfig { keyrings: ["user", "team-secrets", "system"] }
         ↓
Time 2: Create KeyringSearch provider
         search = KeyringSearch::new("myapp", "api_key")
             .with_keyrings(["user", "team-secrets", "system"])
         ↓
         The search provider stores: service="myapp", credential="api_key"
         The search provider stores: keyrings=[User, Named("team-secrets"), System]
         ↓
Time 3: Create second Figment instance
         Figment2 = Figment::new().merge(File::from("config.toml"))
         ↓
Time 4: Extract final config
         config = Figment2.extract::<Config>()
         ↓
         Figment2 calls FileProvider.data()
         Returns: {"api_key": "from_file", "debug": true}
         ↓
         Figment2 calls KeyringSearch.data()
             ↓
             KeyringSearch tries each keyring in order:
             1. User keyring → Entry found? Yes → Returns {"api_key": "user_secret"}
             (stops here, doesn't check team-secrets or system)
         ↓
         Figment2 merges: {"api_key": "user_secret", "debug": true}
         ↓
Time 5: Result: Config { api_key: "user_secret", debug: true }
```

**If user keyring didn't have entry:**

```
Time 4: Figment2 calls KeyringSearch.data()
         ↓
         KeyringSearch tries each keyring:
         1. User keyring → Entry found? No → Continue
         2. team-secrets keyring → Entry found? Yes → Returns {"api_key": "team_secret"}
         ↓
         Figment2 merges: {"api_key": "team_secret", "debug": true}
```

**Why use two Figment instances?**
- The `keyrings` array is CONFIGURATION, not a secret
- We extract it once to build the search path
- Then use that path to fetch the actual secret
- Avoids re-reading config file during secret retrieval

---

## Pattern 4: Kimi Design (design.kimi.md) - Not a Rewrite

The Kimi document is a **review**, not a design rewrite. It doesn't provide a new design pattern.

---

## Comparison of Patterns

| Pattern | Figment-as-Input? | Keyring Selection | Complexity | When to Use |
|----------|-------------------|------------------|------------|---------------|
| **Simple layering** | No | Single type (user or system hardcoded) | Simple | Most common case |
| **GLM env-config** | No (reads env var directly) | Via environment variable | Different deployments without code changes |
| **Multi-provider fallback** | No | Multiple providers merged in order | Moderate | Progressive fallback: user → system → env |
| **Opus KeyringSearch** | Yes (Figment extracts search path) | Via config file | Complex, configurable search order |

---

## Visualizing the Difference

### Simple Layering (No Figment-as-Input)

```
Application Code
     │
     ├─ Figment::new()
     │
     ├─ .merge(File::from(...))
     ├─ .merge(KeyringProvider::new(...))    ← Keyring type hardcoded in code
     ├─ .merge(Env::prefixed(...))
     │
     └─ .extract::<Config>()
              │
              ├─ Calls FileProvider.data()
              ├─ Calls KeyringProvider.data()
              └─ Calls EnvProvider.data()
```

### Opus KeyringSearch (Figment-as-Input)

```
Application Code
     │
     ├─ Figment::new()               ← Figment #1
     │
     ├─ .merge(File::from(...))
     │
     └─ .extract::<KeyringConfig>()     ← Extract CONFIG (not secrets)
              │
              └─ Returns: KeyringConfig { keyrings: ["user", "team", ...] }
              │
Application uses config
     │
     ├─ KeyringSearch::new(...)
     │        .with_keyrings(config.keyrings)  ← Use extracted config
     │
     ├─ Figment::new()               ← Figment #2
     │
     ├─ .merge(File::from(...))
     ├─ .merge(KeyringSearch)           ← Provider uses config from Figment #1
     │
     └─ .extract::<Config>()
              │
              ├─ Calls FileProvider.data()
              └─ Calls KeyringSearch.data()  ← Search uses keyrings array
                      │
                      ├─ Try User keyring
                      ├─ Try team keyring
                      └─ Try System keyring
```

---

## Key Takeaways

### 1. Providers are Independent

A provider never knows:
- What other providers returned
- That it's part of a merge chain
- What order it was merged in

A provider only knows:
- Its own configuration (service, credential, etc.)
- Its own data source (file, environment, keyring, etc.)

### 2. Figment Orchestrates Merging

Figment's job is to:
1. Call `.data()` on each provider in merge order
2. Merge the returned data (later providers override earlier ones)
3. Extract the final merged data into your struct

### 3. Figment-as-Input Means "Configuration, Not Secrets"

When a provider uses Figment as input:
- It extracts **configuration metadata** (e.g., which keyrings to search)
- It does NOT extract the secrets themselves
- A second Figment instance uses the configured provider to fetch actual secrets

This is valid because:
- The search path is not secret (which keyrings to check)
- The actual secret values are only fetched in the second Figment instance

### 4. Layering Order is Explicit

```rust
// Layer 1 merged first (lowest precedence)
.merge(ProviderA)
// Layer 2 merged second (overrides Layer 1)
.merge(ProviderB)
// Layer 3 merged last (highest precedence)
.merge(ProviderC)
```

When extracting:
1. All providers' `.data()` is called
2. Results are merged: `result = merge(merge(merge(A, B), C))`
3. If multiple providers return the same key, the LAST provider wins

---

## Practical Recommendations

### For Most Applications: Use Simple Layering

```rust
let config: Config = Figment::new()
    .merge(File::from("config.toml"))
    .merge(KeyringProvider::new("myapp", "api_key"))  // User keyring (default)
    .merge(Env::prefixed("MYAPP_"))
    .extract()?;
```

### For Different Deployments: Use Environment Configuration

```rust
// Development: MYAPP_KEYRING_TARGET=user (or unset)
// Production: MYAPP_KEYRING_TARGET=system

let config: Config = Figment::new()
    .merge(File::from("config.toml"))
    .merge(KeyringProvider::from_env("myapp", "api_key", "MYAPP_KEYRING_TARGET"))
    .extract()?;
```

### For Robust Fallback: Use Multi-Provider Pattern

```rust
let config: Config = Figment::new()
    .merge(File::from("config.toml"))
    .merge(KeyringProvider::new("myapp", "api_key").optional())           // User
    .merge(KeyringProvider::new("myapp", "api_key")
        .with_target(KeyringTarget::System).optional())    // System
    .merge(Env::prefixed("MYAPP_"))                              // Env
    .extract()?;
```

### For Configurable Search Paths: Use Opus KeyringSearch Pattern

```rust
// config.toml
[secrets]
keyrings = ["user", "team-secrets", "system"]

// Code
let search_path: KeyringConfig = Figment::new()
    .merge(File::from("config.toml"))
    .extract()?;

let config: Config = Figment::new()
    .merge(File::from("config.toml"))
    .merge(KeyringSearch::new("myapp", "api_key")
        .with_keyrings(search_path.keyrings))
    .extract()?;
```

---

## Common Confusions Clarified

### Q: Can a provider read from Figment?

**A:** Yes, but it's unusual. A provider typically reads from files, environment, databases, or keyrings - NOT from Figment itself.

**Valid use case:** Extracting configuration (like "which keyrings to search") from Figment to configure how a provider behaves.

**Invalid use case:** A provider calling `figment.extract()` internally - this creates infinite recursion.

### Q: Does Provider A see what Provider B returned?

**A:** No. Providers are completely isolated. Only Figment sees all providers' data.

### Q: How do I know which provider won a conflict?

**A:** The provider merged **last** wins. This is explicit and predictable:

```rust
Figment::new()
    .merge(ProviderA)  // Returns {"key": "value_a"}
    .merge(ProviderB)  // Returns {"key": "value_b"}
    .merge(ProviderC)  // Returns {"key": "value_c"}
    .extract()?
// Result: key = "value_c" (Provider C won)
```

### Q: Can I change the merge order dynamically?

**A:** Yes, build the chain conditionally:

```rust
let mut figment = Figment::new()
    .merge(File::from("config.toml"));

if use_keyring {
    figment = figment.merge(KeyringProvider::new("myapp", "api_key"));
}

if use_system_keyring {
    figment = figment.merge(KeyringProvider::new("myapp", "api_key")
        .with_target(KeyringTarget::System));
}

let config: Config = figment.extract()?;
```

### Q: What happens if two providers return the same key?

**A:** Last provider wins. This is the same as all Figment providers - files, env vars, etc.

### Q: How does `.optional()` affect layering?

**A:** An optional provider returns an empty map if it has no data. This doesn't prevent other providers from contributing.

```rust
Figment::new()
    .merge(File::from("config.toml"))           // Has {"key": "file_value"}
    .merge(KeyringProvider::new(...).optional()) // Keyring doesn't have entry, returns {}
    .merge(Env::prefixed("MYAPP_"))          // No env vars set, returns {}
    .extract()?
// Result: key = "file_value" (from file provider)
```

---

## Conclusion

The key to understanding Figment layering is:

1. **Providers are independent** - they don't communicate with each other
2. **Figment orchestrates** - it calls each provider and merges results
3. **Order matters** - later providers override earlier ones
4. **Figment-as-input is rare** - used for extracting configuration, not secrets
5. **Two-phase loading** is valid** - extract config path (Figment #1), then load secrets (Figment #2)

For most use cases, **simple layering** with `.optional()` for graceful fallback is the right choice. Use **environment configuration** for deployment-specific keyring selection. Use **KeyringSearch** only if you need complex, configurable search paths.

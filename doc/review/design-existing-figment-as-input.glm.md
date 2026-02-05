# Using Existing Figment Data as Provider Input

**Purpose:** Demonstrating how to use extracted configuration data to build providers dynamically

---

## Valid Pattern: Figment as Factory for Providers

Your question: "Can we use config that already exists during join?" 

**Answer: YES!** This is the **factory pattern** where configuration data drives provider creation.

### How It Works

```
Application Code
     │
     └─ Figment::new().merge(File::from("config.toml"))
         ↓
         └─ .extract::<KeyringConfig>()
              └─ Returns: KeyringConfig { keyrings: ["user", "team-secrets"] }
                    │
                    User code builds providers using extracted config
                    │
                    ├─ for keyring in keyring_config.keyrings {
                    │   └─ KeyringSearch::new(...).with_keyrings(&[keyring]) }
                    └─ Each provider uses the extracted config
```

### Key Insight

1. **`extract()` reads from Figment**, not from providers
2. **Providers are merged into Figment**, they don't see each other
3. **Any data in Figment is valid input** for creating new providers

### Valid Flow

```rust
use figment2::{Figment, providers::File};
use serde::Deserialize;

// Configuration that user will create
#[derive(Deserialize, Debug)]
struct KeyringConfig {
    /// Which keyrings to search, in priority order
    keyrings: Vec<String>,
}

// Application builds base Figment
let base_figment = Figment::new()
    .merge(File::from("config.toml"));

// Extract the keyring configuration
let keyring_config: KeyringConfig = base_figment.extract()?;

// Now build keyring providers using extracted config
let mut figment = Figment::new()
    .merge(File::from("config.toml"));

for keyring_name in keyring_config.keyrings {
    let provider = KeyringSearch::new("myapp", "api_key")
        .with_keyrings(vec![keyring_name.clone()]);
    
    figment = figment.merge(provider);
}

let config: Config = figment.extract()?;
```

---

## Why This Works

### 1. Data Flow is One-Directional

```
FileProvider.data()  ─┐
                      └─→ Figment stores data
                            └─→ .extract::<KeyringConfig>()
                                      └─→ Returns: KeyringConfig
                                                └─→ User uses to build providers
```

### 2. Two Separate Figments Are Fine

```rust
// Figment #1: Holds File data + extracted config
let base_figment = Figment::new()
    .merge(File::from("config.toml"))
    .merge(Env::prefixed("MYAPP_"));

let keyring_config = base_figment.extract::<KeyringConfig>()?;

// Figment #2: Holds File data + keyring providers
let secrets_figment = Figment::new()
    .merge(File::from("config.toml"));

for keyring in keyring_config.keyrings {
    secrets_figment = secrets_figment.merge(
        KeyringSearch::new("myapp", "api_key")
            .with_keyrings(vec![keyring])
    );
}

// Extract from secrets_figment (has keyring providers)
let config: Config = secrets_figment.extract()?;
```

**Both Figments have the same File data** - this is fine!

---

## Why This is Better Than Hardcoding

### Problem with Hardcoding

```rust
// Developer must maintain hardcoded keyrings
let config: Config = Figment::new()
    .merge(File::from("config.toml"))
    .merge(KeyringProvider::new("myapp", "api_key"))  // What keyring?
    .merge(KeyringProvider::new("myapp", "db_url"))   // What keyring?
    .extract()?;

// Operations team needs: keyrings = ["user"]
// Dev team needs: keyrings = ["user", "team-secrets"]
// Production team needs: keyrings = ["system"]
// Every team member needs: recompile for different keyring setup
```

### Solution with Figment-Driven Providers

```rust
// config.toml (each team maintains their own)
# operations.toml
[keyring_config]
keyrings = ["user", "team-secrets"]

# development.toml
[keyring_config]
keyrings = ["user", "team-secrets", "system"]

# production.toml
[keyring_config]
keyrings = ["system"]

// Code works for all teams - no recompilation needed
let figment = Figment::new()
    .merge(File::from("config.toml"))
    .merge(Env::prefixed("MYAPP_SECRETS_"));  // Override with env var

let keyring_config = figment.extract::<KeyringConfig>()?;

let mut secrets_figment = Figment::new()
    .merge(File::from("config.toml"));

for keyring in keyring_config.keyrings {
    secrets_figment = secrets_figment.merge(
        KeyringSearch::new("myapp", "api_key")
            .with_keyrings(vec![keyring])
    );
}

let config: Config = secrets_figment.extract()?;
```

---

## Comparison: Hardcoded vs Figment-Driven

| Aspect | Hardcoded | Figment-Driven |
|---------|-----------|-------------------|
| **Flexibility** | Hard to change keyring paths | Config file controls search |
| **Multi-team support** | Requires code changes per team | Same code works for all teams |
| **Deployment-specific** | Need different binaries | Config file per environment |
| **Code complexity** | Simple but inflexible | Slightly more complex |
| **Debuggability** | Hard to trace keyring choice | Config file shows search path |
| **Ops overhead** | Developer must coordinate | Ops manages config files |
| **Type safety** | Compile-time errors | Runtime errors (acceptable) |

---

## Advanced Pattern: Conditional Provider Creation

Use configuration to decide whether to include providers at all:

```rust
#[derive(Deserialize)]
struct AppConfig {
    /// If true, use keyring provider
    use_keyring: bool,
    
    /// If use_keyring is true, which type?
    keyring_type: Option<String>,
}

fn load_config() -> Result<Config, figment2::Error> {
    let app_config: AppConfig = Figment::new()
        .merge(File::from("config.toml"))
        .extract()?;

    let mut figment = Figment::new()
        .merge(File::from("config.toml"));

    if app_config.use_keyring {
        if let Some(keyring_type) = app_config.keyring_type {
            let provider = KeyringSearch::new("myapp", "api_key")
                .with_keyrings(vec![keyring_type]);
            
            figment = figment.merge(provider);
        }
    }

    let config: Config = figment.extract()?;
    Ok(config)
}
```

---

## Even More Advanced: Dynamic Provider Registry

Use extracted data to choose which provider factory to use:

```rust
#[derive(Deserialize)]
struct ProviderConfig {
    // Which provider to use for different secrets
    secret_providers: HashMap<String, String>,  // e.g., {"api_key": "keyring", "db_url": "file"}
}

fn load_config() -> Result<Config, figment2::Error> {
    // Get provider configuration
    let provider_config: ProviderConfig = Figment::new()
        .merge(File::from("config.toml"))
        .extract()?;

    let mut figment = Figment::new()
        .merge(File::from("config.toml"));

    // Build providers based on config
    for (secret_name, provider_type) in &provider_config.secret_providers {
        let provider = match provider_type.as_str() {
            "keyring" => KeyringSearch::new(secret_name, secret_name),
            "file" => File::from(format!("secrets/{}.toml", secret_name)),
            "env" => Env::prefixed(format!("SECRET_{}", secret_name)),
            _ => continue,  // Unknown type, skip
        };
        
        figment = figment.merge(provider);
    }

    let config: Config = figment.extract()?;
    Ok(config)
}
```

---

## Complete Example: Factory Pattern in Action

```rust
use figment2::{Figment, providers::File, providers::Env};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct KeyringConfig {
    keyrings: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Config {
    api_key: String,
    database_url: String,
}

fn load_config() -> Result<Config, figment2::Error> {
    // Step 1: Build base Figment
    let base_figment = Figment::new()
        .merge(File::from("config.toml"))
        .merge(Env::prefixed("MYAPP_"));

    // Step 2: Extract keyring configuration
    let keyring_config = base_figment.extract::<KeyringConfig>()?;

    println!("Using keyrings: {:?}", keyring_config.keyrings);

    // Step 3: Build dynamic keyring providers
    let mut secrets_figment = Figment::new()
        .merge(File::from("config.toml"));

    for keyring_name in keyring_config.keyrings {
        println!("Adding provider for keyring: {}", keyring_name);
        
        let provider = KeyringSearch::new("myapp", "api_key")
            .with_keyrings(vec![keyring_name.clone()]);
        
        secrets_figment = secrets_figment.merge(provider);
    }

    // Step 4: Extract final config
    let config: Config = secrets_figment.extract()?;
    
    println!("Loaded config: api_key={}", config.api_key);
    
    Ok(config)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    load_config()?;
    Ok(())
}
```

**Example Config File:**
```toml
# config.toml
[secrets]
keyrings = ["user", "team-alpha", "system"]
```

**Example Runs:**
```bash
# Development - uses user and team-alpha keyrings
$ cargo run
Using keyrings: ["user", "team-alpha"]
Adding provider for keyring: user
Adding provider for keyring: team-alpha
Loaded config: api_key=user_secret

# Production - uses only system keyring
$ MYAPP_SECRETS_KEYRINGS="system" cargo run
Using keyrings: ["system"]
Adding provider for keyring: system
Loaded config: api_key=prod_secret
```

---

## Key Advantages

### 1. **Separation of Concerns**
- Configuration (what keyrings to search) lives in config file
- Code (how to load) lives in application
- Ops team manages config, development team doesn't touch code

### 2. **No Recompilation**
- Different teams use same application binary
- Only need to update config file, not code

### 3. **Flexibility**
- Can add new keyring sources without code changes
- Can disable keyring entirely via config (`keyrings = []`)
- Can specify different search paths per secret

### 4. **Debuggability**
- Config file clearly shows which keyrings are being searched
- Easy to verify configuration before running application

---

## When This Pattern is Better Than Static

### Use Factory Pattern When:

✅ **Multiple deployment scenarios** need different keyring setups
✅ **Different teams** need different configurations
✅ **Configuration should be managed by ops**, not hardcode in code
✅ **Want to experiment** with different keyring sources
✅ **Users** should be able to disable keyring entirely

### Use Static Chain When:

✅ **Single deployment** with fixed keyring setup
✅ **Simple application** without complex configuration
✅ **Performance** is critical (no dynamic provider building overhead)

---

## Alternative: KeyringProvider Accepts Figment

```rust
impl KeyringProvider {
    /// Alternative constructor that accepts Figment for keyring paths
    pub fn from_figment(
        service: &str,
        credential_name: &str,
        figment: &Figment,
        keyring_config_key: &str,  // e.g., "keyring_config.keyrings"
    ) -> Self {
        let keyrings: figment.extract_inner(keyring_config_key)?;
        
        // Build provider using extracted keyrings
        let mut provider = KeyringSearch::new(service, credential_name);
        for keyring in keyrings {
            provider = provider.also(keyring);
        }
        
        provider
    }
}
```

**Usage:**
```rust
let base_figment = Figment::new()
    .merge(File::from("config.toml"))
    .merge(Env::prefixed("MYAPP_"));

let provider = KeyringProvider::from_figment(
    "myapp",
    "api_key",
    &base_figment,
    "keyring_config.keyrings"  // Path to Vec<String> in Figment
);
```

---

## Conclusion

**Yes, you can absolutely use existing Figment data to configure providers!** This is the factory pattern and is perfectly valid.

### Recommendation

For your use case (configurable keyring management):
- **Use factory pattern** (extract config, build providers)
- **Or** use `KeyringProvider::from_figment()` alternative constructor

Both approaches are valid and achieve your goal: allowing Figment to manage keyring search paths as first-class configuration.

**Key difference:**
- Factory pattern: More flexible, supports multiple secrets with one config structure
- Alternative constructor: More explicit, ties each provider to specific config path

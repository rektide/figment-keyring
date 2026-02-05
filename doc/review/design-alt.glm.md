# Alternative Design Approaches - Figment Keyring Provider

**Document:** Analysis of structural alternatives to converged design  
**Date:** 2026-02-05  
**Reviewer:** opencode (GLM)

---

## Overview

The three design rewrite documents (GLM, Opus, Kimi) have converged on a nearly identical `KeyringProvider` structure using a builder pattern. This document analyzes alternative structural approaches that were considered and evaluated, providing rationale for accepting the converged design and rejecting alternatives.

## Converged Design Summary

All three rewrite proposals use a similar builder pattern:

```rust
use std::collections::BTreeMap;
use figment2::{Provider, Error, Metadata, Profile, value::Value};

pub struct KeyringProvider {
    service: String,              // Keyring service identifier
    credential_name: String,        // Keyring entry name (or "username")
    config_key: Option<String>,     // Maps to different config key if set
    namespace: Option<String>,      // Prepends prefix to config key
    optional: bool,               // Silent skip on missing entry
    profile: Option<Profile>,     // Target specific profile (None = all)
}

impl KeyringProvider {
    pub fn new(service: &str, credential_name: &str) -> Self;
    pub fn map_to(self, key: &str) -> Self;
    pub fn as_key(self, key: &str) -> Self;
    pub fn with_namespace(self, namespace: &str) -> Self;
    pub fn optional(self) -> Self;
    pub fn with_profile(self, profile: Profile) -> Self;
}
```

This design emerged from cross-review synthesis addressing consensus concerns:
- Error handling with optional/fallback semantics
- Explicit key mapping to avoid implicit magic
- Namespace support to prevent collisions
- Thread safety guarantees
- Clear testing strategy

---

## Alternative Approaches

### Alternative 1: Configuration-Driven (Declarative)

Instead of constructing providers programmatically, load keyring configuration from a file or from the Figment configuration itself.

```rust
#[derive(Deserialize, Debug)]
pub struct KeyringConfig {
    service: String,
    entries: Vec<KeyringEntry>,
}

#[derive(Deserialize, Debug)]
pub struct KeyringEntry {
    credential: String,
    config_key: Option<String>,
    optional: bool,
    profile: Option<String>,
}

impl Provider for KeyringConfig {
    fn data(&self) -> Result<BTreeMap<Profile, BTreeMap<String, Value>>, Error> {
        let mut result = BTreeMap::new();
        for entry in &self.entries {
            // Load each entry and merge into result
        }
        Ok(result)
    }
}

impl KeyringConfig {
    pub fn extract_from_figment(figment: &Figment) -> Result<Self, Error> {
        // Extract keyring config from existing Figment instance
        figment.extract::<KeyringConfig>()
    }
}
```

**Usage Example:**
```toml
# config.toml
[keyring]
service = "myapp"

[[keyring.entries]]
credential = "api_key"
optional = false

[[keyring.entries]]
credential = "database_url"
optional = true
config_key = "db_url"
profile = "production"
```

```rust
// In application code
let config: Config = Figment::new()
    .merge(File::from("config.toml"))
    .merge(KeyringConfig::extract_from_figment(&figment)?)
    .merge(Env::prefixed("MYAPP_"))
    .extract()?;
```

**Advantages:**
- Declarative configuration matches Figment's philosophy
- Easier to version control keyring entry expectations
- Configuration and secret management in one place
- Non-developers can modify which secrets are loaded without code changes

**Disadvantages:**
- More complex API surface (two ways to configure)
- Less discoverable for developers (have to read config file to see what's loaded)
- Coupling between config file structure and application code
- Type checking happens at config parse time, not compile time
- May not align with Figment2's existing provider patterns

**Verdict:** Consider as secondary constructor for v1.1 if users request declarative configuration

---

### Alternative 2: Trait-Based Backend Abstraction

Make the keyring backend pluggable for testing flexibility and potential alternative backends.

```rust
pub trait KeyringBackend: Send + Sync {
    fn get(&self, service: &str, credential: &str) -> Result<Option<String>, KeyringError>;
}

// Real backend using system keyring
pub struct SystemKeyringBackend;
impl KeyringBackend for SystemKeyringBackend {
    fn get(&self, service: &str, credential: &str) -> Result<Option<String>, KeyringError> {
        // Use keyring crate
    }
}

// Mock backend for testing (no feature flags needed)
pub struct MockKeyringBackend {
    entries: HashMap<(String, String), String>,
    fail_on: HashSet<(String, String)>,
}

impl KeyringBackend for MockKeyringBackend {
    fn get(&self, service: &str, credential: &str) -> Result<Option<String>, KeyringError> {
        if self.fail_on.contains(&(service.to_string(), credential.to_string())) {
            return Err(KeyringError::PermissionDenied);
        }
        Ok(self.entries.get(&(service.to_string(), credential.to_string())).cloned())
    }
}

// Generic provider
pub struct KeyringProvider<B: KeyringBackend> {
    backend: B,
    service: String,
    credential: String,
    config_key: Option<String>,
    namespace: Option<String>,
    optional: bool,
    profile: Option<Profile>,
}

// Convenience constructors for real usage
impl KeyringProvider<SystemKeyringBackend> {
    pub fn new(service: &str, credential: &str) -> Self {
        Self {
            backend: SystemKeyringBackend,
            service: service.to_string(),
            credential: credential.to_string(),
            config_key: None,
            namespace: None,
            optional: false,
            profile: None,
        }
    }
}

// Testing constructor
impl KeyringProvider<MockKeyringBackend> {
    pub fn with_mock(
        mock: MockKeyringBackend,
        service: &str,
        credential: &str,
    ) -> Self {
        Self {
            backend: mock,
            service: service.to_string(),
            credential: credential.to_string(),
            config_key: None,
            namespace: None,
            optional: false,
            profile: None,
        }
    }
}
```

**Usage Example:**
```rust
// Production code
let provider = KeyringProvider::new("myapp", "api_key");

// Test code
let mock = MockKeyringBackend::new()
    .with_entry("myapp", "api_key", "test_secret");
let provider = KeyringProvider::with_mock(mock, "myapp", "api_key");
```

**Advantages:**
- Clean testing without feature flags or build-time configuration
- Explicit dependency on keyring backend
- Allows alternative backends (e.g., encrypted file backend for CI/CD)
- Type-safe backend selection at compile time
- Clear separation between provider logic and backend access

**Disadvantages:**
- Generic type parameter adds complexity (`KeyringProvider<SystemKeyringBackend>`)
- Type inference challenges in chained builder patterns
- Documentation must explain the generic parameter
- More verbose API for simple use cases
- Two constructor patterns (`::new` and `::with_mock`) could be confusing

**Verdict:** Overkill for current requirements. Feature flag-based mock is simpler and sufficient.

---

### Alternative 3: Enum-Based Provider (Single vs Multi)

Distinguish between single-entry and multi-entry providers using an enum to make semantics explicit.

```rust
pub enum KeyringProvider {
    Single {
        service: String,
        credential: String,
        config_key: Option<String>,
        namespace: Option<String>,
        optional: bool,
        profile: Option<Profile>,
    },
    Multi {
        service: String,
        entries: Vec<CredentialMapping>,
    },
}

pub struct CredentialMapping {
    credential: String,
    config_key: Option<String>,
    optional: bool,
    profile: Option<Profile>,
}

impl KeyringProvider {
    // Constructor for single entry
    pub fn single(service: &str, credential: &str) -> Self {
        KeyringProvider::Single {
            service: service.to_string(),
            credential: credential.to_string(),
            config_key: None,
            namespace: None,
            optional: false,
            profile: None,
        }
    }

    // Constructor for multiple entries
    pub fn multi(service: &str) -> MultiBuilder {
        MultiBuilder::new(service.to_string())
    }

    // Builder methods for single variant
    pub fn map_to(mut self, key: &str) -> Self {
        match &mut self {
            KeyringProvider::Single { config_key, .. } => {
                *config_key = Some(key.to_string());
            }
            KeyringProvider::Multi { .. } => {
                // Multi variant uses separate builder
            }
        }
        self
    }
}

pub struct MultiBuilder {
    service: String,
    entries: Vec<CredentialMapping>,
}

impl MultiBuilder {
    pub fn new(service: String) -> Self {
        Self {
            service,
            entries: Vec::new(),
        }
    }

    pub fn with_entry(mut self, credential: &str) -> EntryBuilder {
        EntryBuilder::new(self, credential)
    }

    pub fn build(self) -> KeyringProvider {
        KeyringProvider::Multi {
            service: self.service,
            entries: self.entries,
        }
    }
}

pub struct EntryBuilder<'a> {
    builder: &'a mut MultiBuilder,
    credential: String,
    config_key: Option<String>,
    optional: bool,
    profile: Option<Profile>,
}

impl EntryBuilder<'_> {
    fn new(builder: &mut MultiBuilder, credential: String) -> EntryBuilder {
        EntryBuilder {
            builder,
            credential,
            config_key: None,
            optional: false,
            profile: None,
        }
    }

    pub fn map_to(mut self, key: &str) -> Self {
        self.config_key = Some(key.to_string());
        self
    }

    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    pub fn add(self) -> &mut MultiBuilder {
        self.builder.entries.push(CredentialMapping {
            credential: self.credential,
            config_key: self.config_key,
            optional: self.optional,
            profile: self.profile,
        });
        self.builder
    }
}
```

**Usage Example:**
```rust
// Single entry
let provider = KeyringProvider::single("myapp", "api_key")
    .map_to("key")
    .optional();

// Multiple entries
let provider = KeyringProvider::multi("myapp")
    .with_entry("api_key").optional().add()
    .with_entry("database_url").map_to("db_url").add()
    .with_entry("jwt_secret").add()
    .build();
```

**Advantages:**
- Explicit distinction between single and multi-entry behavior
- No ambiguity about partial failure semantics (single = fail-fast, multi = collect all)
- Enum variants document behavior at type level
- Multi-builder can validate entries at build time

**Disadvantages:**
- Enum pattern matching overhead in implementation
- Complex nested builder for multi-entry variant
- API surface fragmentation (different constructors, different builder patterns)
- Two different patterns to learn
- More code to maintain and test
- Type complexity for users (`match` on provider if needed)

**Verdict:** Unnecessary complexity. Single-entry pattern with multiple `.merge()` calls is idiomatic Figment2.

---

### Alternative 4: Structured Configuration via JSON

Embrace structured data (JSON) from the start instead of treating it as a future enhancement.

```rust
pub struct KeyringProvider {
    service: String,
    credential: String,
    mode: KeyringMode,
}

pub enum KeyringMode {
    String {
        config_key: Option<String>,
        optional: bool,
    },
    Json {
        merge_strategy: MergeStrategy,
    },
}

pub enum MergeStrategy {
    Root,                    // Merge JSON keys into config root
    Flatten { prefix: String }, // Flatten and prefix keys
    NestedAt { path: String },  // Nest at specific path
}

impl KeyringProvider {
    pub fn new(service: &str, credential: &str) -> Self {
        Self {
            service: service.to_string(),
            credential: credential.to_string(),
            mode: KeyringMode::String {
                config_key: None,
                optional: false,
            },
        }
    }

    pub fn as_json(self) -> Self {
        Self {
            mode: KeyringMode::Json {
                merge_strategy: MergeStrategy::Root,
            },
            ..self
        }
    }

    pub fn json_at_path(self, path: &str) -> Self {
        Self {
            mode: KeyringMode::Json {
                merge_strategy: MergeStrategy::NestedAt {
                    path: path.to_string(),
                },
            },
            ..self
        }
    }

    // Standard builder methods for string mode
    pub fn map_to(mut self, key: &str) -> Self {
        if let KeyringMode::String { config_key, .. } = &mut self.mode {
            *config_key = Some(key.to_string());
        }
        self
    }

    pub fn optional(mut self) -> Self {
        if let KeyringMode::String { optional, .. } = &mut self.mode {
            *optional = true;
        }
        self
    }
}
```

**Usage Example:**
```rust
// Simple string mode (current behavior)
let provider = KeyringProvider::new("myapp", "api_key")
    .optional();
// Keyring contains: "my_api_key_123"
// Config: { "api_key": "my_api_key_123" }

// JSON mode - merge to root
let provider = KeyringProvider::new("myapp", "credentials")
    .as_json();
// Keyring contains: { "api_key": "...", "database_url": "...", "jwt_secret": "..." }
// Config: { "api_key": "...", "database_url": "...", "jwt_secret": "..." }

// JSON mode - nested at path
let provider = KeyringProvider::new("myapp", "credentials")
    .json_at_path("secrets");
// Keyring contains: { "api_key": "...", "database_url": "..." }
// Config: { "secrets": { "api_key": "...", "database_url": "..." } }

// JSON mode - flatten with prefix
let provider = KeyringProvider::new("myapp", "credentials")
    .json_mode(KeyringMode::Json {
        merge_strategy: MergeStrategy::Flatten {
            prefix: "secret".to_string(),
        },
    });
// Keyring contains: { "api_key": "...", "database_url": "..." }
// Config: { "secret.api_key": "...", "secret.database_url": "..." }
```

**Advantages:**
- Handles both simple and complex use cases in one API
- Future-proof for structured secrets
- Reduces need for multiple provider instances
- More ergonomic for applications with many secrets

**Disadvantages:**
- JSON parsing in critical path adds complexity
- Malformed JSON handling adds error cases
- Type conversion issues (JSON may contain wrong types)
- Debugging harder (is error JSON parsing or keyring access?)
- Harder to document (two modes with different semantics)
- May encourage putting too much data in keyring (anti-pattern)

**Verdict:** Defer until real-world usage shows demand. Simple strings cover 80% of cases.

---

### Alternative 5: Error Callback Strategy

Allow custom error handling logic to be injected at construction time.

```rust
use std::sync::Arc;

pub struct KeyringProvider {
    service: String,
    credential: String,
    config_key: Option<String>,
    namespace: Option<String>,
    profile: Option<Profile>,
    optional: bool,
    on_error: Option<Arc<dyn ErrorHandler>>,
}

pub trait ErrorHandler: Send + Sync {
    fn handle(&self, error: &KeyringError) -> ErrorAction;
}

pub type ErrorHandlerFn = dyn Fn(&KeyringError) -> ErrorAction + Send + Sync;

pub enum ErrorAction {
    Fail,                     // Return error (default behavior)
    Skip,                     // Return empty map (like .optional())
    Retry { delay: Duration },  // Retry after delay
    Fallback(String),          // Provide fallback value instead
    Recover(Box<dyn FnOnce() -> Result<String, String> + Send>),
}

impl KeyringProvider {
    pub fn new(service: &str, credential: &str) -> Self {
        Self {
            service: service.to_string(),
            credential: credential.to_string(),
            config_key: None,
            namespace: None,
            optional: false,
            profile: None,
            on_error: None,
        }
    }

    pub fn on_error<F>(mut self, handler: F) -> Self
    where
        F: Fn(&KeyringError) -> ErrorAction + Send + Sync + 'static,
    {
        self.on_error = Some(Arc::new(handler));
        self
    }

    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }
}

impl Provider for KeyringProvider {
    fn data(&self) -> Result<BTreeMap<Profile, BTreeMap<String, Value>>, Error> {
        let result = match self.fetch_from_keyring() {
            Ok(value) => value,
            Err(error) => {
                let action = if let Some(handler) = &self.on_error {
                    handler.handle(&error)
                } else if self.optional {
                    ErrorAction::Skip
                } else {
                    ErrorAction::Fail
                };

                match action {
                    ErrorAction::Fail => return Err(error.into()),
                    ErrorAction::Skip => BTreeMap::new(),
                    ErrorAction::Retry { delay } => {
                        std::thread::sleep(delay);
                        return self.data(); // Recurse (could loop forever!)
                    },
                    ErrorAction::Fallback(value) => {
                        self.format_value(&value)
                    },
                    ErrorAction::Recover(f) => {
                        match f() {
                            Ok(recovered) => self.format_value(&recovered),
                            Err(msg) => return Err(Error::custom(msg)),
                        }
                    },
                }
            }
        };
        Ok(result)
    }
}
```

**Usage Example:**
```rust
// Simple error handling
let provider = KeyringProvider::new("myapp", "api_key")
    .optional();

// Custom error handling with retry
let provider = KeyringProvider::new("myapp", "api_key")
    .on_error(|err| match err {
        KeyringError::ServiceUnavailable => ErrorAction::Retry {
            delay: Duration::from_secs(5),
        },
        KeyringError::EntryNotFound => ErrorAction::Skip,
        _ => ErrorAction::Fail,
    });

// Custom error handling with fallback
let provider = KeyringProvider::new("myapp", "api_key")
    .on_error(|err| match err {
        KeyringError::EntryNotFound => ErrorAction::Fallback("default_key".to_string()),
        _ => ErrorAction::Fail,
    });

// Complex recovery logic
let provider = KeyringProvider::new("myapp", "api_key")
    .on_error(|err| match err {
        KeyringError::EntryNotFound => {
            // Try environment variable as fallback
            if let Ok(value) = std::env::var("MYAPP_API_KEY") {
                ErrorAction::Fallback(value)
            } else {
                ErrorAction::Fail
            }
        },
        _ => ErrorAction::Fail,
    });
```

**Advantages:**
- Maximum flexibility for error handling
- Supports retry logic without external machinery
- Allows environment-specific behavior (retry in dev, fail in prod)
- Fallback values without other providers

**Disadvantages:**
- Over-engineering for most use cases
- Callbacks make control flow hard to follow
- Recursion risk in retry logic (could loop forever)
- Error handling logic lives in configuration, not application code
- Type signature becomes complex (`Arc<dyn ErrorHandler>`)
- Harder to test (need to test callback logic)
- Cognitive load: when to use `.optional()` vs `.on_error()`?

**Verdict:** Over-engineering. Standard error types and `.optional()` cover 95% of cases.

---

### Alternative 6: Lazy Evaluation with Caching

Defer keyring access until config extraction and cache the result for subsequent calls.

```rust
use std::sync::{Arc, OnceLock};

pub struct KeyringProvider {
    state: Arc<ProviderState>,
    config_key: Option<String>,
    namespace: Option<String>,
    profile: Option<Profile>,
    optional: bool,
}

struct ProviderState {
    service: String,
    credential: String,
    cache: OnceLock<Result<BTreeMap<Profile, BTreeMap<String, Value>>, KeyringError>>,
}

impl KeyringProvider {
    pub fn new(service: &str, credential: &str) -> Self {
        Self {
            state: Arc::new(ProviderState {
                service: service.to_string(),
                credential: credential.to_string(),
                cache: OnceLock::new(),
            }),
            config_key: None,
            namespace: None,
            optional: false,
            profile: None,
        }
    }

    fn fetch_and_cache(&self) -> Result<BTreeMap<Profile, BTreeMap<String, Value>>, KeyringError> {
        self.state.cache.get_or_init(|| {
            // Fetch from keyring
            let value = fetch_from_keyring(&self.state.service, &self.state.credential)?;
            let formatted = self.format_value(&value)?;
            Ok(formatted)
        }).clone()
    }
}

impl Provider for KeyringProvider {
    fn data(&self) -> Result<BTreeMap<Profile, BTreeMap<String, Value>>, Error> {
        match self.fetch_and_cache() {
            Ok(data) => Ok(data),
            Err(err) if self.optional => Ok(BTreeMap::new()),
            Err(err) => Err(err.into()),
        }
    }
}

impl Clone for KeyringProvider {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            config_key: self.config_key.clone(),
            namespace: self.namespace.clone(),
            profile: self.profile.clone(),
            optional: self.optional,
        }
    }
}
```

**Usage Example:**
```rust
let provider = KeyringProvider::new("myapp", "api_key");

// First call - fetches from keyring
let data1 = provider.data()?;

// Second call - returns cached value (no keyring access)
let data2 = provider.data()?;

// Cloned provider shares cache
let provider2 = provider.clone();
let data3 = provider2.data(); // Cached, no keyring access
```

**Advantages:**
- Avoids repeated keyring IPC calls
- Clear about caching semantics (explicit in API)
- Shared cache across cloned providers
- Performance benefit for repeated `.data()` calls
- No external caching machinery needed

**Disadvantages:**
- Internal mutability complexity (`Arc`, `OnceLock`)
- Premature optimization - Figment2 typically calls `.data()` once
- Cache never invalidates (stale data if keyring changes)
- `OnceLock` doesn't support refresh (would need `RwLock` for reload)
- More complex implementation
- Cache behavior subtle (shared across clones, when does it clear?)

**Verdict:** Premature optimization. Figment2's `extract()` is called once at startup.

---

## Recommendation: Accept Converged Design

The converged builder-pattern design from the three rewrite documents should be accepted for implementation.

### Why the Converged Design is Superior

1. **Simplicity**
   - Builder pattern is idiomatic Rust
   - Easy to understand and use
   - No generics or enums to explain
   - Methods autocomplete and self-document

2. **Discoverability**
   - All available options visible via `.map_to()`, `.optional()`, etc.
   - IDE can show all builder methods
   - No need to read config files to understand behavior

3. **Flexibility**
   - Adding new features (namespace, profile) doesn't break existing code
   - Builder methods are opt-in
   - Backward compatible evolution

4. **Testability**
   - Feature flag-based mock backend is straightforward
   - Tests are clear and explicit
   - No generic type parameters to manage

5. **Alignment with Figment2**
   - Follows existing provider patterns
   - `.merge()` chaining is idiomatic
   - Fits naturally into Figment ecosystem

6. **Cognitive Load**
   - Single mental model: one provider = one secret
   - Clear error handling: either fails or is optional
   - No callback complexity or state machines

### One Enhancement Worth Considering

Add a secondary constructor for declarative configuration use cases (from Alternative 1):

```rust
impl KeyringProvider {
    // Primary API (current converged design)
    pub fn new(service: &str, credential: &str) -> Self { ... }

    // Secondary API for declarative configs (v1.1 enhancement)
    pub fn from_config(config: KeyringConfig) -> Vec<Self> {
        config.entries.into_iter()
            .map(|entry| Self::new(&config.service, &entry.credential)
                .maybe_map_to(entry.config_key)
                .maybe_optional(entry.optional)
                .maybe_profile(entry.profile))
            .collect()
    }
}

// Helper trait for optional builder methods
trait MaybeBuilder<T>: Sized {
    fn maybe_map_to(self, key: Option<String>) -> T;
    fn maybe_optional(self, optional: bool) -> T;
    fn maybe_profile(self, profile: Option<Profile>) -> T;
}

impl<T> MaybeBuilder<T> for T
where
    T: KeyringBuilder,
{
    fn maybe_map_to(self, key: Option<String>) -> T {
        match key {
            Some(k) => self.map_to(&k),
            None => self,
        }
    }

    fn maybe_optional(self, optional: bool) -> T {
        if optional {
            self.optional()
        } else {
            self
        }
    }

    fn maybe_profile(self, profile: Option<Profile>) -> T {
        match profile {
            Some(p) => self.with_profile(p),
            None => self,
        }
    }
}
```

**Usage:**
```rust
#[derive(Deserialize)]
struct KeyringConfig {
    service: String,
    entries: Vec<KeyringEntry>,
}

// In code
let config: KeyringConfig = figment.extract()?;
let providers = KeyringProvider::from_config(config);
let figment = Figment::new()
    .merge(File::from("base.toml"));
for provider in providers {
    figment = figment.merge(provider);
}
let result = figment.extract::<Config>()?;
```

This provides declarative configuration as an **optional** feature without complicating the primary API.

---

## Rejected Alternatives and Rationale

| Alternative | Status | Rationale |
|------------|--------|------------|
| **Alt 1: Config-driven** | Defer to v1.1 | Nice enhancement, but secondary constructor preserves primary API simplicity |
| **Alt 2: Generic backend** | Reject | Overkill; feature flag is simpler and sufficient |
| **Alt 3: Enum provider** | Reject | Unnecessary complexity; single-entry pattern is idiomatic |
| **Alt 4: JSON mode** | Defer indefinitely | Strings cover 80% of cases; wait for real-world demand |
| **Alt 5: Error callbacks** | Reject | Over-engineering; standard errors + `.optional()` cover 95% of cases |
| **Alt 6: Lazy caching** | Reject | Premature optimization; Figment2 calls `.data()` once at startup |

---

## Conclusion

The converged design from the three rewrite documents represents the optimal balance of simplicity, flexibility, and discoverability for the Figment Keyring Provider. The builder pattern:

- Aligns with Rust idioms and Figment2 patterns
- Provides clear, discoverable API
- Supports future extensibility without breaking changes
- Is easy to test and reason about

Alternatives were evaluated but rejected due to:
- Unnecessary complexity (generic backends, enum providers)
- Premature optimization (caching)
- Over-engineering (error callbacks)
- Unproven need (JSON mode, config-driven)

The design is ready for implementation as specified in the converged documents.

---

## Appendix: Comparison Summary

| Feature | Converged | Alt 1 | Alt 2 | Alt 3 | Alt 4 | Alt 5 | Alt 6 |
|---------|-------------|---------|---------|---------|---------|---------|---------|
| **Simplicity** | ✅ Simple | ✅ Simple | ❌ Generic | ❌ Complex | ❌ Complex | ❌ Complex | ⚠️ Moderate |
| **Discoverability** | ✅ High | ⚠️ Low | ✅ High | ⚠️ Low | ⚠️ Low | ⚠️ Low | ✅ High |
| **Testability** | ✅ Easy | ✅ Easy | ✅ Easy | ⚠️ Moderate | ⚠️ Moderate | ⚠️ Hard | ⚠️ Moderate |
| **Flexibility** | ✅ High | ✅ High | ✅ High | ✅ High | ✅ High | ✅ Very High | ⚠️ Moderate |
| **Performance** | ✅ Good | ✅ Good | ✅ Good | ✅ Good | ⚠️ Overhead | ✅ Good | ✅ Good |
| **Complexity** | ✅ Low | ⚠️ Moderate | ⚠️ Moderate | ❌ High | ❌ High | ❌ High | ⚠️ Moderate |
| **Idiomatic** | ✅ Rust | ⚠️ Mixed | ✅ Rust | ⚠️ Mixed | ⚠️ Mixed | ⚠️ Unusual | ✅ Rust |

**Recommendation:** Accept converged design, consider Alt 1 as v1.1 enhancement.

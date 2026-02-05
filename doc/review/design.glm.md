# Design Review: Figment Keyring Provider

**Reviewer:** opencode  
**Date:** 2024-02-05  
**Document:** doc/design.md

---

## Executive Summary

The design document is well-structured and presents a clear solution for integrating system keyrings with Figment2. The proposed `KeyringProvider` follows good design patterns and integrates cleanly with Figment2's layered configuration model. However, several areas need clarification or additional consideration around error handling, testing strategy, ergonomics, and edge cases.

This review incorporates insights from concurrent reviews by Opus and Kimi, covering complementary concerns around API ergonomics, operational considerations, and security aspects.

**Overall Assessment:** ✅ Approved with recommendations

---

## Cross-Review Synthesis

Three independent reviews were conducted, revealing consistent themes and complementary insights:

**All three reviewers identified:**
- Error handling semantics need clarification (missing entries vs unavailable service)
- Testing strategy is missing and critical
- Profile support in `data()` return type is unexplained
- Optional/fallback semantics not addressed

**Opus review identified:**
- Username→config key mapping is implicit magic
- Headless/service environment concerns
- Multi-secret convenience constructor suggestion
- Backwards precedence comment in usage example

**Kimi review identified:**
- Batch retrieval API for reducing verbosity
- Secret rotation and audit logging considerations
- Keyring entry creation/permissions discussion
- Alternative approaches not considered
- Service/username terminology confusion

**This review identified:**
- Threading/concurrency not addressed
- Performance and caching considerations
- Namespace support priority
- Platform-specific behavior (headless, locked keyring)

---

## Strengths

### 1. Clear Problem Statement
- Effectively identifies the security risks of plaintext configuration
- Makes a compelling case for using system keyrings
- Positions the solution clearly within the Figment2 ecosystem

### 2. Well-Structured Architecture
- Clean separation of concerns
- Simple, focused API design
- Good integration with existing Figment2 patterns
- Clear precedence model explanation

### 3. Practical Tradeoffs Documented
- Honest about limitations (single value per provider, no structured data)
- Clear on security considerations
- Realistic about in-memory plaintext exposure

### 4. Future Enhancements Well-Considered
- JSON/structured secrets
- Entry discovery
- Namespace support
- Validation and caching

---

## Concerns & Recommendations

### 🔴 Must-Have Before Implementation

#### 1. Error Handling Strategy (Lines 117-123)
**Issue:** Error handling is vague. How does a missing keyring entry behave vs. a keyring service being unavailable?

**Recommendation:**
- Explicitly distinguish between error types:
  - "Entry not found" → soft error, allow other providers to supply value
  - "Permission denied" → fatal, security issue
  - "Keyring service unavailable" → configurable (warn/fatal)
  - "Backend error" → fatal
- Provide `.optional()` builder method for non-critical secrets
- Document error behavior in provider construction

```rust
impl KeyringProvider {
    pub fn optional(&self) -> Self; // Returns None if missing, doesn't fail
    pub fn exists(&self) -> Result<bool, Error>; // Check without retrieval
}
```

#### 2. Username→Config Key Mapping (All Reviews)
**Issue:** The `username` parameter becomes the config key name implicitly. This is undocumented magic and may cause confusion.

**Recommendation:**
- Document this behavior prominently in API docs
- Add `.map_to()` or `.as_key()` builder method for explicit mapping
- Consider type aliases to clarify semantics:

```rust
pub type Service = String;
pub type CredentialName = String; // Instead of "username"

// Usage:
KeyringProvider::new("myapp", "prod_api_key")
    .map_to("api_key")  // Config sees "api_key", not "prod_api_key"
```

#### 3. Testing Strategy (All Reviews)
**Issue:** No discussion of testing approach. Keyring access is notoriously difficult to test in automated environments.

**Recommendations:**
- Add dedicated testing section covering:
  - Mock keyring backend for unit tests
  - Integration tests for each platform (macOS, Linux, Windows)
  - CI/CD configuration for platform-specific tests
  - Conditional compilation with feature flags
  - Example test cases

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "mock-keyring")]
    use mock_keyring::MockKeyring;

    #[test]
    fn test_retrieves_single_value() { /* ... */ }
    #[test]
    fn test_handles_missing_entry_gracefully() { /* ... */ }
    #[test]
    fn test_optional_secret_skips_on_missing() { /* ... */ }
}
```

#### 4. Profile Handling in Configuration Model (All Reviews)
**Issue:** The `data()` method returns `BTreeMap<Profile, BTreeMap<String, Value>>`, but the design never explains how profiles work with keyring entries.

**Recommendation:**
- Explicitly document Profile behavior:
  - Does keyring provide same value across all profiles?
  - Can user target specific profiles via `.with_profile(Profile::Prod)`?
  - Or return value under default profile only?
- Clarify whether profiles are supported or explicitly document as out-of-scope

```rust
// Design decision needed:
// Option A: Same value for all profiles
fn data(&self) -> Result<BTreeMap<Profile, BTreeMap<String, Value>>, Error> {
    let mut map = BTreeMap::new();
    let value = self.get_from_keyring()?;
    for profile in Profile::all() {
        map.insert(profile, btreemap! { "api_key": value.clone() });
    }
    Ok(map)
}

// Option B: User-specified profile
KeyringProvider::new("myapp", "api_key")
    .with_profile(Profile::Production)
```

### 🟡 Should-Have

#### 5. Batch/Multi-Secret API (Opus, Kimi)
**Issue:** Single value per provider requires verbose API for multiple secrets and causes repeated IPC calls.

**Recommendation:**
- Add convenience API for batch retrieval:
  - Reduces boilerplate
  - Potentially batches keyring access (backend-dependent)
  - Better ergonomics for common case

```rust
// Multiple providers (verbose):
.merge(KeyringProvider::new("myapp", "api_key"))
.merge(KeyringProvider::new("myapp", "database_url"))
.merge(KeyringProvider::new("myapp", "jwt_secret"))

// Batch API (ergonomic):
.merge(KeyringProvider::multi("myapp", &["api_key", "database_url", "jwt_secret"]))

// Or builder pattern:
KeyringProvider::for_service("myapp")
    .with_entries(&["api_key", "database_url"])
```

#### 6. Thread Safety (This Review)
**Issue:** Design doesn't address thread safety. Figment2's `Figment::new().merge(...)` chain can be used across threads.

**Recommendation:**
- Explicitly document thread safety guarantees
- If `KeyringProvider` is not `Send`/`Sync`, document restriction
- Consider using `Arc` for shared provider instances
- Document behavior of `data()` call under concurrent access

#### 7. Headless/Server Environments (Opus, This Review)
**Issue:** System keyrings often require user session context. Design doesn't address CI/CD, Docker, systemd services.

**Recommendation:**
- Add operational guidance section:
  - Document failure modes for headless environments
  - Recommend environment variables or file providers as fallbacks
  - Provide examples for CI/CD workflows
  - Document platform-specific behavior (SSH, GUI prompts)

```rust
// Example: Graceful degradation for headless
#[cfg(feature = "keyring-fallback")]
let provider = KeyringProvider::new("myapp", "api_key")
    .or_else(|| Env::var("MYAPP_API_KEY").map(EnvProvider::single));

// Or documentation:
// "In CI environments, prefer environment variables. Keyring provider will
//  fail gracefully if keyring service is unavailable when .optional() is used."
```

#### 8. Secret Rotation and Runtime Updates (Kimi)
**Issue:** Design states "retrieved once at configuration loading" but doesn't address secret rotation or long-running processes.

**Recommendation:**
- Document that values are cached at load time
- Consider optional reload mechanism:
  - `.reload_interval(Duration::from_secs(300))`
  - Manual `reload()` method
- Document that rotation requires application restart (acceptable for most use cases)

#### 9. Keyring Entry Creation and Permissions (Kimi)
**Issue:** Design doesn't address how entries are created or their access control.

**Recommendation:**
- Document entry creation workflow:
  - Who creates entries? (app developer, operator, setup script?)
  - Should library provide CLI tool for management?
  - Example commands for each platform:
    ```bash
    # macOS
    security add-generic-password -s myapp -a api_key -w "secret"

    # Linux (secret-tool)
    secret-tool store --label='myapp api_key' service myapp username api_key
    ```
- Document recommended permissions/attributes
- Consider providing helper utility crate or CLI

### 🟢 Nice-to-Have

#### 10. Performance and Caching (This Review)
**Issue:** Security considerations don't discuss performance implications of keyring access.

**Recommendation:**
- Document keyring access costs (IPC, encryption/decryption)
- Clarify whether multiple providers cause repeated calls
- Document caching strategy (single load at `data()` call)
- Consider benchmarks in future work

#### 11. Namespace Support Priority (This Review)
**Suggestion:** Elevate namespace support from "future enhancement" to v1. Without it, keyring keys could conflict with config file keys.

```rust
// Avoid conflicts:
KeyringProvider::new("myapp", "api_key")
    .with_namespace("secrets")
// Produces: { "secrets.api_key": "..." }
```

#### 12. Alternative Approaches (Kimi)
**Suggestion:** Briefly explain why system keyring was chosen over alternatives to strengthen design rationale:
- Environment variables with file paths (`MYAPP_API_KEY_FILE`)
- Secret management services (AWS Secrets Manager, Vault)
- File-based encryption (age, sops)

#### 13. Audit Logging (Kimi)
**Suggestion:** Consider optional audit trail:
- Log keyring access at DEBUG level
- Distinguish cache hits vs actual keyring access
- Help operators understand configuration loading

#### 14. Usage Example Issues (Opus)
**Issue:** Comment in example (line 93) says "Keyring fallback" but keyring is merged last (highest precedence).

**Correction:**
```rust
Figment::new()
    .merge(File::from("config.toml"))           // Base config
    .merge(KeyringProvider::new("myapp", "api_key"))  // Keyring overrides
    .merge(Env::prefixed("MYAPP_"))             // Env has highest precedence
    .extract()?;
```

---

## Implementation Priority Checklist

### Must-Have (Blockers for v1)
- [ ] Explicit error type handling (not found vs unavailable)
- [ ] `.optional()` builder method
- [ ] Document username→config key mapping
- [ ] Add `.map_to()`/`.as_key()` for explicit key naming
- [ ] Testing strategy section with mock backend
- [ ] Clarify Profile support in `data()` method

### Should-Have (Aim for v1, can defer if needed)
- [ ] Batch/multi-secret convenience API
- [ ] Document thread safety guarantees
- [ ] Headless/server environment documentation
- [ ] Secret rotation guidance
- [ ] Keyring entry creation workflow examples
- [ ] Better error handling examples in usage section

### Nice-to-Have (v1.1 or v2)
- [ ] Namespace support
- [ ] Performance benchmarks
- [ ] Reload mechanism for long-running processes
- [ ] Alternative approaches documentation
- [ ] Optional audit logging
- [ ] CLI tool for entry management

---

## Open Questions

1. **What happens when Figment2's `extract()` is called multiple times?** Does the keyring provider fetch values each time or cache them?

2. **How should applications handle keyring locked but available?** Should provider block, fail immediately, or provide async option?

3. **Should there be `KeyringProvider::default()`?** Use standard convention like binary name for service/username?

4. **Same config key from multiple providers?** Error or allowed with precedence?

5. **What's the testing strategy for CI without keyring?** Skip keyring tests or always use mocks?

6. **How does batch API work when some entries exist and others don't?** Partial success vs fail-fast?

7. **Should entry discovery require explicit opt-in?** Security implications of auto-discovery?

---

## Conclusion

The design document provides a solid foundation for the keyring provider. The proposed API is clean, well-integrated with Figment2, and addresses a genuine need. Three independent reviews identified consistent themes around error handling, testing, and API ergonomics, plus complementary concerns covering security, operations, and performance.

**Primary concerns to address before implementation:**
- Clear error handling semantics (distinguish missing vs unavailable)
- Optional/fallback mechanism for non-critical secrets
- Testing strategy with mock backend
- Explicit username→config key mapping with `.map_to()` option
- Clarify Profile support

**Design status:** Ready for implementation with above recommendations incorporated.

---

## Cross-Review Acknowledgments

This review synthesizes findings from three independent analyses:
- **Opus review**: Identified implicit username→key mapping, headless concerns, multi-secret API
- **Kimi review**: Covered batch API, secret rotation, entry permissions, alternative approaches
- **This review**: Addressed threading, performance, namespace priority, platform behavior

All three reviewers converged on error handling, testing strategy, and profile handling as critical concerns, strengthening confidence in these recommendations.

---

## Reviewer Notes

- Assumes familiarity with Rust, Figment2, and the keyring crate
- Review conducted against Rust library design best practices
- No security audit performed; dedicated security review recommended before v1
- Cross-review synthesis increases confidence in findings through triangulation

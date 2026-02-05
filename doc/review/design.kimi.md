# Design Review: Figment Keyring Provider

**Document Reviewed:** `doc/design.md`  
**Date:** 2026-02-05  
**Reviewer:** Kimi  
**Cross-Review:** Synthesized with reviews by Opus and GLM

---

## Executive Summary

The design proposes a sound approach to bridging system keyrings with Figment2's configuration system. The architecture is well-considered and leverages Figment2's provider model effectively. However, several areas require attention before implementation: configuration ergonomics, error handling granularity, and the "single value per provider" limitation need refinement.

Three independent reviews (Opus, GLM, and this review) identified consistent critical concerns around error handling, testing strategy, and API ergonomics, strengthening confidence in these recommendations.

**Recommendation:** Proceed with implementation after addressing the concerns in this review.

---

## Cross-Review Synthesis

This review incorporates insights from three independent analyses:

**All three reviewers identified:**
- Error handling semantics need clarification (missing entries vs unavailable service)
- Testing strategy is missing and critical
- Profile support in `data()` return type is unexplained
- Optional/fallback semantics not addressed
- Username→config key mapping needs explicit documentation

**Opus review identified:**
- Username→config key mapping is implicit magic
- Headless/service environment concerns (CI/CD, Docker, systemd)
- Multi-secret convenience constructor suggestion
- **Bug:** Backwards precedence comment in usage example (line 93)

**GLM review identified:**
- Thread safety not addressed
- Namespace support should be elevated to v1 (not future enhancement)
- Platform-specific behavior documentation needed
- Keyring crate version constraints

**This review identified:**
- Batch retrieval API for reducing verbosity
- Secret rotation and audit logging considerations
- Keyring entry creation/permissions discussion
- Alternative approaches not considered

---

## Detailed Review

### 1. Architecture Assessment

**Strengths:**
- **Clean integration:** The provider model fits naturally into Figment2's architecture
- **Layered approach:** Using keyring as a fallback rather than primary source demonstrates good security hygiene
- **Cross-platform:** Leveraging the `keyring` crate is the right choice for platform support

**Critical Bug Found:**

#### 1.1 Usage Example Precedence Error (Line 93)

The usage example comment says "Keyring fallback" but the keyring provider is merged last, giving it **highest** precedence, not lowest:

```rust
// CURRENT (INCORRECT):
Figment::new()
    .merge(File::from("config.toml"))           // Base config
    .merge(Env::prefixed("MYAPP_"))             // Env overrides
    .merge(KeyringProvider::new("myapp", "api_key"))  // Keyring fallback ← WRONG

// CORRECTED:
Figment::new()
    .merge(File::from("config.toml"))           // Base config
    .merge(KeyringProvider::new("myapp", "api_key"))  // Keyring overrides
    .merge(Env::prefixed("MYAPP_"))             // Env has highest precedence
```

Or swap the order to match the comment's intent.

#### 1.2 Single Value Per Provider Limitation

The current design requires one `KeyringProvider` instance per secret:

```rust
.merge(KeyringProvider::new("myapp", "api_key"))
.merge(KeyringProvider::new("myapp", "database_url"))
```

This is verbose and requires the application to know the complete set of secrets at compile time. Consider a batch retrieval API:

```rust
KeyringProvider::for_service("myapp")
    .with_entries(&["api_key", "database_url", "secret_token"])
```

This would retrieve all entries in one provider call, reducing keyring access overhead and improving ergonomics.

#### 1.3 Configuration Key Naming

The design maps `username` directly to the configuration key name. This conflates two different concerns:
- **Keyring entry name:** The identifier for the credential in the keyring
- **Configuration key name:** The name expected by the application's config struct

These may differ. Consider supporting an explicit mapping:

```rust
KeyringProvider::new("myapp", "prod_api_key")
    .map_to("api_key")  // Configuration will see "api_key", not "prod_api_key"
```

### 2. Security Analysis

**Strengths:**
- Acknowledges that plaintext values exist in memory after retrieval
- Warns against logging secrets
- Uses OS-level keyring integration

**Missing Considerations:**

#### 2.1 Keyring Entry Permissions

The design doesn't address how entries are created or what permissions they should have. Document:
- Who creates the keyring entries?
- What access control should entries have?
- Should the library provide a CLI tool for entry management?

Example platform commands should be documented:
```bash
# macOS
security add-generic-password -s myapp -a api_key -w "secret"

# Linux (secret-tool)
secret-tool store --label='myapp api_key' service myapp username api_key
```

#### 2.2 Secret Rotation Support

How does an application handle keyring entry updates at runtime? The design states "Keyring values are retrieved once at configuration loading" - this is correct for startup, but consider:
- Should there be a reload mechanism for long-running processes?
- What happens if a secret is rotated while the app is running?

#### 2.3 Audit Logging

System keyrings often provide audit trails. Should the provider:
- Log access events at DEBUG level?
- Distinguish between cache hits and actual keyring access?

### 3. Error Handling

**Current Design:** Returns Figment2 errors for various failure modes.

**Issues:**

#### 3.1 Distinguishable Error Types

Not all keyring failures should be treated equally:

| Error Type | Behavior |
|------------|----------|
| Entry not found | Could be acceptable (fallback to other provider) |
| Permission denied | Likely fatal - indicates security issue |
| Keyring service unavailable | Could be warning or fatal depending on requirements |
| Backend error | Likely fatal |

The design should specify how these map to Figment2 error types and whether consumers can distinguish between them.

#### 3.2 Missing Entry Handling

The example shows:

```rust
.merge(KeyringProvider::new("myapp", "api_key"))
```

What happens if `api_key` doesn't exist in the keyring? This will cause a runtime error. The design should specify:
- Whether missing entries are errors or silent skips
- How to mark a secret as optional vs required
- Whether there's a `.optional()` method or similar

### 4. Configuration Model Critique

#### 4.1 Service/Username Naming

The design uses `service` and `username` terminology from the `keyring` crate. However:
- "Username" is confusing when storing an API key (it's not a username)
- Alternative keyring libraries use different terminology (e.g., "account", "item")

Consider accepting this terminology but documenting it clearly, or providing type aliases for clarity:

```rust
pub type Service = String;
pub type CredentialName = String;  // Instead of "username"
```

#### 4.2 Profile Support

The provider signature shows it returns `BTreeMap<Profile, BTreeMap<String, Value>>`. The design doesn't specify how profiles work with keyring entries. For example:
- Should `KeyringProvider::new("myapp", "api_key")` work with Figment2 profiles?
- Could there be `KeyringProvider::for_profile("production", "myapp", "api_key")`?

This should be explicitly addressed or documented as not supported.

### 5. Thread Safety (Identified by GLM)

The design doesn't address thread safety. Figment2's `Figment::new().merge(...)` chain can be used across threads.

**Recommendation:**
- Explicitly document thread safety guarantees
- If `KeyringProvider` is not `Send`/`Sync`, document restriction
- Consider using `Arc` for shared provider instances
- Document behavior of `data()` call under concurrent access

### 6. Future Enhancements Analysis

#### 6.1 Namespace Support (Should be v1, not Future)

The design lists namespace support as a future enhancement. **GLM makes a strong case to elevate this to v1:** without namespace support, keyring keys could conflict with config file keys:

```rust
KeyringProvider::new("myapp", "api_key")
    .with_namespace("secrets")
// Produces: { "secrets.api_key": "..." }
```

This prevents accidental collisions between keyring secrets and file-based configuration.

#### 6.2 JSON Secrets (Mentioned)

The future enhancement suggests JSON parsing. This is valuable but introduces complexity:
- What if JSON is malformed?
- Should it support nested paths (e.g., `secrets.database.host`)?
- How to handle type conversion?

Consider whether this should be in scope or left to applications to handle.

#### 6.3 Entry Discovery (Mentioned)

Automatic discovery is listed as a future enhancement. This has significant security implications:
- Could accidentally expose entries the app shouldn't see
- Non-deterministic configuration loading
- Harder to audit what secrets an app uses

If implemented, it should require explicit opt-in and perhaps filtering patterns.

### 7. Implementation Details

#### 7.1 Dependencies

The design lists minimal dependencies. Verify:
- Whether `keyring` crate version constraints are compatible with Figment2 (Opus notes API differences between keyring 1.x and 2.x)
- If any additional error handling crates are needed (e.g., `thiserror`)

#### 7.2 Testing Strategy

Not mentioned in the design. Testing keyring code is challenging because:
- Requires system keyring access
- Platform-specific behavior
- Can't easily mock without traits

Recommend documenting a testing approach, possibly using:
- Mock keyring backend for unit tests
- Integration tests with real keyring (conditional compilation)
- CI configuration for testing on different platforms

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

### 8. Headless/Service Environment Concerns (Opus)

System keyrings often require user session context. The design should address:
- CI/CD pipelines
- Systemd services
- Docker containers
- SSH sessions without agent forwarding

**Recommendation:** Document failure modes and recommend alternatives (env vars, file providers) for these contexts.

### 9. Documentation Gaps

The following should be added to the design or implementation:

1. **Installation/Setup Guide:** How does a user populate the keyring with secrets?
2. **Troubleshooting Guide:** Common issues (keyring locked, service unavailable, etc.)
3. **Security Checklist:** Best practices for operators deploying apps using this
4. **Performance Notes:** Keyring access can be slow (especially on first access) - document expected latency

### 10. Alternative Approaches Considered?

The design doesn't discuss alternatives. For completeness, consider mentioning:
- **Environment variables with prefix:** Why not just `MYAPP_API_KEY_FILE` pointing to a secrets file?
- **Secret management services:** AWS Secrets Manager, HashiCorp Vault integration
- **File-based encryption:** age, sops, or similar tools

Briefly explaining why system keyring was chosen over these alternatives would strengthen the design rationale.

---

## Recommendations

### Must-Have Before Implementation

1. **Fix usage example precedence:** Correct the "fallback" comment or reorder the merge chain
2. **Clarify error handling:** Specify behavior for missing entries and distinguishable error types
3. **Add configuration key mapping:** Support renaming keyring entries to configuration keys via `.map_to()` or `.as_key()`
4. **Document testing approach:** How will this be tested across platforms? Include mock backend strategy
5. **Clarify Profile support:** Explicitly document whether profiles are supported

### Should-Have (Aim for v1)

6. **Namespace support:** Prevent key collisions between keyring and file configs
7. **Batch retrieval API:** Support retrieving multiple entries in one provider
8. **Optional secrets:** Allow marking secrets as optional with fallback behavior
9. **Thread safety documentation:** Document `Send`/`Sync` guarantees
10. **Security documentation:** Add operator setup and security checklist
11. **Headless environment guidance:** Document CI/CD and Docker limitations

### Nice-to-Have

12. **Profile-aware keyring access:** Support per-profile secrets
13. **Performance benchmarks:** Measure keyring access overhead
14. **CLI tool:** Helper for managing keyring entries during development

### Reconsider

15. **Entry discovery:** Automatic discovery has security risks; consider whether this should be implemented

---

## Questions for the Author

1. How should missing keyring entries be handled - as errors or silent skips with fallback to other providers?
2. What's the expected workflow for populating keyring entries during development and deployment?
3. Should secrets support Figment2 profiles (dev/staging/production), and if so, how?
4. Is there a plan for handling keyring service unavailability (e.g., headless servers without keyring)?
5. What's the testing strategy for CI/CD environments that may not have system keyrings available?
6. What keyring crate version should be targeted (1.x or 2.x)?

---

## Conclusion

The design is solid and addresses a real need in the Figment2 ecosystem. The architecture decisions are sound, particularly the integration with Figment2's provider model. The primary concerns are around ergonomics (single value limitation), error handling granularity, and operational concerns (entry setup, permissions, testing).

Three independent reviews converged on the same critical issues, strengthening confidence in these recommendations. The cross-review synthesis ensures comprehensive coverage of API design, security, operations, and edge cases.

**Status:** Approve with revisions

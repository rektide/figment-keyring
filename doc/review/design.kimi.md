# Design Review: Figment Keyring Provider

**Document Reviewed:** `doc/design.md`  
**Date:** 2026-02-05  
**Reviewer:** Kimi

---

## Executive Summary

The design proposes a sound approach to bridging system keyrings with Figment2's configuration system. The architecture is well-considered and leverages Figment2's provider model effectively. However, several areas require attention before implementation: configuration ergonomics, error handling granularity, and the "single value per provider" limitation need refinement.

**Recommendation:** Proceed with implementation after addressing the concerns in this review.

---

## Detailed Review

### 1. Architecture Assessment

**Strengths:**
- **Clean integration:** The provider model fits naturally into Figment2's architecture
- **Layered approach:** Using keyring as a fallback rather than primary source demonstrates good security hygiene
- **Cross-platform:** Leveraging the `keyring` crate is the right choice for platform support

**Concerns:**

#### 1.1 Single Value Per Provider Limitation

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

#### 1.2 Configuration Key Naming

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

#### 2.2 Secret Rotation Support

How does an application handle keyring entry updates at runtime? The design states "Keyring values are retrieved once at configuration loading" - this is correct for startup, but consider:
- Should there be a reload mechanism for long-running processes?
- What happens if a secret is rotated while the app is running?

#### 2.3 Audit Logging

System keyrings often provide audit trails. Should the provider:
- Log access events (at appropriate levels)?
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

### 5. Future Enhancements Analysis

#### 5.1 JSON Secrets (Mentioned)

The future enhancement suggests JSON parsing. This is valuable but introduces complexity:
- What if JSON is malformed?
- Should it support nested paths (e.g., `secrets.database.host`)?
- How to handle type conversion?

Consider whether this should be in scope or left to applications to handle.

#### 5.2 Entry Discovery (Mentioned)

Automatic discovery is listed as a future enhancement. This has significant security implications:
- Could accidentally expose entries the app shouldn't see
- Non-deterministic configuration loading
- Harder to audit what secrets an app uses

If implemented, it should require explicit opt-in and perhaps filtering patterns.

### 6. Implementation Details

#### 6.1 Dependencies

The design lists minimal dependencies. Verify:
- Whether `keyring` crate version constraints are compatible with Figment2
- If any additional error handling crates are needed (e.g., `thiserror`)

#### 6.2 Testing Strategy

Not mentioned in the design. Testing keyring code is challenging because:
- Requires system keyring access
- Platform-specific behavior
- Can't easily mock without traits

Recommend documenting a testing approach, possibly using:
- Mock keyring backend for unit tests
- Integration tests with real keyring (conditional compilation)
- CI configuration for testing on different platforms

### 7. Documentation Gaps

The following should be added to the design or implementation:

1. **Installation/Setup Guide:** How does a user populate the keyring with secrets?
2. **Troubleshooting Guide:** Common issues (keyring locked, service unavailable, etc.)
3. **Security Checklist:** Best practices for operators deploying apps using this
4. **Performance Notes:** Keyring access can be slow (especially on first access) - document expected latency

### 8. Alternative Approaches Considered?

The design doesn't discuss alternatives. For completeness, consider mentioning:
- **Environment variables with prefix:** Why not just `MYAPP_API_KEY_FILE` pointing to a secrets file?
- **Secret management services:** AWS Secrets Manager, HashiCorp Vault integration
- **File-based encryption:** age, sops, or similar tools

Briefly explaining why system keyring was chosen over these alternatives would strengthen the design rationale.

---

## Recommendations

### Must-Have Before Implementation

1. **Clarify error handling:** Specify behavior for missing entries and distinguishable error types
2. **Add configuration key mapping:** Support renaming keyring entries to configuration keys
3. **Document testing approach:** How will this be tested across platforms?

### Should-Have

4. **Batch retrieval API:** Support retrieving multiple entries in one provider
5. **Optional secrets:** Allow marking secrets as optional with fallback behavior
6. **Security documentation:** Add operator setup and security checklist

### Nice-to-Have

7. **Profile support:** Clarify or implement profile-aware keyring access
8. **Performance benchmarks:** Measure keyring access overhead
9. **CLI tool:** Helper for managing keyring entries during development

### Reconsider

10. **Entry discovery:** Automatic discovery has security risks; consider whether this should be implemented

---

## Questions for the Author

1. How should missing keyring entries be handled - as errors or silent skips with fallback to other providers?
2. What's the expected workflow for populating keyring entries during development and deployment?
3. Should secrets support Figment2 profiles (dev/staging/production), and if so, how?
4. Is there a plan for handling keyring service unavailability (e.g., headless servers without keyring)?
5. What's the testing strategy for CI/CD environments that may not have system keyrings available?

---

## Conclusion

The design is solid and addresses a real need in the Figment2 ecosystem. The architecture decisions are sound, particularly the integration with Figment2's provider model. The primary concerns are around ergonomics (single value limitation), error handling granularity, and operational concerns (entry setup, permissions, testing).

With the recommendations above addressed, this design should result in a clean, secure, and maintainable implementation.

**Status:** Approve with revisions

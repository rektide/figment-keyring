# Design Review: Figment Keyring Provider

**Reviewer**: Claude (opus)  
**Document**: [doc/design.md](../design.md)

---

## Summary

Solid design for a focused utility. The single-secret-per-provider model is clean and composable, though it requires verbosity for multi-secret use cases.

**Verdict:** Approve with revisions

---

## Comparison with Other Reviews

After reading [design.glm.md](./design.glm.md) and [design.kimi.md](./design.kimi.md), here's where I stand:

### Points of Agreement (Consensus Issues)

All three reviews independently identified these problems—they should be prioritized:

1. **Optional/fallback semantics undefined** - What happens when keyring entry is missing? This is the #1 issue across all reviews.

2. **Profile handling unexplained** - The trait returns `BTreeMap<Profile, ...>` but no explanation of profile behavior.

3. **Username→key mapping is implicit magic** - GLM suggests renaming, Kimi suggests `.map_to()`, I suggested `.as_key()`. All agree this needs addressing.

4. **Testing strategy missing** - GLM and Kimi both call this out explicitly. Critical for a library that requires platform-specific system services.

5. **Headless/service environments** - Docker, CI/CD, systemd services. All reviews note this gap.

### Where I Differ

**On batch retrieval API (Kimi's suggestion):**
```rust
KeyringProvider::for_service("myapp")
    .with_entries(&["api_key", "database_url"])
```

I'm lukewarm on this. It conflates "provider per secret" with "provider per service" and introduces partial-failure semantics (what if only some entries exist?). The verbosity of multiple `.merge()` calls is Figment2's idiom—fighting it may create more confusion than it solves.

**On entry discovery (GLM question, Kimi caution):**

Kimi correctly notes security risks. I'd go further: **don't implement this**. Non-deterministic config loading is an anti-pattern. If you don't know what secrets your app needs, you have bigger problems.

**On caching/performance (GLM's concern):**

GLM worries about repeated IPC calls. In practice, Figment2's `extract()` is typically called once at startup. Caching is premature optimization. If someone calls `extract()` in a hot loop, that's a usage bug.

**On secret rotation (Kimi's point):**

Kimi raises runtime secret rotation. I disagree this belongs in the design—Figment2 is a startup-time configuration library, not a runtime secrets manager. Applications needing rotation should use Vault, AWS Secrets Manager, etc.

### Points I Missed That Others Caught

- **Thread safety** (GLM): Good catch. `KeyringProvider` should document `Send + Sync` guarantees.
- **Error type granularity** (GLM, Kimi): Distinguishing "entry not found" vs "keyring unavailable" vs "permission denied" is important for user experience.
- **Who creates entries?** (Kimi): Operational documentation gap. Should document `secret-tool` (Linux), `security` CLI (macOS), etc.
- **Alternatives considered** (Kimi): Design should briefly justify keyring over env-file-based secrets, Vault, etc.

---

## Original Analysis

### Strengths

1. **Minimal API surface**: `KeyringProvider::new(service, username)` mirrors keyring semantics.
2. **Honest limitations section**: Acknowledging constraints upfront is valuable.
3. **Clear precedence example**: File → Env → Keyring layering is immediately understandable.

### Concerns

#### Username-as-key mapping is implicit

```rust
KeyringProvider::new("myapp", "api_key")
// produces: { "api_key": "value" }
```

The `username` becoming the config key is undocumented. Consider:
- Explicit `.as_key("api_key")` or `.map_to("api_key")` builder method
- Document this behavior prominently
- What if `username` contains dots or invalid config key characters?

#### Missing: Optional/fallback semantics

The design doesn't address what happens when a keyring entry is missing:

1. **Fail hard** (implied) - breaks "fallback" claim in usage example
2. **Return empty** - allows other layers to provide values
3. **Explicit optional mode** - `.optional()` builder method

This is a critical API decision.

#### Headless/service environment story

System keyrings require user session context. Document failure modes for:
- CI/CD pipelines
- Systemd services
- Docker containers
- SSH sessions without agent forwarding

#### Profile support unclear

`BTreeMap<Profile, ...>` in the trait but no explanation:
- Which profile does the keyring value appear under?
- Can users target specific profiles?

### API Suggestions

#### Builder pattern for flexibility

```rust
KeyringProvider::new("myapp", "api_key")
    .as_key("secrets.api_key")  // custom config key
    .optional()                  // don't fail if missing
    .with_profile(Profile::Dev)  // target specific profile
```

#### Multi-secret convenience (lower priority)

```rust
KeyringProvider::multi("myapp", &["api_key", "db_password", "jwt_secret"])
```

Reduces boilerplate without JSON complexity, but I'm less convinced this is needed after reading other reviews.

### Minor Issues

1. **Usage example comment is backwards**: "Keyring fallback" but keyring merged last = highest precedence.
2. **`keyring` crate version**: Specify minimum version (1.x vs 2.x API differences).
3. **Error types**: Map keyring errors to distinguishable Figment2 errors.

---

## Implementation Priorities

Based on consensus across all three reviews:

### Must-Have (P0)
- [ ] Define missing-entry behavior (optional vs required)
- [ ] Document/implement profile handling
- [ ] Add `.optional()` method
- [ ] Testing strategy section

### Should-Have (P1)
- [ ] Key remapping (`.as_key()` or `.map_to()`)
- [ ] Distinguishable error types
- [ ] Document thread safety guarantees
- [ ] Headless environment documentation

### Nice-to-Have (P2)
- [ ] Batch retrieval API
- [ ] Operator setup guide (how to populate keyring)
- [ ] Alternatives-considered section

### Don't Implement
- Entry discovery (security risk, non-deterministic)
- Runtime secret rotation (out of scope for Figment2)
- Caching (premature optimization)

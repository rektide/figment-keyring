# Design Review: Figment Keyring Provider

**Reviewer**: Claude (opus)  
**Document**: [doc/design.md](../design.md)

## Summary

Solid design for a focused utility. The single-secret-per-provider model is clean and composable, though it requires verbosity for multi-secret use cases.

## Strengths

1. **Minimal API surface**: `KeyringProvider::new(service, username)` is intuitive and mirrors how keyrings actually work.

2. **Honest limitations section**: Acknowledging "single value per provider" and "runtime errors" upfront is valuable.

3. **Clear precedence example**: The File → Env → Keyring layering example immediately communicates typical usage.

## Concerns

### Username-as-key mapping is implicit

```rust
KeyringProvider::new("myapp", "api_key")
// produces: { "api_key": "value" }
```

The `username` becoming the config key is undocumented magic. Consider:
- Explicit `.as_key("api_key")` builder method
- Document this behavior prominently
- What if `username` contains dots or invalid config key characters?

### Missing: Optional/fallback semantics

The design doesn't address what happens when a keyring entry is missing. Options:

1. **Fail hard** (current implied behavior) - breaks the "fallback" claim in the usage example
2. **Return empty** - allows other layers to provide values
3. **Explicit optional mode** - `.optional()` builder method

This is a critical API decision that affects composability.

### Headless/service environment story

System keyrings often require user session context. The design should address:
- CI/CD pipelines
- Systemd services
- Docker containers
- SSH sessions without agent forwarding

Recommendation: Document failure modes and recommend alternatives (env vars, file providers) for these contexts.

### Profile support unclear

Figment2 supports profiles (dev, prod, etc.). The design shows `BTreeMap<Profile, ...>` in the trait but doesn't explain:
- Which profile does the keyring value appear under?
- Can users target specific profiles?
- Consider: `.with_profile(Profile::Dev)`

## API Suggestions

### Builder pattern for flexibility

```rust
KeyringProvider::new("myapp", "api_key")
    .as_key("secrets.api_key")  // custom config key
    .optional()                  // don't fail if missing
    .with_profile(Profile::Dev)  // target specific profile
```

### Multi-secret convenience constructor

```rust
KeyringProvider::multi("myapp", &["api_key", "db_password", "jwt_secret"])
// produces: { "api_key": "...", "db_password": "...", "jwt_secret": "..." }
```

This reduces boilerplate without the complexity of JSON parsing.

## Minor Issues

1. **Usage example comment is backwards**: "Keyring fallback" comment says fallback, but keyring is merged last (highest precedence). The comment should say "Keyring override" or reorder the merges.

2. **`keyring` crate version**: Consider specifying minimum version given API differences between keyring 1.x and 2.x.

3. **Error type**: Does `figment2::Error` have a variant suitable for "secret not found"? May need custom error mapping.

## Future Enhancements Feedback

- **JSON/Structured Secrets**: Good idea but adds parsing errors. Consider `KeyringProvider::json(...)` variant.
- **Entry Discovery**: Platform-dependent; Secret Service supports enumeration, macOS Keychain has limitations. Worth pursuing but document platform gaps.
- **Caching**: Unnecessary complexity for typical config-load-once patterns. Defer unless benchmarks show need.

## Verdict

**Approve with revisions**: Core design is sound. Address the optional/fallback semantics and profile handling before implementation. The username→key implicit mapping needs documentation at minimum.

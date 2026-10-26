## What changed

The Rust project shipped its 2026 edition on Thursday, bringing the largest set of language changes since 2021, according to the release team, which spent two years on it.

Async closures, generators, and a reworked borrow checker are the headline features, and the team says migration is automatic for most crates, with a few manual steps left.

Library authors can opt in per crate, so mixed-edition dependency graphs keep working, a guarantee the project has kept since the first edition shipped in 2018.

```toml
edition = "2026"
```

The full release notes, including a migration guide, are available on the project blog.

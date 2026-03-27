Run the full ship flow: verify quality, ensure test coverage, update
artifacts, smoke test, then push, create PR, and merge when CI is green.

## Phases

### 1. Pre-flight
- Verify on a feature branch (not main)
- Check for uncommitted changes
- Fetch latest main and rebase if needed

### 2. Test Coverage
- Identify changed code paths
- Ensure tests exist for:
  - Positive cases (happy path)
  - Negative cases (errors, edge cases)
  - Security scenarios (malicious input)
- Add missing tests

### 3. Artifact Updates
- Update specs if behavior changed
- Update CHANGELOG.md for user-facing changes
- Update documentation for API changes

### 4. Simplification
- Remove dead code introduced by changes
- Reduce duplication
- Simplify where possible without changing behavior

### 5. Security Review
- Check for injection risks in new parsing code
- Verify resource limits are enforced
- Review input validation

### 6. Smoke Test
- Run `just test` for full test suite
- Run `just lint` for code quality
- Build CLI and verify basic extraction works

### 7. Quality Gates
- `cargo fmt --all -- --check`
- `cargo clippy --workspace -- -D warnings`
- `cargo deny check`

### 8. Push, PR, Merge
- Push branch to remote
- Create or update PR with summary
- Wait for CI to pass
- Merge when green

**IMPORTANT**: Phases 2-5 (tests, artifacts, simplification, security review,
smoke testing) are the quality core — do NOT skip them.

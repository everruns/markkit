Run the full ship flow: verify quality, ensure test coverage, update
artifacts, smoke test, then push, create PR, address review comments,
and merge only when CI is green, all threads resolved, and user confirms.

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

### 8. Push & PR
- Push branch to remote
- Create or update PR with summary
- Wait for CI to pass

### 9. Review & Merge
- Check for unresolved review comments — address or reply to each
- Ensure all review threads are resolved
- Confirm no changes-requested reviews remain outstanding
- Ask the user whether to merge now or wait for reviewer feedback
- Only merge when: CI green, no unresolved threads, no pending change requests

**IMPORTANT**: Phases 2-5 (tests, artifacts, simplification, security review,
smoke testing) are the quality core — do NOT skip them.

**IMPORTANT**: Never auto-merge without confirming with the user. PRs should
have time for human review before merging.

Run pre-release maintenance checks on the markkit codebase.

Each section below is an outcome to achieve, not a script to follow.
Use whatever tools and approaches make sense to verify and fix.

Fix issues as you find them — don't just report. Use conventional
commit messages for any fixes.

Sections (run all unless $ARGUMENTS specifies a subset):

## 1. Dependencies
- Run `cargo update` and verify tests pass
- Check for outdated dependencies
- Review Dependabot PRs if any

## 2. Security
- Run `cargo audit` for vulnerabilities
- Run `cargo deny check` for license compliance
- Review any new advisories

## 3. Tests
- Run full test suite: `just test`
- Check coverage isn't regressing
- Identify any `#[ignore]` tests that should be re-enabled

## 4. Documentation
- Verify README is current
- Check rustdoc builds without warnings
- Ensure specs match implementation

## 5. Examples
- Verify examples compile and run
- Update examples for any API changes

## 6. Specs
- Review specs/ for accuracy
- Update implementation status

## 7. Simplification
- Look for dead code, unused dependencies
- Identify duplication opportunities
- Reduce complexity where possible

## 8. CI Health
- Check recent GitHub Actions runs
- Verify nightly builds are passing
- Review any flaky tests

Deliver a summary of findings and actions taken.

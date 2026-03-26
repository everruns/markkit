# Maintenance

## Regular Maintenance Tasks

### Dependencies
- Weekly: Review Dependabot PRs
- Monthly: Run `cargo update` and test
- Quarterly: Audit transitive dependencies

### Security
- `cargo audit` for known vulnerabilities
- `cargo deny check` for license compliance
- Review security advisories

### Testing
- Monitor test coverage trends
- Update test fixtures for new edge cases
- Run benchmarks to detect regressions

### Documentation
- Keep specs current with implementation
- Update examples for API changes
- Review and update README

## Pre-Release Maintenance

Run the `/maintain` command to verify:
- Dependency currency
- Security posture
- Test coverage
- Documentation accuracy
- Working examples
- Spec alignment
- Code cleanliness
- CI health

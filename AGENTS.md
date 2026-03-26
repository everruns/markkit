# Markkit – Coding Agent Guidance

Minimal tokens. Telegraph style. Drop filler.

## Quick Reference

- Build: `just build`
- Test: `just test`
- Lint: `just lint`
- Pre-PR: `just pre-pr`
- Specs: `specs/` directory

## Rules

- Fetch `origin/main` and rebase before editing
- Small PRs, incremental changes
- Conventional Commits: `type(scope): description`
- Real human git identity only
- No Claude session links in commits/PRs
- Write failing test first, then fix
- Security tests for parsing changes
- Run `just pre-pr` before submitting

## Crate Map

| Crate | Purpose |
|-------|---------|
| `markkit` | Core extraction library |
| `markkit-cli` | CLI tool |
| `markkit-bench` | Benchmarks |
| `markkit-eval` | LLM evaluation |
| `markkit-python` | Python bindings |
| `markkit-js` | Node.js bindings |

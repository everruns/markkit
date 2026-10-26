---
name: ship
description: Run the full Markkit ship flow — verify quality, ensure test coverage, update artifacts, smoke test, push, create PR, and merge when CI is green. Trigger when user says ship, ship it, fix and ship, asks to push and merge a branch, or runs maintenance.
user_invocable: true
metadata:
  internal: true
---

Run the full ship flow: verify quality, ensure test coverage, update artifacts,
smoke test, then push, create PR, and merge when CI is green.

Implements the Pre-PR Checklist in `AGENTS.md`. Execute ALL phases, not just push/merge.

## Arguments

- `$ARGUMENTS` - Optional: what is being shipped (PR title/body context, scopes checks)

## Phase 1: Pre-flight

1. Confirm not on `main`
2. Detached `HEAD` with task changes: create a branch first
3. Uncommitted changes belong to the task: keep going. Unrelated: stop and tell the user
4. `git config user.name`/`user.email` are human (see `AGENTS.md` Commit Attribution)

## Phase 2: Test Coverage

Review `git diff origin/main...HEAD` and `git log origin/main..HEAD`:

1. Identify every changed code path
2. `just test`; fix failures
3. Add missing tests:
   - **Positive**: happy path, valid input
   - **Negative**: invalid input, errors, boundaries, limits
   - **Security**: parsing, limits, or URL fetching changes add tests per
     `knowledge/security/threat-model.md` and update its Test Coverage map
   - **Golden fixtures**: HTML conversion changes add `tests/fixtures/html/<name>.{html,md}`
4. `just test` green

## Phase 3: Artifact Updates

Skip what isn't affected.

1. **Knowledge** (`knowledge/`): update affected concepts in the same change; add a `knowledge/log.md` entry; `just check-okf`
2. **Threat model**: new input surface or limit → new/updated entry, keep `T*`/`S*` ids stable
3. **AGENTS.md**: new commands, knowledge concepts, workflow changes
4. **README.md / docs/**: user-facing behavior, flags, API
5. **Binding READMEs**: exported function changes in `markkit-python` / `markkit-js`
6. **CHANGELOG.md**: Unreleased entry for user-facing changes

## Phase 3b: Simplification

Duplication, deep nesting, dead code, unclear names, needless abstraction. If changed, back to Phase 2.

## Phase 3c: Security Review

1. Untrusted input validated and bounded before use (size, depth, counts)
2. No unbounded allocation driven by attacker-controlled headers or sizes
3. Errors don't leak raw input or internal paths
4. No code execution paths (scripts, macros, formulas, entities)
5. `unsafe` justified and sound

Fix, add regression tests, update the threat model.

## Phase 3d: Design Quality

Real domain types over stringly-typed interfaces; specific error variants; no
compatibility shims (fix call sites); no premature generics; visibility protects invariants.

## Phase 4: Smoke Testing

1. CLI: `just run extract --file <file>` on realistic input (indented HTML, office docs); read the output
2. Examples: `just examples`
3. Bindings changed: build them (`maturin develop` / `napi build`) and call the changed function

## Phase 5: Quality Gates

```bash
git fetch origin main && git rebase origin/main
just pre-pr
```

Fix root causes; never bypass hooks or weaken tests/audit policy.

## Phase 6: Push and PR

```bash
git push -u origin <branch>
```

Create or update the PR:

- **Title**: conventional commit style
- **Body**: `.github/pull_request_template.md`; functional change and impact, not a file walkthrough
- **Evidence**: Before / After (CLI output, test results); say so when nothing observable changed

## Phase 7: CI and Merge

- Wait for CI with bounded checks; elapsed time alone is not a blocker
- Failure: read logs, fix root cause, push, wait again
- All required checks green and review threads resolved: squash-merge, confirm it merged
- **NEVER** merge red CI

## Phase 8: Post-merge

Report the merged PR URL.

## Rules

- Phases 2-4 are the quality core; never skip them
- "Fix and ship": implement first, then ship
- Partial issue coverage: `Part of #N`, not `Closes #N`

Process open GitHub issues and resolve them via merged pull requests.

## Qualifying Issues

Only process issues created by authorized users or containing approval
comments from those users.

## Core Principle

One issue = one PR. No bundling. No skipping.

## Process Per Issue

### 1. Understand & Scope
- Read the issue thoroughly
- Classify: bug, feature, docs, refactor
- Identify affected crates and files
- Create branch: `fix/issue-{N}-{short-slug}`

### 2. Test First
- For bugs: create a failing test that reproduces the issue
- For features: scaffold validation tests
- Place tests alongside existing test modules

### 3. Implement
- Make minimal, focused changes
- Cover positive and negative test cases
- Consider security implications for parsing changes

### 4. Ship
- Use the `/ship` skill for:
  - Spec updates
  - Quality gates
  - PR creation
  - CI verification
  - Merge

### 5. Return to Main
```bash
git checkout main && git pull origin main
```
Then proceed to next issue.

## Post-Resolution

After all issues are resolved, identify any `#[ignore]` tests
that now pass and un-ignore them in a single PR.

## Arguments

Optional: specific issue numbers or labels.
Default: process all qualifying open issues.

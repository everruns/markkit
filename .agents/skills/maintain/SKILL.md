---
name: maintain
description: Analyze, fix, validate, and ship Markkit maintenance through green CI and merge. Trigger on run maintenance, maintain, maintainace, or maintaiance; use analysis-only mode only when explicitly requested.
user_invocable: true
metadata:
  internal: true
---

# Markkit maintenance

A request to run maintenance authorizes the full outcome: analyze the repository,
fix findings, validate, push, open a PR, resolve review/CI failures, and squash-merge
when every required check is green. A local commit or a list of follow-up issues
is not completion. Explicit analysis-only requests omit edits and shipping.

Read `knowledge/operations/maintenance.md` and execute its checklist, then use the
`ship` skill. Sync from latest `origin/main` before editing. Split independent
fixes into separate small PRs (one for structure/docs, one per behavior fix).

Keep findings in the active pass and fix root causes. A large diff, missing audit,
failed local check, or long build is work to complete, not a reason to defer.
Never weaken tests, resource limits, or `deny.toml` policy to ship an upgrade.
If the newest upstream version breaks a security contract, establish that, keep
the newest safe version with a tested and documented pin, and finish the pass.

Audit CI definitions, not only results: run the Workflow credential audit in
`knowledge/operations/maintenance.md` against `.github/workflows/` (top-level
`permissions`, `persist-credentials: false`, secrets via step `env:` only, no
`pull_request_target`/`workflow_run`/`issue_comment`, no `${{ github.event.* }}`
inside `run:`). A finding there is a fix in this pass.

Smoke test real-world shaped input, not only fixtures: run the CLI on indented,
multi-block HTML and on each office format, and read the output.

Only stop for a missing permission, unavailable service, or user decision that
cannot be resolved within the task. Report evidence and continue independent work.
Never merge red CI or claim success while a required check is unfinished.

Finish by appending a factual entry to the Pass record in
`knowledge/operations/maintenance.md` and a `knowledge/log.md` entry. The final
response names the merged PRs, key fixes, verification, and accepted constraints.

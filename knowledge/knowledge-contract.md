---
type: Playbook
title: Knowledge Maintenance Contract
description: Rules for maintaining the Markkit knowledge bundle and its OKF conformance.
tags:
  - markkit
  - knowledge
  - okf
  - process
---

# Knowledge Maintenance Contract

`knowledge/` is Markkit's canonical [Open Knowledge Format (OKF) v0.2](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md)
bundle and persistent project memory. It replaces the former `specs/` directory.

## Maintenance rules

- Treat this knowledge as part of the implementation, not historical documentation.
- Before changing behavior, read the relevant concepts and follow their decisions, or update them in the same change.
- When code changes a documented behavior, design decision, invariant, limitation, threat, test strategy, or process, update the affected concept in the same pull request.
- Record decisions not recoverable from code. Link to source and tests rather than duplicating volatile details.
- Keep stable identifiers (`T1`..`T6`, `S1`..`S11`); never renumber them.
- User-facing guides stay in `docs/`; API reference stays in rustdoc.
- Say what is true now: a stub is a stub, a planned mitigation is planned.

## OKF conformance rules

The bundle declares `okf_version: "0.2"` in the bundle-root [`index.md`](index.md).

- Every `.md` file except the reserved `index.md` and `log.md` is a **concept** and starts with YAML frontmatter containing a non-empty `type`, plus `title` and a single-line `description`.
- `index.md` files carry **no** frontmatter, except the bundle root, which may carry only `okf_version`.
- `index.md` bodies are link lists under headings: `* [Title](path) - description`, reusing each concept's `description` verbatim.
- Every `index.md` lists its own directory: concepts beside it and subdirectories under it, nothing deeper.
- `log.md` entries are grouped under `## YYYY-MM-DD` headings, newest first.
- Links between concepts are relative and must resolve; links inside code spans and fenced blocks are text.
- Every concept links to at least one other concept, conventionally in a closing `## See also`.
- Bundle documents are referenced as relative markdown links, never as `knowledge/...` repository paths inside the bundle.

## Layout

| Directory | Holds |
|---|---|
| [`foundations/`](foundations/) | Pipeline architecture, IR |
| [`formats/`](formats/) | HTML extraction, format detection, office mappings |
| [`security/`](security/) | Security model, threat model |
| [`runtimes/`](runtimes/) | Python and Node.js packages |
| [`operations/`](operations/) | Testing, docs, examples, maintenance, release, eval |

Only this contract and `log.md` sit at the bundle root. Directory conveys domain;
`type` conveys kind. Types in use: `Architecture`, `Subsystem Design`,
`Interface Contract`, `Package Design`, `Threat Model`, `Test Strategy`, `Playbook`.

## Enforcement

`scripts/check_okf.py knowledge` validates these rules (ported from bashkit, no
third-party dependencies). `just check-okf` runs it plus the pinned upstream
`okf-lint` when installed; CI runs both in the lint job. Regression tests live in
`scripts/tests/test_check_okf.py` (`just test-scripts`).

```console
$ python3 scripts/check_okf.py knowledge
knowledge: OKF v0.2 conformant (15 concepts, 6 index files, 1 log file)
```

## See also

- [Maintenance](operations/maintenance.md), the periodic pass that re-checks the bundle
- [Documentation Architecture](operations/documentation.md), the doc trees this bundle is not

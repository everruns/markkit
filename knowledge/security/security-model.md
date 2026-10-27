---
type: Threat Model
title: Security Model
description: Threat categories and security invariants summary for untrusted document input.
tags:
  - markkit
  - security
---

# Security Model

## Threat Model

Markkit processes untrusted input from the internet. Key threats:

### TM-INJ: Injection via extracted content
- **Mitigation**: All output is markdown text; no code execution
- **Mitigation**: Strip script/style/event handler content

### TM-DOS: Denial of service
- **Mitigation**: Input size limits
- **Mitigation**: Processing time limits
- **Mitigation**: Output size limits
- **Mitigation**: Nesting depth limits
- **Mitigation**: Table span layout bounded by a grid budget (spans dropped past it); see [Threat Model](threat-model.md) T2e

### TM-SSRF: Server-side request forgery (URL fetching)
- **Mitigation**: Non-public addresses refused by default, after DNS resolution and on every redirect (opt in with `allow_private_networks`)
- **Mitigation**: http/https only, optional host allowlist, redirect limit, body bounded by `max_input_size`, fetch bounded by `max_processing_secs`
- **Residual**: an environment proxy resolves names itself; see [Threat Model](threat-model.md) T3

### TM-DOC: Malicious office documents
- **Mitigation**: Bounded ZIP decompression (entry count, per-entry and total size)
- **Mitigation**: Spreadsheet grid pre-scan / streaming (no dense-grid amplification)
- **Mitigation**: No macro execution, no formula evaluation, hidden sheets skipped by default
- **Compatibility**: ZIP entry-name decoding errors propagate; Unicode and legacy CP437 part names preserve bounded case-insensitive lookup
- See [Threat Model](threat-model.md) T6

### TM-MEM: Memory exhaustion
- **Mitigation**: Streaming processing where possible
- **Mitigation**: Memory budgets for large documents

## Security Invariants

1. Extracted markdown never contains executable content
2. Resource limits cannot be bypassed
3. URL fetching respects network boundaries
4. Malformed input produces errors, never panics

## See also

- [Threat Model](threat-model.md), the detailed per-threat breakdown and test map
- [Testing Strategy](../operations/testing.md), security and property test categories

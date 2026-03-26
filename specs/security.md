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

### TM-SSRF: Server-side request forgery (URL fetching)
- **Mitigation**: URL allowlist support
- **Mitigation**: Private IP range blocking
- **Mitigation**: Redirect limit

### TM-MEM: Memory exhaustion
- **Mitigation**: Streaming processing where possible
- **Mitigation**: Memory budgets for large documents

## Security Invariants

1. Extracted markdown never contains executable content
2. Resource limits cannot be bypassed
3. URL fetching respects network boundaries
4. Malformed input produces errors, never panics

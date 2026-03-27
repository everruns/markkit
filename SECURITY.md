# Security Policy

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

Instead, please report them via email to **security@everruns.com**.

Please include:

- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

## Response Timeline

- **Acknowledgment**: Within 48 hours
- **Initial assessment**: Within 1 week
- **Critical fixes**: Target 30 days

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | Yes       |

## Security Architecture

Markkit processes untrusted input (HTML, PDF, DOCX, etc.) and converts it to markdown. Key security considerations:

- **Input sanitization**: All parsed content is sanitized before output
- **Resource limits**: Configurable limits on input size and processing time
- **No code execution**: Extracted content is treated as data, never executed
- **Network isolation**: URL fetching is opt-in with configurable allowlists
- **Memory budgets**: Configurable memory limits to prevent DoS via large documents

## Scope

The following components are in scope for security reports:

- `markkit` core library crate
- `markkit-cli` command-line tool
- Official Python and Node.js bindings
- Published documentation

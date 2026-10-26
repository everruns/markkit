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

Markkit processes untrusted input (HTML, DOCX, spreadsheets) and converts it to markdown. Key security considerations:

- **No code execution**: scripts, styles, forms, iframes and SVG are stripped; macros and formulas are never executed; XML entities are never expanded
- **Resource limits**: input size, HTML/XML nesting depth, archive entry count and decompressed size, spreadsheet grid cells, rows and columns
- **Explicit failure**: malformed input returns errors, never panics; unsupported formats fail explicitly
- **URL fetching**: enabled by the default `http_client` feature; http/https only, non-public addresses refused by default (after DNS and on every redirect), optional host allowlist, bounded redirects, body size and fetch time. Behind an environment HTTP proxy, the proxy resolves names, so restrict egress there too

Full analysis: [threat model](knowledge/security/threat-model.md).

## Scope

The following components are in scope for security reports:

- `markkit` core library crate
- `markkit-cli` command-line tool
- Official Python and Node.js bindings
- Published documentation

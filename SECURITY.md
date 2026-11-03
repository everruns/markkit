# Security Policy

Markkit turns untrusted documents into Markdown, often inside AI agents and
services that accept files or URLs from users. We take reports seriously and
appreciate responsible disclosure.

## Reporting a Vulnerability

**Do not report security vulnerabilities through public GitHub issues,
discussions or pull requests.**

Report privately through either channel:

- [GitHub private vulnerability reporting](https://github.com/everruns/markkit/security/advisories/new)
  (preferred)
- Email **security@everruns.com**

Please include:

- Affected package(s) and version(s), and the feature flags or binding in use
- Description of the vulnerability and its impact
- Steps to reproduce, ideally a minimal input file (attach it; do not link
  public hosting) or a failing test
- Suggested fix (optional)

Inputs that make markkit panic, hang, exhaust memory past its configured
limits, execute embedded content, or reach a network address the fetch policy
should refuse are all in scope; see the invariants in the
[threat model](knowledge/security/threat-model.md).

## Response Timeline

- **Acknowledgment**: within 48 hours
- **Initial assessment**: within 7 days
- **Fix target**: within 30 days for critical issues

We coordinate disclosure with the reporter, publish a GitHub security advisory
(and a RustSec advisory for the crates) once a fixed release is available, and
credit reporters who wish to be named.

## Supported Versions

Markkit is pre-1.0. Security fixes land on `main` and ship in a new release of
the latest minor line, published in sync to crates.io, PyPI and npm.

| Version | Supported |
|---------|-----------|
| 0.3.x   | Yes       |
| < 0.3   | No        |

## Scope

- `markkit` crate (crates.io)
- `markkit-cli` crate and `markkit` command-line tool
- `markkit` Python package (PyPI)
- `@everruns/markkit` Node.js package and `@everruns/markkit-wasm` WebAssembly
  package (npm)
- `MarkkitTool` LLM agent tool (`tool` feature)
- Official examples and documentation

Out of scope: vulnerabilities in host-installed tools markkit invokes or reads
(Tesseract, OCR model files, language data), host runtimes (CPython, Node,
browsers) and third-party dependencies, unless markkit's use of them creates
the issue. Report those upstream; tell us too if markkit should mitigate them.

## Security Architecture

Key properties, enforced by tests and fuzzing:

- **No code execution**: scripts, styles, forms, iframes and SVG are stripped;
  macros, formulas and PDF JavaScript are never executed; XML entities are
  never expanded
- **Resource limits**: input size, HTML/XML nesting depth, archive entry count
  and decompressed size, spreadsheet cells/rows/columns, PDF pages/objects/
  stream sizes, output size and processing deadlines (`Limits`)
- **Explicit failure**: malformed input returns errors, never panics;
  unsupported formats fail explicitly
- **Output safety**: rendered Markdown neutralizes dangerous link schemes and
  raw HTML
- **URL fetching**: opt-in `http_client` feature (off by default; CLI and
  Python/Node bindings enable it); http/https only, non-public addresses
  refused by default (after DNS and on every redirect), optional host
  allowlist, bounded redirects, body size and fetch time. Behind an environment
  HTTP proxy, the proxy resolves names, so restrict egress there too

Full analysis: [security model](knowledge/security/security-model.md),
[threat model](knowledge/security/threat-model.md),
[native dependencies](knowledge/security/native-dependencies.md) and
[fuzzing](knowledge/security/fuzzing.md).

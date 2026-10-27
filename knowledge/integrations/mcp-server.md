---
type: Interface Contract
title: MCP Server
description: The markkit mcp stdio Model Context Protocol server exposing MarkkitTool, its methods, limits and error mapping.
tags:
  - markkit
  - tools
  - llm
  - mcp
---

# MCP Server

## Status
Implemented as the `markkit mcp` CLI subcommand (`crates/markkit-cli/src/mcp.rs`).

## Summary

`markkit mcp` serves one tool, `markkit`, over the MCP stdio transport: JSON-RPC 2.0,
one message per line on stdin, replies on stdout, diagnostics on stderr. Everything
about the tool (schemas, validation, output cap, localized errors, SSRF policy) comes
from [Tool Contract](tool-contract.md); the server only frames it.

Client config:

```json
{ "mcpServers": { "markkit": { "command": "markkit", "args": ["mcp"] } } }
```

## Methods

| Method | Reply |
|--------|-------|
| `initialize` | `protocolVersion` (client's if in `2025-06-18`, `2025-03-26`, `2024-11-05`, else `2025-06-18`), `capabilities.tools`, `serverInfo`, `instructions` = tool system prompt |
| `notifications/*` | none |
| `ping` | `{}` |
| `tools/list` | one tool: `name`, `title`, `description`, `inputSchema`, `outputSchema`, read-only annotations (`openWorldHint` only when `url` is offered) |
| `tools/call` | `content` (one text block with the result JSON), `structuredContent`, `isError: false` |
| anything else | `-32601` method not found |

## Error rules

- Malformed JSON or oversized line: `-32700`, `id: null`.
- Not an object (incl. batches, dropped in MCP 2025-06-18), bad `jsonrpc`, non string/number `id`: `-32600`.
- Unknown tool name: `-32602`.
- Tool argument or extraction failure (`ToolError::UserFacing`): result with `isError: true` and the localized message, so the model can correct itself.
- `ToolError::Internal`: logged to stderr, client sees `internal error`.
- Client responses (no `method`, has `result`/`error`) are ignored; the server sends no requests.

## Flags

`--no-url` (drop the `url` source, no network), every `extract` limit flag (`--max-input-size` alias `--max-size`, `--timeout`, `--max-nodes`, ...; see `markkit::Limits`),
`--max-output-chars` (default 100 000), `--main-content`, `--locale` (`en-US`, `uk-UA`).

## Limits

- Each line is read with a cap of `ceil(max_input_size / 3) * 4 + 1 MiB` (base64 payload plus
  framing). A longer line is drained without buffering and answered with `-32700`; the
  session continues.
- Requests are handled sequentially; one call holds at most one document.
- URL calls keep the core SSRF guard (private, loopback, link-local and metadata
  addresses refused).

## Verification

- Unit: `crates/markkit-cli/src/mcp.rs` tests (negotiation, notifications, malformed
  messages, tool errors, SSRF, `--no-url`, oversized line draining).
- End to end: `crates/markkit-cli/tests/mcp_integration.rs` spawns the binary.

## See also

- [Tool Contract](tool-contract.md), the tool this server exposes
- [Threat Model](../security/threat-model.md), MCP messages attack surface and T3 SSRF
- [Testing](../operations/testing.md)

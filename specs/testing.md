# Testing Strategy

## Test Categories

### Unit Tests
- Format detection logic
- Individual HTML element conversion
- Configuration builder
- Error handling paths

### Integration Tests
- End-to-end HTML → markdown conversion
- URL fetching + extraction pipeline
- Multi-format extraction
- Large document handling

### Property-Based Tests
- Arbitrary HTML input → no panics
- Roundtrip stability (extract twice = same result)
- Resource limits are enforced

### Security Tests
- Malicious HTML (XSS payloads stripped)
- Oversized input rejection
- Deep nesting handling
- Zip bombs (DOCX)
- Encoding edge cases

### Benchmark Tests
- Extraction throughput (MB/s)
- Memory usage
- Startup latency

## Test Data

- `tests/fixtures/` – sample documents per format
- Real-world samples from common websites
- Edge case documents (deeply nested, huge tables, etc.)

## CI Requirements

All tests must pass before merge. Security tests run with
`proptest` for fuzzy input coverage.

# Markkit Knowledge Update Log

## 2026-10-26

* **OKF migration**: `specs/` became this OKF v0.2 bundle, grouped into `foundations/`, `formats/`, `security/`, `runtimes/`, `operations/`. Every concept gained frontmatter and a See also section; `just check-okf` and CI enforce conformance. See [Knowledge Maintenance Contract](knowledge-contract.md).
* **Accuracy pass**: [Python Package](runtimes/python-package.md) now names the shipped `markkit` module; [Examples](operations/examples.md) lists the four real examples; [Evaluation Harness](operations/eval.md) is marked as a stub; [Threat Model](security/threat-model.md) T3 records that `http_client` is a default feature and SSRF mitigations are still open.
* **Maintenance contract**: [Maintenance](operations/maintenance.md) adopts the analyze, fix, ship invocation contract and a workflow credential audit.

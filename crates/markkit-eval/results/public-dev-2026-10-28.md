# Public benchmarks (dev)

2754 cases; 0 quality regressions.

| Metric | Score |
|---|---:|
| agentdocbench/category/cjk | 0.7490 |
| agentdocbench/category/docx | 0.9989 |
| agentdocbench/category/epub | 0.9891 |
| agentdocbench/category/financial | 0.4233 |
| agentdocbench/category/form | 0.7487 |
| agentdocbench/category/html | 1.0000 |
| agentdocbench/category/paper | 0.8232 |
| agentdocbench/category/report | 0.8710 |
| agentdocbench/category/scanned | 0.0000 |
| agentdocbench/category/slides | 0.6748 |
| agentdocbench/category/spreadsheet | 1.0000 |
| agentdocbench/category/two-column | 0.7466 |
| agentdocbench/format/csv | 1.0000 |
| agentdocbench/format/docx | 0.9989 |
| agentdocbench/format/epub | 0.9891 |
| agentdocbench/format/html | 1.0000 |
| agentdocbench/format/pdf | 0.5836 |
| agentdocbench/format/pptx | 0.9572 |
| agentdocbench/format/xlsx | 1.0000 |
| agentdocbench/no_ocr | 0.8204 |
| agentdocbench/overall | 0.7520 |
| wcxb/default/overall | 0.7366 |
| wcxb/default/type/article | 0.8439 |
| wcxb/default/type/collection | 0.5525 |
| wcxb/default/type/documentation | 0.8333 |
| wcxb/default/type/forum | 0.4543 |
| wcxb/default/type/listing | 0.5627 |
| wcxb/default/type/product | 0.4930 |
| wcxb/default/type/service | 0.7542 |
| wcxb/main_content/overall | 0.6534 |
| wcxb/main_content/type/article | 0.8493 |
| wcxb/main_content/type/collection | 0.3748 |
| wcxb/main_content/type/documentation | 0.8256 |
| wcxb/main_content/type/forum | 0.2724 |
| wcxb/main_content/type/listing | 0.3627 |
| wcxb/main_content/type/product | 0.4260 |
| wcxb/main_content/type/service | 0.4277 |

WCXB empty/absent full-text references: 21 pages. Upstream empty-reference F1 behavior is preserved; these cases remain in the denominator and snippet scores still apply.

## Conversion timing

Median of three fresh processes after one warm-up, sequential, startup included.

| Suite / mode | Median per-document ms | Documents |
|---|---:|---:|
| agentdocbench / default | 15.199 | 38 |
| wcxb / default | 7.877 | 1358 |
| wcxb / main_content | 8.184 | 1358 |

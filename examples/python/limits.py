"""Resource limits and error handling.

Every limit is optional with generous defaults. A breach raises RuntimeError
whose message names the limit; invalid arguments raise ValueError.

    python examples/python/limits.py
"""

import markkit

limits = markkit.Limits(
    max_input_size=64 * 1024,  # bytes in
    max_output_size=256,  # bytes of Markdown out
    max_nodes=1_000,  # parsed DOM / XML nodes
    timeout=5.0,  # seconds for the whole call
)
print(limits)

cases = {
    "small": "<h1>Fits</h1><p>Well under every limit.</p>",
    "big": "<h1>Report</h1>" + "<p>A paragraph of findings.</p>" * 50,
    "noisy": "<ul>" + "<li><b>x</b></li>" * 2_000 + "</ul>",
}
for name, html in cases.items():
    try:
        md = markkit.extract_html_sync(html, limits=limits)
        print(f"{name}: ok, {len(md)} chars")
    except RuntimeError as err:
        print(f"{name}: rejected: {err}")

# Limits apply to every function, including documents and URLs.
try:
    markkit.extract_document_sync(b"x" * 1024, file_name="big.txt", limits=markkit.Limits(max_input_size=100))
except RuntimeError as err:
    print("document:", err)

"""Markkit quick start for Python: HTML to Markdown, sync and async.

    pip install markkit
    python examples/python/quickstart.py
"""

import asyncio

import markkit

PAGE = """
<html><body>
  <nav><a href="/">Home</a> <a href="/docs">Docs</a></nav>
  <article>
    <h1>Getting started</h1>
    <p>Markkit turns <strong>untrusted</strong> documents into clean Markdown.</p>
    <p>See the <a href="guide/install">install guide</a>.</p>
  </article>
  <aside class="ad">Buy now!</aside>
  <footer>(c) Example</footer>
</body></html>
"""

# 1. Defaults: nav, header, footer, scripts, styles and forms are stripped.
print("--- extract_html_sync ---")
print(markkit.extract_html_sync(PAGE))

# 2. Selectors, main-content detection and link resolution.
print("--- extract_html_sync_with_options ---")
print(
    markkit.extract_html_sync_with_options(
        PAGE,
        include_selectors=["article"],
        exclude_selectors=[".ad"],
        base_url="https://example.com/docs/",
        main_content=True,
    )
)


# 3. Async variants run on a background Tokio runtime and return awaitables.
#    `markkit.extract_url(url)` works the same way (public hosts only by default).
async def main() -> None:
    md = await markkit.extract_html("<h2>Async</h2><p>Non-blocking extraction.</p>")
    print("--- await extract_html ---")
    print(md)
    md = await markkit.extract_bytes(b"name,score\nada,10\n", "text/csv")
    print("--- await extract_bytes (CSV) ---")
    print(md)


asyncio.run(main())

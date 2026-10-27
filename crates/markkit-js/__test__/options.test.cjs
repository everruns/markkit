// Node binding tests (run after `napi build --platform`): options parity,
// extractDocument, URL policy. All inputs inline; the URL test uses a local server.
const { test } = require("node:test");
const assert = require("node:assert/strict");
const http = require("node:http");
const m = require("..");

test("defaults unchanged without options", async () => {
  assert.equal(m.extractHtmlSync("<h1>Hello</h1>"), "# Hello\n");
  assert.equal(await m.extractHtml("<p>a</p>"), "a\n");
  assert.equal(await m.extractBytes(Buffer.from("<p>b</p>"), "text/html"), "b\n");
});

test("selector and link options apply to every entry point", async () => {
  const html = '<nav>menu</nav><article><p>Read <a href="https://x.test">this</a></p></article><aside class="ad">buy</aside>';
  const options = { preserveLinks: false, includeSelectors: ["article"], excludeSelectors: [".ad"] };
  assert.equal(m.extractHtmlSync(html, options), "Read this\n");
  assert.equal(await m.extractHtml(html, options), "Read this\n");
  assert.equal(await m.extractBytes(Buffer.from(html), "text/html", options), "Read this\n");
  assert.equal(m.extractDocumentSync(Buffer.from(html), { fileName: "a.html" }, options).markdown, "Read this\n");
});

test("input size limit", () => {
  assert.throws(() => m.extractHtmlSync("<p>too long</p>", { maxInputSize: 4 }), /Resource limit/);
});

test("invalid numeric options are rejected", () => {
  assert.throws(() => m.extractHtmlSync("<p>x</p>", { maxArchiveTotalSize: -1 }), /invalid options/);
  assert.throws(() => m.extractHtmlSync("<p>x</p>", { maxSpreadsheetCells: 1.5 }), /invalid options/);
});

test("extractDocument returns metadata", async () => {
  const out = await m.extractDocument(Buffer.from("<p>bytes</p>"), { contentType: "text/html" });
  assert.equal(out.markdown, "bytes\n");
  assert.equal(out.format, "html");
  assert.deepEqual(out.warnings, []);
  assert.equal(out.title ?? null, null);
  assert.throws(() => m.extractDocumentSync(Buffer.from("%PDF-1.7"), { fileName: "a.pdf" }), /Unsupported/);
});

test("extractDocument exposes page metadata", () => {
  const html = '<html lang="en"><head><title>T</title><meta property="og:site_name" content="Site"><link rel="canonical" href="https://x.test/a"></head><body><p>b</p></body></html>';
  const out = m.extractDocumentSync(Buffer.from(html), { contentType: "text/html" });
  assert.equal(out.title, "T");
  assert.equal(out.metadata.siteName, "Site");
  assert.equal(out.metadata.canonicalUrl, "https://x.test/a");
  assert.equal(out.metadata.language, "en");
});

function serve(body) {
  return new Promise(resolve => {
    const srv = http.createServer((_, res) => { res.setHeader("content-type", "text/html"); res.end(body); });
    srv.listen(0, "127.0.0.1", () => resolve(srv));
  });
}

test("extractUrl blocks private networks unless allowed", async () => {
  const srv = await serve("<h1>Local</h1>");
  const url = `http://127.0.0.1:${srv.address().port}/`;
  try {
    await assert.rejects(m.extractUrl(url), /URL blocked/);
    assert.equal(await m.extractUrl(url, { allowPrivateNetworks: true }), "# Local\n");
    await assert.rejects(m.extractUrl(url, { allowPrivateNetworks: true, allowedHosts: ["example.com"] }), /URL blocked/);
  } finally {
    srv.close();
  }
});

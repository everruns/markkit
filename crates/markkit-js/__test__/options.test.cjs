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

test("resource limit options", () => {
  const html = "<p>hello world</p>".repeat(200);
  assert.throws(() => m.extractHtmlSync(html, { maxOutputSize: 100 }), /max_output_size/);
  assert.throws(() => m.extractHtmlSync(html, { maxNodes: 50 }), /max_nodes/);
  assert.throws(() => m.extractHtmlSync(html, { timeout: 0 }), /timeout/);
  assert.ok(m.extractHtmlSync(html, { timeout: 1e300, maxOutputSize: 1 << 20, maxNodes: 10000 }).includes("hello"));
  const deep = "<blockquote>".repeat(20) + "<p>Deep</p>" + "</blockquote>".repeat(20);
  assert.ok(!m.extractHtmlSync(deep, { maxNestingDepth: 5 }).includes("> ".repeat(6)));
});

test("baseUrl and mainContent options", () => {
  const page = '<body><nav><a href="/n">menu</a></nav><article><h1>T</h1>' + "<p>Long article text, with commas, and more words here.</p>".repeat(8) + '<p><a href="/a">rel</a></p></article><aside>side</aside></body>';
  const md = m.extractHtmlSync(page, { baseUrl: "https://x.test/d/", mainContent: true });
  assert.ok(md.includes("(https://x.test/a)"));
  assert.ok(!md.includes("side"));
  assert.ok(m.extractHtmlSync(page).includes("side"));
});

test("invalid numeric options are rejected", () => {
  assert.throws(() => m.extractHtmlSync("<p>x</p>", { maxArchiveTotalSize: -1 }), /invalid options/);
  assert.throws(() => m.extractHtmlSync("<p>x</p>", { maxSpreadsheetCells: 1.5 }), /invalid options/);
  assert.throws(() => m.extractHtmlSync("<p>x</p>", { maxPdfDecodedSize: -1 }), /invalid options/);
  assert.throws(() => m.extractHtmlSync("<p>x</p>", { timeout: -1 }), /invalid options/);
  assert.throws(() => m.extractHtmlSync("<p>x</p>", { timeout: NaN }), /invalid options/);
});

test("extractDocument returns metadata", async () => {
  const out = await m.extractDocument(Buffer.from("<p>bytes</p>"), { contentType: "text/html" });
  assert.equal(out.markdown, "bytes\n");
  assert.equal(out.format, "html");
  assert.deepEqual(out.warnings, []);
  assert.equal(out.title ?? null, null);
  assert.throws(() => m.extractDocumentSync(Buffer.from("PK"), { fileName: "a.ppt" }), /Unsupported/);
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

test("PDF source geometry is opt-in and obeys layout limits", () => {
  const content = "BT /F1 10 Tf 10 100 Td (Hi) Tj ET";
  const source = `%PDF-1.7
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >> endobj
4 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj
5 0 obj << /Length ${content.length} >> stream
${content}
endstream endobj
trailer << /Root 1 0 R >>
%%EOF`;
  const data = Buffer.from(source);
  const hint = { fileName: "a.pdf" };
  const plain = m.extractDocumentSync(data, hint);
  assert.equal(plain.markdown, "Hi\n");
  assert.ok(plain.layout == null);
  const result = m.extractDocumentSync(data, hint, { pdfLayout: true });
  assert.equal(result.markdown, plain.markdown);
  assert.equal(result.layout.pages[0].width, 300);
  assert.equal(result.layout.pages[0].blocks[0].kind, "paragraph");
  assert.deepEqual(result.layout.pages[0].blocks[0].spanIds, [0]);
  assert.equal(result.layout.pages[0].spans[0].words[0].text, "Hi");
  assert.equal(result.layout.pages[0].height, 400);
  assert.equal(result.layout.pages[0].spans[0].text, "Hi");
  assert.deepEqual(result.layout.pages[0].spans[0].baseline, [[10, 300], [20, 300]]);
  assert.throws(() => m.extractDocumentSync(data, hint, { pdfLayout: true, maxNodes: 1 }), /max_nodes/);
  assert.throws(() => m.extractDocumentSync(data, hint, { pdfLayout: true, maxOutputSize: 50 }), /max_output_size/);
});


test("OCR backend configuration validates names", () => {
  assert.throws(() => m.extractHtmlSync("<p>Hi</p>", { pdfOcrBackend: "unknown" }), /tesseract or ocrs/);
  assert.equal(m.extractHtmlSync("<p>Hi</p>", { pdfOcrBackend: "ocrs", pdfOcrDetectionModel: "unused.onnx", pdfOcrRecognitionModel: "unused.onnx" }), "Hi\n");
});

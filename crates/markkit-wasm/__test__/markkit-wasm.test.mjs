// Headless Node tests against the built pkg/ (run scripts/build.sh first).
import { test, before } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { docx, zip } from "./zip.mjs";
import { initMarkkit, Markkit, MarkkitTool, version } from "../pkg/index.js";

const pkgJson = JSON.parse(readFileSync(new URL("../package.json", import.meta.url)));

before(async () => {
  await initMarkkit();
  await initMarkkit(); // idempotent
});

test("version matches package.json", () => {
  assert.equal(version(), pkgJson.version);
});

test("extractHtml with defaults", () => {
  const mk = new Markkit();
  assert.equal(mk.extractHtml("<h1>Hello</h1><p>World</p>"), "# Hello\n\nWorld\n");
});

test("options are applied", () => {
  const html = '<nav>menu</nav><article><p>Read <a href="https://x.test">this</a></p></article><aside class="ad">buy</aside>';
  const mk = new Markkit({ preserveLinks: false, includeSelectors: ["article"], excludeSelectors: [".ad"] });
  assert.equal(mk.extractHtml(html), "Read this\n");
});

test("unknown option is rejected", () => {
  assert.throws(() => new Markkit({ preserveLink: false }), /invalid options/);
  assert.throws(() => new Markkit({ maxInputSize: "big" }), /invalid options/);
  assert.throws(() => new Markkit().extractDocument(new Uint8Array(), { filename: "a.docx" }), /invalid options/);
});

test("input size limit", () => {
  const mk = new Markkit({ maxInputSize: 4 });
  assert.throws(() => mk.extractHtml("<p>too long</p>"), /Resource limit/);
});

test("output, node and depth limits", () => {
  const html = "<p>hello world</p>".repeat(200);
  assert.throws(() => new Markkit({ maxOutputSize: 100 }).extractHtml(html), /max_output_size/);
  assert.throws(() => new Markkit({ maxNodes: 50 }).extractHtml(html), /max_nodes/);
  const deep = "<blockquote>".repeat(20) + "<p>Deep</p>" + "</blockquote>".repeat(20);
  assert.ok(!new Markkit({ maxNestingDepth: 5 }).extractHtml(deep).includes("> ".repeat(6)));
  assert.throws(() => new Markkit({ timeout: 1 }), /invalid options/);
});

test("baseUrl and mainContent options", () => {
  const page = '<body><nav><a href="/n">menu</a></nav><article><h1>T</h1>' + "<p>Long article text, with commas, and more words here.</p>".repeat(8) + '<p><a href="/a">rel</a></p></article><aside>side</aside></body>';
  const md = new Markkit({ baseUrl: "https://x.test/d/", mainContent: true }).extractHtml(page);
  assert.ok(md.includes("(https://x.test/a)"));
  assert.ok(!md.includes("side"));
  assert.ok(new Markkit().extractHtml(page).includes("side"));
});

test("extractDocument returns metadata", () => {
  const mk = new Markkit();
  const out = mk.extractDocument(new TextEncoder().encode("<p>bytes</p>"), { fileName: "a.html" });
  assert.equal(out.markdown, "bytes\n");
  assert.equal(out.format, "html");
  assert.deepEqual(out.warnings, []);
  assert.equal(out.title, null);
});

test("extractDocument exposes page metadata in camelCase", () => {
  const html = '<html lang="en"><head><title>T</title><meta property="og:site_name" content="Site"><link rel="canonical" href="https://x.test/a"></head><body><p>b</p></body></html>';
  const out = new Markkit().extractDocument(new TextEncoder().encode(html), { contentType: "text/html" });
  assert.equal(out.title, "T");
  assert.equal(out.metadata.siteName, "Site");
  assert.equal(out.metadata.canonicalUrl, "https://x.test/a");
  assert.equal(out.metadata.language, "en");
  assert.equal(out.metadata.site_name, undefined);
});

test("extractDocument reads plain text and Markdown", () => {
  const mk = new Markkit();
  const txt = mk.extractDocument(new TextEncoder().encode("a\nb"), { fileName: "n.txt" });
  assert.equal(txt.format, "text");
  assert.equal(txt.markdown, "a  \nb\n");
  const md = mk.extractDocument(new TextEncoder().encode("# T\n\n<script>x()</script>\n"), { fileName: "d.md" });
  assert.equal(md.format, "markdown");
  assert.equal(md.title, "T");
  assert.ok(!md.markdown.includes("script"));
});

test("extractDocument reads CSV and TSV", () => {
  const mk = new Markkit();
  const csv = mk.extractDocument(new TextEncoder().encode("a;b\n1;2\n"), { fileName: "t.csv" });
  assert.equal(csv.format, "csv");
  assert.equal(csv.markdown, "| a | b |\n| --- | --- |\n| 1 | 2 |\n");
  const tsv = mk.extractDocument(new TextEncoder().encode("x\ty\n"), { fileName: "t.tsv" });
  assert.equal(tsv.format, "tsv");
});

test("extractDocument reads Jupyter notebooks", () => {
  const nb = { nbformat: 4, metadata: {}, cells: [{ cell_type: "markdown", source: "# NB" }, { cell_type: "code", source: "1+1", outputs: [] }] };
  const out = new Markkit().extractDocument(new TextEncoder().encode(JSON.stringify(nb)), { fileName: "a.ipynb" });
  assert.equal(out.format, "ipynb");
  assert.equal(out.title, "NB");
  assert.ok(out.markdown.includes("1+1"));
});

test("extractBytes and unsupported formats", () => {
  const mk = new Markkit();
  assert.equal(mk.extractBytes(new TextEncoder().encode("<p>x</p>"), "text/html"), "x\n");
  assert.throws(() => mk.extractDocument(new TextEncoder().encode("%PDF-1.7"), { fileName: "a.pdf" }), /PDF extraction error/);
  assert.throws(() => mk.extractDocument(new Uint8Array([0]), { fileName: "a.doc" }), /Unsupported/);
});

test("tool metadata and definition", () => {
  const tool = new MarkkitTool();
  assert.equal(tool.name, "markkit");
  assert.equal(tool.locale, "en-US");
  assert.ok(tool.systemPrompt().startsWith("markkit:"));
  const def = tool.toolDefinition();
  assert.equal(def.type, "function");
  assert.equal(def.function.name, "markkit");
  // No http_client in wasm: the url source is not offered.
  assert.equal(def.function.parameters.properties.url, undefined);
  assert.deepEqual(tool.inputSchema(), def.function.parameters);
  assert.ok(tool.outputSchema().required.includes("markdown"));
});

test("tool rejects unknown options", () => {
  assert.throws(() => new MarkkitTool({ maxOutputChar: 5 }), /invalid options/);
  assert.throws(() => new MarkkitTool({ locale: 5 }), /invalid options/);
  assert.throws(() => new MarkkitTool("x"), /invalid options/);
});

test("tool locale", () => {
  const tool = new MarkkitTool({ locale: "uk-UA" });
  assert.equal(tool.name, "markkit");
  assert.equal(tool.displayName, "Витяг Markdown");
});

test("tool executes html and base64", async () => {
  const tool = new MarkkitTool({ maxOutputChars: 5 });
  const html = await tool.execute({ html: "<h1>Hi</h1>" });
  assert.equal(html.markdown, "# Hi\n");
  const b64 = Buffer.from("<p>abcdefgh</p>").toString("base64");
  const out = await tool.execute({ content_base64: b64, file_name: "a.html" });
  assert.equal(out.markdown, "abcde");
  assert.equal(out.truncated, true);
});

test("tool errors carry userFacing", async () => {
  const tool = new MarkkitTool();
  await assert.rejects(tool.execute({}), e => e.userFacing === true && /exactly one/.test(e.message));
  await assert.rejects(tool.execute({ url: "https://example.com" }), e => e.userFacing === true && /disabled/.test(e.message));
  await assert.rejects(tool.execute({ content_base64: "!!" }), e => e.userFacing === true);
});

test("DOCX converts (zip + XML stack works in wasm)", () => {
  const mk = new Markkit();
  const bytes = docx('<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Report</w:t></w:r></w:p><w:p><w:r><w:t>Body text</w:t></w:r></w:p>');
  const out = mk.extractDocument(bytes, { fileName: "r.docx" });
  assert.equal(out.format, "docx");
  assert.match(out.markdown, /Report/);
  assert.match(out.markdown, /Body text/);
  assert.equal(out.stats.paragraphs, 2);
});

test("zip bomb style archive limits hold in wasm", () => {
  const mk = new Markkit({ maxArchiveEntrySize: 1024 });
  const bytes = docx(`<w:p><w:r><w:t>${"a".repeat(64 * 1024)}</w:t></w:r></w:p>`);
  assert.throws(() => mk.extractDocument(bytes, { fileName: "r.docx" }), /limit/i);
});

test("malformed office input throws, never traps", () => {
  const mk = new Markkit();
  assert.throws(() => mk.extractDocument(zip([["junk", "x"]]), { fileName: "a.docx" }));
  assert.throws(() => mk.extractDocument(new Uint8Array([0x50, 0x4b, 3, 4, 0, 0]), { fileName: "a.xlsx" }));
  // Instance still usable after errors.
  assert.equal(mk.extractHtml("<p>ok</p>"), "ok\n");
});

test("tool converts base64 DOCX", async () => {
  const tool = new MarkkitTool();
  const b64 = Buffer.from(docx("<w:p><w:r><w:t>From tool</w:t></w:r></w:p>")).toString("base64");
  const out = await tool.execute({ content_base64: b64, file_name: "a.docx" });
  assert.equal(out.format, "docx");
  assert.match(out.markdown, /From tool/);
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
  const data = new TextEncoder().encode(source);
  const hint = { fileName: "a.pdf" };
  const plain = new Markkit().extractDocument(data, hint);
  assert.equal(plain.markdown, "Hi\n");
  assert.ok(plain.layout == null);
  const result = new Markkit({ pdfLayout: true }).extractDocument(data, hint);
  assert.equal(result.markdown, plain.markdown);
  assert.equal(result.layout.pages[0].width, 300);
  assert.equal(result.layout.pages[0].blocks[0].kind, "paragraph");
  assert.deepEqual(result.layout.pages[0].blocks[0].spanIds, [0]);
  assert.equal(result.layout.pages[0].spans[0].words[0].text, "Hi");
  assert.equal(result.layout.pages[0].height, 400);
  assert.equal(result.layout.pages[0].spans[0].text, "Hi");
  assert.deepEqual(result.layout.pages[0].spans[0].baseline, [[10, 300], [20, 300]]);
  assert.throws(() => new Markkit({ pdfLayout: true, maxNodes: 1 }).extractDocument(data, hint), /max_nodes/);
  assert.throws(() => new Markkit({ pdfLayout: true, maxOutputSize: 50 }).extractDocument(data, hint), /max_output_size/);
});

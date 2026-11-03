// @everruns/markkit-wasm in any JavaScript runtime (browser, edge, Deno, Bun, Node).
//
//   npm install @everruns/markkit-wasm
//   node examples/wasm/quickstart.mjs
//
// Inside this repository, build the package first (`just wasm`); the example
// then loads crates/markkit-wasm/pkg. Set MARKKIT_WASM_MODULE to override.
import { readFile } from "node:fs/promises";

const modulePath =
  process.env.MARKKIT_WASM_MODULE ?? new URL("../../crates/markkit-wasm/pkg/index.js", import.meta.url).href;
const { initMarkkit, Markkit, MarkkitTool, version } = await import(modulePath);

// Load the .wasm once. Browsers fetch it next to the module; Node reads it from disk.
await initMarkkit();
console.log(`markkit-wasm ${version()}`);

// 1. HTML. Options are fixed per instance; reuse it for many calls.
const mk = new Markkit({ includeSelectors: ["article"], preserveImages: false });
console.log("--- extractHtml ---");
console.log(mk.extractHtml('<nav>menu</nav><article><h1>Hello</h1><p>From <b>WebAssembly</b>.</p></article>'));

// 2. Documents from bytes. In a browser: new Uint8Array(await res.arrayBuffer()).
//    There is no URL fetching in WASM: the host fetches, so network policy stays with it.
const docs = new Markkit();
for (const name of ["report.docx", "sales.xlsx", "sample.pdf"]) {
  const bytes = new Uint8Array(await readFile(new URL(`../data/${name}`, import.meta.url)));
  const doc = docs.extractDocument(bytes, { fileName: name });
  console.log(`=== ${name}: ${doc.format}, title ${JSON.stringify(doc.title)} ===`);
  console.log(doc.markdown);
}

// 3. Agent tool: an LLM tool definition plus a validated `execute`.
const tool = new MarkkitTool({ maxOutputChars: 2000 });
console.log("--- tool definition ---");
console.log(JSON.stringify(tool.toolDefinition()).slice(0, 120) + "...");
const result = await tool.execute({ html: "<h1>Tool call</h1><p>Result for the model.</p>" });
console.log("--- tool result ---");
console.log(result);

// 4. Errors: limits and bad input throw; `userFacing` errors are safe to show a model.
try {
  new Markkit({ maxOutputSize: 8 }).extractHtml("<p>This output is longer than eight bytes.</p>");
} catch (err) {
  console.log("--- limit ---");
  console.log(err.message);
}
mk.free(); // optional: release wasm memory early
docs.free();

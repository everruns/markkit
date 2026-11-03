// Markkit quick start for Node.js: HTML to Markdown, sync and async.
//
//   npm install @everruns/markkit
//   node examples/node/quickstart.cjs
//
// Inside this repository the example loads the locally built binding
// (crates/markkit-js); set MARKKIT_MODULE to point elsewhere.
const { extractHtml, extractHtmlSync, extractBytes } = require(
  process.env.MARKKIT_MODULE ?? '@everruns/markkit',
);

const page = `
<html><body>
  <nav><a href="/">Home</a> <a href="/docs">Docs</a></nav>
  <article>
    <h1>Getting started</h1>
    <p>Markkit turns <strong>untrusted</strong> documents into clean Markdown.</p>
    <p>See the <a href="guide/install">install guide</a>.</p>
  </article>
  <aside class="ad">Buy now!</aside>
</body></html>`;

async function main() {
  // 1. Defaults: nav, header, footer, scripts, styles and forms are stripped.
  console.log('--- extractHtmlSync ---');
  console.log(extractHtmlSync(page));

  // 2. Every function takes a trailing options object (flat camelCase keys).
  console.log('--- with options ---');
  console.log(
    await extractHtml(page, {
      includeSelectors: ['article'],
      excludeSelectors: ['.ad'],
      baseUrl: 'https://example.com/docs/',
      preserveImages: false,
    }),
  );

  // 3. Bytes with a content type. `extractUrl(url, options)` works the same
  //    way; non-public addresses are blocked by default.
  console.log('--- extractBytes (CSV) ---');
  console.log(await extractBytes(Buffer.from('name,score\nada,10\n'), 'text/csv'));
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});

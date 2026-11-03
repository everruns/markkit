// Convert DOCX, XLSX and PDF files with metadata, stats and warnings.
//
//   node examples/node/documents.cjs                  # bundled samples
//   node examples/node/documents.cjs my.pptx my.epub  # your own files
const fs = require('node:fs');
const path = require('node:path');
const { extractDocument, extractDocumentSync } = require(
  process.env.MARKKIT_MODULE ?? '@everruns/markkit',
);

const data = path.join(__dirname, '..', 'data');
const files = process.argv.slice(2);
if (files.length === 0) {
  files.push(...['report.docx', 'sales.xlsx', 'sample.pdf'].map((f) => path.join(data, f)));
}

async function main() {
  for (const file of files) {
    // Format detection: hint.contentType, then hint.fileName, then magic bytes.
    const doc = await extractDocument(fs.readFileSync(file), { fileName: path.basename(file) });
    console.log(`=== ${path.basename(file)} ===`);
    console.log('format:', doc.format, '| title:', doc.title ?? '(none)');
    console.log('stats:', JSON.stringify(doc.stats));
    for (const w of doc.warnings) console.log(`warning [${w.code}]: ${w.detail}`);
    console.log(`\n${doc.markdown}`);
  }

  // Limits are option keys too; breaches throw an Error naming the limit.
  try {
    extractDocumentSync(fs.readFileSync(path.join(data, 'report.docx')), { fileName: 'report.docx' }, { maxOutputSize: 64 });
  } catch (err) {
    console.log('limit:', err.message);
  }

  // PDF geometry: pages, positioned spans and semantic blocks (camelCase keys).
  const pdf = extractDocumentSync(fs.readFileSync(path.join(data, 'sample.pdf')), { fileName: 'sample.pdf' }, { pdfLayout: true });
  const page = pdf.layout.pages[0];
  console.log(`=== sample.pdf layout: ${page.width}x${page.height} pt ===`);
  for (const b of page.blocks) console.log(b.kind.padEnd(10), b.bbox.map(Math.round));
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});

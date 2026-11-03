/** Load the WebAssembly module. Call (and await) once before using any class. */
export function initMarkkit(
  input?: RequestInfo | URL | Response | BufferSource | WebAssembly.Module,
): Promise<void>;

/** Library version. */
export function version(): string;

/** Extraction options (all optional; defaults match the Rust builder). */
export interface MarkkitOptions {
  /** Maximum input size in bytes (default 50 MiB). */
  maxInputSize?: number;
  /** Maximum Markdown output in bytes (default 128 MiB). */
  maxOutputSize?: number;
  /** Maximum parsed nodes (default 5 000 000). */
  maxNodes?: number;
  /** Maximum element nesting (default 256, capped at 512). */
  maxNestingDepth?: number;
  /** Absolute URL relative links and images resolve against. */
  baseUrl?: string;
  /** Keep only the detected main content (article body) of HTML pages. */
  mainContent?: boolean;
  /** Include PDF source geometry in extractDocument results. */
  pdfLayout?: boolean;
  /** Drop margin headers/footers repeated across PDF pages (default false). */
  pdfRemoveRepeatedHeaders?: boolean;
  /** PDF pages rendered; later pages are truncated with a warning (default 10 000). */
  maxPdfPages?: number;
  /** Decoded bytes per PDF stream (default 64 MiB). */
  maxPdfStreamSize?: number;
  /** Decoded PDF bytes in total, also bounds layout geometry (default 256 MiB). */
  maxPdfDecodedSize?: number;
  /** Keep link URLs as `[text](url)` (default true); false keeps the text only. */
  preserveLinks?: boolean;
  /** Keep images as `![alt](src)` (default true); false drops them. */
  preserveImages?: boolean;
  /** CSS selectors to include (empty = whole document). */
  includeSelectors?: string[];
  /** CSS selectors to drop, added to nav/footer/header/script/style/noscript. */
  excludeSelectors?: string[];
  /** Maximum ZIP entries in DOCX/XLSX containers (default 10 000). */
  maxArchiveEntries?: number;
  /** Maximum decompressed bytes per ZIP entry (default 64 MiB). */
  maxArchiveEntrySize?: number;
  /** Maximum decompressed bytes per ZIP container (default 256 MiB). */
  maxArchiveTotalSize?: number;
  /** Rows rendered per sheet; extra rows are truncated with a warning (default 10 000). */
  maxSheetRows?: number;
  /** Columns rendered per sheet; extra columns are truncated with a warning (default 256). */
  maxSheetCols?: number;
  /** Maximum XLS/ODS grid cells before parsing (default 4 000 000). */
  maxSpreadsheetCells?: number;
  /** Render hidden and very hidden sheets (default false: skipped with a warning). */
  includeHiddenSheets?: boolean;
}

export interface DocumentHint {
  /** MIME type, e.g. from a `Content-Type` header. */
  contentType?: string;
  /** File name or URL path; only the extension is used. */
  fileName?: string;
}

export interface ExtractionStats {
  pages: number | null;
  sheets: number | null;
  rows: number | null;
  paragraphs: number | null;
  tables: number | null;
}

/** Document metadata (HTML meta, Open Graph, JSON-LD); fields are null when absent. */
export interface Metadata {
  description: string | null;
  author: string | null;
  published: string | null;
  language: string | null;
  canonicalUrl: string | null;
  siteName: string | null;
  image: string | null;
}

export interface Warning {
  /** Stable machine code (`truncated_sheet`, `hidden_slide_skipped`, ...). */
  code: string;
  detail: string;
}

export interface Extraction {
  layout?: PdfLayout;
  markdown: string;
  title: string | null;
  format: string;
  warnings: Warning[];
  stats: ExtractionStats;
  metadata: Metadata;
}

/** Source geometry in displayed crop-box points, x right and y down. */
export interface PdfLayout {
  version: number;
  pages: {
    page: number;
    width: number;
    height: number;
    rotation: number;
    blocks: PdfBlock[];
    spans: {
      id: number;
      text: string;
      /** Approximate em bounds: [left, top, right, bottom]. */
      bbox: [number, number, number, number];
      baseline: [[number, number], [number, number]];
      font?: string;
      size: number;
      angle: number;
      words: {text:string; bbox:[number,number,number,number]; baseline:[[number,number],[number,number]]}[];
      bold?: boolean;
      italic?: boolean;
      role?: string;
      ocr?: boolean;
    }[];
  }[];
}

export interface PdfBlock {
  id:number;
  kind:string;
  bbox:[number,number,number,number];
  spanIds:number[];
  lines:{bbox:[number,number,number,number];spanIds:number[]}[];
  cells?:{row:number;column:number;rowspan:number;colspan:number;bbox:[number,number,number,number];spanIds:number[]}[];
}

/**
 * Extractor with fixed options; create once and reuse. Methods are synchronous
 * and throw an `Error` naming the cause (exceeded limit, unsupported format,
 * parse failure). Unknown option keys throw in the constructor.
 */
export class Markkit {
  constructor(options?: MarkkitOptions);
  /** Convert an HTML string to Markdown. */
  extractHtml(html: string): string;
  /**
   * Convert document bytes (HTML, PDF, DOCX, XLSX/XLSM/XLSB/XLS/ODS, CSV/TSV,
   * ipynb, Markdown, text) to Markdown plus title, format, warnings, stats and
   * metadata. Format detection: `hint.contentType`, then `hint.fileName`'s
   * extension, then magic bytes.
   */
  extractDocument(bytes: Uint8Array, hint?: DocumentHint): Extraction;
  /** Convert bytes to Markdown only, detected from `contentType` and the bytes. */
  extractBytes(bytes: Uint8Array, contentType?: string): string;
  /** Release the wasm memory now instead of on garbage collection. */
  free(): void;
}

export interface MarkkitToolOptions extends MarkkitOptions {
  /** `en-US` (default) or `uk-UA`. */
  locale?: string;
  /** Cap on returned Markdown characters (default 100 000). */
  maxOutputChars?: number;
}

/** Tool call arguments (exactly one of `html` / `content_base64`). */
export interface MarkkitToolArgs {
  html?: string;
  content_base64?: string;
  file_name?: string;
  content_type?: string;
  max_chars?: number | null;
}

export interface MarkkitToolResult {
  markdown: string;
  title: string | null;
  format: string;
  warnings: string[];
  truncated: boolean;
}

/** Rejection reason of `MarkkitTool.execute`. */
export interface MarkkitToolError extends Error {
  /** True when the message is safe to return to the model. */
  userFacing: boolean;
}

/** LLM agent tool (Everruns toolkit contract). No `url` source in wasm. */
export class MarkkitTool {
  constructor(options?: MarkkitToolOptions);
  readonly name: string;
  readonly displayName: string;
  readonly shortDescription: string;
  readonly description: string;
  readonly version: string;
  readonly locale: string;
  help(): string;
  systemPrompt(): string;
  inputSchema(): Record<string, unknown>;
  outputSchema(): Record<string, unknown>;
  /** OpenAI-compatible function tool definition. */
  toolDefinition(): Record<string, unknown>;
  execute(args: MarkkitToolArgs): Promise<MarkkitToolResult>;
  free(): void;
}

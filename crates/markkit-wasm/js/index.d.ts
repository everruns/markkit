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
  preserveLinks?: boolean;
  preserveImages?: boolean;
  /** CSS selectors to include (empty = whole document). */
  includeSelectors?: string[];
  /** CSS selectors to drop, added to nav/footer/header/script/style/noscript. */
  excludeSelectors?: string[];
  maxArchiveEntries?: number;
  maxArchiveEntrySize?: number;
  maxArchiveTotalSize?: number;
  maxSheetRows?: number;
  maxSheetCols?: number;
  maxSpreadsheetCells?: number;
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

export interface Extraction {
  markdown: string;
  title: string | null;
  format: string;
  warnings: string[];
  stats: ExtractionStats;
  metadata: Metadata;
}

export class Markkit {
  constructor(options?: MarkkitOptions);
  extractHtml(html: string): string;
  extractDocument(bytes: Uint8Array, hint?: DocumentHint): Extraction;
  extractBytes(bytes: Uint8Array, contentType?: string): string;
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

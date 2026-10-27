/* tslint:disable */
/* eslint-disable */

/**
 * Configured extractor.
 */
export class Markkit {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Document bytes to Markdown only.
     */
    extractBytes(bytes: Uint8Array, content_type?: string | null): string;
    /**
     * Document bytes to `{ markdown, title, format, warnings, stats }`.
     * `hint` is `{ contentType?, fileName? }`.
     */
    extractDocument(bytes: Uint8Array, hint: any): any;
    /**
     * HTML string to Markdown.
     */
    extractHtml(html: string): string;
    /**
     * `new Markkit(options?)`.
     */
    constructor(options: any);
}

/**
 * LLM agent tool (Everruns toolkit contract). Accepts `html` or `content_base64`.
 */
export class MarkkitTool {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Run one call. Resolves to `ToolOutput.result`; rejects with an `Error`
     * whose `userFacing` property says whether the message is safe for the model.
     */
    execute(args: any): Promise<any>;
    help(): string;
    inputSchema(): any;
    /**
     * `new MarkkitTool({ locale?, maxOutputChars?, ...extraction options })`.
     */
    constructor(options: any);
    outputSchema(): any;
    systemPrompt(): string;
    /**
     * OpenAI-compatible function tool definition.
     */
    toolDefinition(): any;
    readonly description: string;
    readonly displayName: string;
    readonly locale: string;
    readonly name: string;
    readonly shortDescription: string;
    readonly version: string;
}

/**
 * Library version.
 */
export function version(): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_markkit_free: (a: number, b: number) => void;
    readonly __wbg_markkittool_free: (a: number, b: number) => void;
    readonly markkit_extractBytes: (a: number, b: number, c: number, d: number, e: number, f: number) => void;
    readonly markkit_extractDocument: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly markkit_extractHtml: (a: number, b: number, c: number, d: number) => void;
    readonly markkit_new: (a: number, b: number) => void;
    readonly markkittool_description: (a: number, b: number) => void;
    readonly markkittool_displayName: (a: number, b: number) => void;
    readonly markkittool_execute: (a: number, b: number) => number;
    readonly markkittool_help: (a: number, b: number) => void;
    readonly markkittool_inputSchema: (a: number, b: number) => void;
    readonly markkittool_locale: (a: number, b: number) => void;
    readonly markkittool_name: (a: number, b: number) => void;
    readonly markkittool_new: (a: number, b: number) => void;
    readonly markkittool_outputSchema: (a: number, b: number) => void;
    readonly markkittool_shortDescription: (a: number, b: number) => void;
    readonly markkittool_systemPrompt: (a: number, b: number) => void;
    readonly markkittool_toolDefinition: (a: number, b: number) => void;
    readonly markkittool_version: (a: number, b: number) => void;
    readonly version: (a: number) => void;
    readonly __wasm_bindgen_func_elem_1938: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_1940: (a: number, b: number, c: number, d: number) => void;
    readonly __wbindgen_export: (a: number, b: number) => number;
    readonly __wbindgen_export2: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_export3: (a: number) => void;
    readonly __wbindgen_export4: (a: number, b: number) => void;
    readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
    readonly __wbindgen_export5: (a: number, b: number, c: number) => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;

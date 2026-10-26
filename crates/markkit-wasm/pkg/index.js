// Entry point for @everruns/markkit-wasm.
//
// `initMarkkit()` loads the .wasm once. In browsers and edge runtimes the
// generated glue fetches `markkit_wasm_bg.wasm` relative to this module; in
// Node (no fetch for file: URLs) the bytes are read from disk instead.
import init, { Markkit, MarkkitTool, version } from "./markkit_wasm.js";

let ready;

export function initMarkkit(input) {
  if (!ready) {
    ready = (async () => {
      let source = input;
      if (source === undefined && typeof process !== "undefined" && process.versions?.node) {
        const { readFile } = await import("node:fs/promises");
        source = await readFile(new URL("./markkit_wasm_bg.wasm", import.meta.url));
      }
      await init(source === undefined ? undefined : { module_or_path: source });
    })();
  }
  return ready;
}

export { Markkit, MarkkitTool, version };

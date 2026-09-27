// Wrangler turns `import x from './file.wasm'` into a compiled WebAssembly.Module;
// Vitest cannot, so tests get the same thing from here (aliased in vitest.config.js).
import { readFileSync } from 'node:fs'

export default new WebAssembly.Module(
  readFileSync(new URL('../src/idleview_core.wasm', import.meta.url))
)

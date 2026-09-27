// idleview-core (core/src/wasm.rs), compiled to WebAssembly. The web page's time
// formatting, weather tiles and photo search come from the same Rust as the desktop
// app's, so the two cannot drift.
//
// The ABI is JSON in, JSON out through linear memory: write the request into a block
// from `alloc`, call, read `(ptr << 32) | len` back, free it.
//
// idleview_core.wasm is built, not committed: `npm run build:core`.

import wasmModule from './idleview_core.wasm'

const { exports: core } = new WebAssembly.Instance(wasmModule, {})
const encoder = new TextEncoder()
const decoder = new TextDecoder()

function call(name, request) {
  const input = encoder.encode(JSON.stringify(request))
  const ptr = core.alloc(input.length)
  new Uint8Array(core.memory.buffer, ptr, input.length).set(input)

  // Rust takes ownership of the input block and frees it.
  const packed = core[name](ptr, input.length)
  const outPtr = Number(packed >> 32n)
  const outLen = Number(packed & 0xffffffffn)
  const text = decoder.decode(new Uint8Array(core.memory.buffer, outPtr, outLen))
  core.dealloc(outPtr, outLen)

  const result = JSON.parse(text)
  if (result && typeof result === 'object' && 'error' in result) {
    throw new Error(`idleview-core ${name}: ${result.error}`)
  }
  return result
}

/** The Unsplash search for this time and weather (always one on queries.json). */
export const photoQuery = (request) => call('photo_query', request)

/** The finished view, the same shape the desktop app draws. */
export const view = (request) => call('view', request)

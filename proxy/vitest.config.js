import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  resolve: {
    alias: [
      {
        find: /^\.\/idleview_core\.wasm$/,
        replacement: fileURLToPath(new URL('./test/wasm-module.js', import.meta.url))
      }
    ]
  },
  test: {
    include: ['src/**/*.test.js']
  }
})

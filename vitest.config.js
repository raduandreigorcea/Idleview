import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.js', 'tests/**/*.test.js'],
    globals: true,
  },
});

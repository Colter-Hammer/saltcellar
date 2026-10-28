import { fileURLToPath, URL } from 'node:url'
import tailwindcss from '@tailwindcss/vite'
import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  plugins: [vue(), tailwindcss()],
  resolve: { alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) } },
  // `just dev`: Vite proxies /api to the API, at BUDGET_LISTEN like `just dev-api`.
  server: {
    port: 5173,
    strictPort: true,
    proxy: { '/api': `http://${process.env.BUDGET_LISTEN ?? '127.0.0.1:8080'}` },
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts', 'checks/**/*.test.ts'],
    // The floor in ../coverage-floors.txt (`web`) is on frontend logic: the TypeScript modules.
    coverage: {
      provider: 'v8',
      include: ['src/**/*.ts'],
      exclude: ['src/**/*.test.ts', 'src/main.ts', 'src/env.d.ts'],
      reporter: ['text', 'json-summary'],
    },
  },
})

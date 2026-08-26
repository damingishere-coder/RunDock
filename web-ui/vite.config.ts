import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    react(),
    tailwindcss(),
    {
      name: 'enforce-javascript-chunk-budget',
      generateBundle(_options, bundle) {
        const oversized = Object.values(bundle).flatMap(output =>
          output.type === 'chunk' && output.code.length > 500 * 1024
            ? [{ fileName: output.fileName, bytes: output.code.length }]
            : []
        )
        if (oversized.length) {
          this.error(
            `JavaScript chunk budget exceeded: ${oversized
              .map(chunk => `${chunk.fileName} (${chunk.bytes} bytes)`)
              .join(', ')}`
          )
        }
      },
    },
  ],
  base: '/',
  resolve: {
    alias: { '@': path.resolve(__dirname, './src') },
    dedupe: ['react', 'react-dom'],
  },
  // @group Testing : vitest configuration
  test: {
    globals: true,
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts'],
    css: false,
    alias: { '@': path.resolve(__dirname, './src') },
    coverage: {
      reporter: ['text', 'lcov'],
      thresholds: {
        statements: 20,
        branches: 15,
        functions: 15,
        lines: 20,
      },
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: {
      output: {
        manualChunks(id) {
          const normalized = id.replace(/\\/g, '/')
          if (normalized.includes('/node_modules/@xterm/')) return 'terminal-vendor'
          if (normalized.includes('/node_modules/@tanstack/')) return 'query-vendor'
          if (normalized.includes('/node_modules/lucide-react/')) return 'icons-vendor'
          if (
            normalized.includes('/node_modules/react/') ||
            normalized.includes('/node_modules/react-dom/') ||
            normalized.includes('/node_modules/react-router')
          )
            return 'react-vendor'
          return undefined
        },
      },
    },
  },
  server: {
    port: 5173,
    host: '0.0.0.0',
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:2999',
        changeOrigin: true,
        ws: true,
      },
    },
  },
})

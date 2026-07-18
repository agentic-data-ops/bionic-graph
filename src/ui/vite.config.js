/// <reference types="vitest" />
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  base: '/ui/',
  server: {
    proxy: {
      '/': {
        target: 'http://127.0.0.1:8080',
        bypass: (req) => {
          // Let Vite serve its own files — UI assets, source, and dev client
          if (req.url === '/' || req.url === '') return req.url;
          if (req.url.startsWith('/ui/')) return req.url;
          if (req.url.startsWith('/src/')) return req.url;
          if (req.url.startsWith('/favicon')) return req.url;
          if (req.url.startsWith('/node_modules/')) return req.url;
          if (req.url.startsWith('/@')) return req.url;
          return null; // proxy API calls to backend
        }
      }
    }
  },
  build: { outDir: 'dist', emptyOutDir: true, minify: false },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: './src/test-setup.js',
  },
})

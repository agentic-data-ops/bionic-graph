import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: { proxy: { '/': 'http://127.0.0.1:8080' } },
  build: { outDir: 'dist', emptyOutDir: true },
})

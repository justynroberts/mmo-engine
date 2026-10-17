import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'

// `npm run dev` proxies the admin API of a locally running server.
export default defineConfig({
  plugins: [react()],
  base: './',
  server: { port: 5183, proxy: { '/api': 'http://127.0.0.1:7780' } },
  build: { outDir: 'dist', sourcemap: false, rolldownOptions: { input: { main: 'index.html', play: 'play.html' } } },
  test: { environment: 'jsdom', setupFiles: ['./src/test/setup.ts'], globals: true, css: false },
})

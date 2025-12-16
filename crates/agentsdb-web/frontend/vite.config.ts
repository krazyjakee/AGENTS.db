import { defineConfig } from 'vite'
import preact from '@preact/preset-vite'

// https://vite.dev/config/
export default defineConfig({
  plugins: [preact()],
  build: {
    outDir: '../dist',
    emptyOutDir: true,
    sourcemap: true,
  },
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://localhost:9090',
        changeOrigin: true,
      },
      '/logo.png': {
        target: 'http://localhost:9090',
        changeOrigin: true,
      },
    },
  },
})

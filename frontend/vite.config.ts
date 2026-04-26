import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  server: {
    host: '0.0.0.0',
    port: 5000,
    allowedHosts: true,
    proxy: {
      '/api-backend': {
        target: 'http://localhost:8000',
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/api-backend/, ''),
      },
      '/api-cp': {
        target: 'http://localhost:8088',
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/api-cp/, ''),
      },
      '/ws-cp': {
        target: 'ws://localhost:8088',
        ws: true,
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/ws-cp/, ''),
      },
    },
  },
})

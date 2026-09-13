import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { fileURLToPath, URL } from 'node:url'

// 前端产物由 Rust 核心（axum）托管，桌面端与浏览器端共用同一份构建
export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) }
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    // 体积优化：路由级分包 + 压缩
    target: 'es2021',
    cssCodeSplit: true,
    chunkSizeWarningLimit: 900,
    rollupOptions: {
      output: {
        manualChunks: {
          vendor: ['vue', 'vue-router', 'pinia'],
          markdown: ['marked', 'dompurify']
        }
      }
    }
  },
  server: {
    port: 5173,
    // 开发模式：把 /api 代理到本地 Rust 核心
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:17801',
        changeOrigin: true,
        ws: false
      }
    }
  }
})

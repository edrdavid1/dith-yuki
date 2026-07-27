import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  base: './',

  // Не затирать вывод cargo при перезапуске dev-сервера
  clearScreen: false,

  server: {
    port: 5173,
    // Падать с ошибкой вместо тихого перехода на 5174:
    // иначе Tauri откроет пустое окно на devUrl из tauri.conf.json
    strictPort: true,
  },

  build: {
    target: 'esnext',
    minify: 'terser',
    outDir: 'dist',
  },
})

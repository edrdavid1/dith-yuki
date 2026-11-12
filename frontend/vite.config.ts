import { execFileSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { defineConfig, type Plugin } from 'vite'
import react from '@vitejs/plugin-react'

const frontendRoot = path.dirname(fileURLToPath(import.meta.url))

type FlexPatchMarker = { version: number; features: string[] }

/**
 * W3: verify the patch-package marker; only clear Vite's flexlayout prebundle
 * when verify reports it stale (exit 2). Expose marker via virtual module for
 * the runtime gate (browser has no require of the CJS FloatingWindow).
 */
function ensureFlexlayoutFloatPatch(): Plugin {
  let marker: FlexPatchMarker | null = null

  const runVerify = () => {
    let stdout = ''
    try {
      stdout = execFileSync(
        process.execPath,
        ['scripts/verify-flexlayout-patch.cjs', '--json'],
        { cwd: frontendRoot, encoding: 'utf8' },
      )
    } catch (err) {
      marker = null
      throw new Error(
        '[ensure-flexlayout-float-patch] flexlayout-react patch missing — run npm install in frontend (patch-package postinstall)',
        { cause: err },
      )
    }

    try {
      // verify may print warnings before the JSON line
      const lines = stdout.trim().split('\n')
      const jsonLine = [...lines].reverse().find((l) => l.startsWith('{')) || '{}'
      const parsed = JSON.parse(jsonLine) as {
        marker?: FlexPatchMarker
        viteStale?: boolean
      }
      marker = parsed.marker ?? null
      if (!marker) {
        throw new Error('verify JSON missing marker')
      }
      // Stale Vite prebundle is cleared inside verify; no blind clear here.
    } catch (err) {
      marker = null
      throw new Error(
        `[ensure-flexlayout-float-patch] could not parse verify JSON: ${err}`,
      )
    }
  }

  return {
    name: 'ensure-flexlayout-float-patch',
    buildStart() {
      runVerify()
    },
    configureServer() {
      runVerify()
    },
    resolveId(id) {
      if (id === 'virtual:flexlayout-patch') return '\0virtual:flexlayout-patch'
      return undefined
    },
    load(id) {
      if (id !== '\0virtual:flexlayout-patch') return undefined
      return `export const marker = ${JSON.stringify(marker)};\nexport const ok = ${marker != null};\n`
    },
  }
}

export default defineConfig({
  plugins: [react(), ensureFlexlayoutFloatPatch()],

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

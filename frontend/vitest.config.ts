import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';

const root = path.dirname(fileURLToPath(import.meta.url));

const channel = process.env.VITE_CHANNEL ?? 'direct';
if (!['direct', 'mas', 'msstore'].includes(channel)) {
  throw new Error(`Invalid VITE_CHANNEL=${channel}`);
}

export default defineConfig({
  define: {
    __CHANNEL__: JSON.stringify(channel),
  },
  resolve: {
    alias: {
      'virtual:flexlayout-patch': path.join(
        root,
        'src/test-stubs/flexlayout-patch.ts',
      ),
    },
  },
  test: {
    globals: true,
    environment: 'jsdom',
    setupFiles: ['./src/test-setup.ts'],
  },
});

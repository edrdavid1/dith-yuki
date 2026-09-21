import { describe, it, expect } from 'vitest';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

/**
 * SPEC §9.8: invoke must go through `shared/ipc` (whitelist surface).
 *
 * Baseline allowlist: audited legacy call sites that do **not** pass
 * archive-derived strings/paths into IPC. New offenders must migrate to
 * `shared/ipc` instead of growing this list.
 */

const SRC_ROOT = join(__dirname, '../../..'); // frontend/src

/** Audited exceptions — see docs/FORMAT_DECISIONS.md Stage close-out. */
const BASELINE_ALLOWLIST = new Set([
  // Hardcoded layout commands + JSON from FlexLayout model (UI state, not files).
  'components/FlexLayoutContainer.tsx',
  'contexts/LayoutContext.tsx',
  // Spike: invoke is commented out; import may remain during experiments.
  'spikes/PopoutTestWindow.tsx',
]);

function walk(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    const st = statSync(full);
    if (st.isDirectory()) {
      out.push(...walk(full));
    } else if (/\.(ts|tsx)$/.test(name)) {
      out.push(full);
    }
  }
  return out;
}

function isAllowed(rel: string): boolean {
  if (rel.startsWith('shared/ipc/') || rel === 'shared/ipc/index.ts') {
    return true;
  }
  if (rel.includes('__tests__') || rel.includes('.test.') || rel.includes('.spec.')) {
    return true;
  }
  if (BASELINE_ALLOWLIST.has(rel)) {
    return true;
  }
  return false;
}

describe('IPC_Layer guardrail', () => {
  it('forbids raw invoke / @tauri-apps/api/core outside shared/ipc (+ audited baseline)', () => {
    const offenders: string[] = [];
    for (const file of walk(SRC_ROOT)) {
      const rel = relative(SRC_ROOT, file).replace(/\\/g, '/');
      if (isAllowed(rel)) continue;

      const text = readFileSync(file, 'utf8');
      if (
        /from\s+['"]@tauri-apps\/api\/core['"]/.test(text) ||
        /\binvoke\s*\(/.test(text)
      ) {
        offenders.push(rel);
      }
    }

    expect(offenders).toEqual([]);
  });

  it('baseline allowlist entries still exist (remove when migrated)', () => {
    for (const rel of BASELINE_ALLOWLIST) {
      const full = join(SRC_ROOT, rel);
      expect(statSync(full).isFile(), `${rel} missing — drop from allowlist`).toBe(true);
    }
  });
});

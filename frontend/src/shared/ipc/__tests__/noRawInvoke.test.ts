import { describe, it, expect } from 'vitest';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

const SRC_ROOT = join(__dirname, '../../..'); // frontend/src

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
  return false;
}

describe('IPC_Layer guardrail', () => {
  it('forbids raw invoke / @tauri-apps/api/core outside shared/ipc', () => {
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
});

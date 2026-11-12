import { describe, it, expect } from 'vitest';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

/**
 * W4: Model.fromJson / `new Model` ownership is layoutStore.ts only.
 * Tests and the store itself are exempt.
 */

const SRC_ROOT = join(__dirname, '../..'); // frontend/src

const ALLOWED = new Set([
  'contexts/layoutStore.ts',
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

function isExempt(rel: string): boolean {
  if (ALLOWED.has(rel)) return true;
  if (rel.includes('__tests__') || rel.includes('.test.') || rel.includes('.spec.')) {
    return true;
  }
  return false;
}

describe('W4 Model.fromJson ownership', () => {
  it('forbids Model.fromJson / new Model outside layoutStore (+ tests)', () => {
    const offenders: string[] = [];
    const fromJson = /Model\.fromJson\s*\(/;
    const newModel = /\bnew\s+Model\s*\(/;

    for (const file of walk(SRC_ROOT)) {
      const rel = relative(SRC_ROOT, file).replace(/\\/g, '/');
      if (isExempt(rel)) continue;

      const text = readFileSync(file, 'utf8')
        // Ignore comments/docs that mention the API without calling it.
        .replace(/\/\*[\s\S]*?\*\//g, '')
        .replace(/\/\/.*$/gm, '');
      if (fromJson.test(text) || newModel.test(text)) {
        offenders.push(rel);
      }
    }

    expect(offenders).toEqual([]);
  });
});

import { describe, it, expect } from 'vitest';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

/**
 * SPEC §9.8: archive/display strings must never land in HTML sinks.
 * Mirrors shared/ipc/__tests__/noRawInvoke.test.ts.
 */

const SRC_ROOT = join(__dirname, '../..'); // frontend/src

const FORBIDDEN = [
  /dangerouslySetInnerHTML/,
  /\.innerHTML\s*=/,
  /\.outerHTML\s*=/,
  /\bv-html\b/,
];

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

function isTestFile(rel: string): boolean {
  return (
    rel.includes('__tests__') ||
    rel.includes('.test.') ||
    rel.includes('.spec.')
  );
}

describe('no dangerous HTML sinks', () => {
  it('forbids innerHTML / dangerouslySetInnerHTML outside tests', () => {
    const files = walk(SRC_ROOT);
    const violations: string[] = [];
    for (const file of files) {
      const rel = relative(SRC_ROOT, file).replace(/\\/g, '/');
      if (isTestFile(rel)) continue;
      const src = readFileSync(file, 'utf8');
      for (const re of FORBIDDEN) {
        if (re.test(src)) {
          violations.push(`${rel} matches ${re}`);
        }
      }
    }
    expect(violations).toEqual([]);
  });
});

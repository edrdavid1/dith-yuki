import { describe, it, expect } from 'vitest';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

/**
 * W7/W10: product UI must not call deprecated per-side / flexlayout_state saves.
 * SoT is save_shell_layout / load_shell_layout via layoutStore.
 * W10: those commands are also unregistered from Tauri generate_handler!.
 */

const SRC_ROOT = join(__dirname, '../..');
const MAIN_RS = join(__dirname, '../../../../src-tauri/src/main.rs');

const FORBIDDEN =
  /\b(?:save_layout_left|save_layout_right|save_layout_center|load_layout_left|load_layout_right|load_layout_center|save_layout|load_layout|reset_layout_to_default)\b/;

function walk(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    const st = statSync(full);
    if (st.isDirectory()) {
      if (name === '__tests__' || name === 'node_modules') continue;
      out.push(...walk(full));
    } else if (/\.(ts|tsx)$/.test(name)) {
      out.push(full);
    }
  }
  return out;
}

describe('W7/W10 legacy layout IPC ban', () => {
  it('forbids deprecated layout save/load command names in product src', () => {
    const offenders: string[] = [];
    for (const file of walk(SRC_ROOT)) {
      const rel = relative(SRC_ROOT, file).replace(/\\/g, '/');
      if (rel.includes('.test.') || rel.includes('__tests__')) continue;
      if (rel.startsWith('spikes/')) continue;

      const text = readFileSync(file, 'utf8')
        .replace(/\/\*[\s\S]*?\*\//g, '')
        .replace(/\/\/.*$/gm, '');
      if (FORBIDDEN.test(text)) offenders.push(rel);
    }
    expect(offenders).toEqual([]);
  });

  it('does not register deprecated layout commands in Tauri main.rs', () => {
    const text = readFileSync(MAIN_RS, 'utf8');
    for (const name of [
      'save_layout',
      'load_layout',
      'reset_layout_to_default',
      'load_layout_left',
      'save_layout_left',
      'load_layout_right',
      'save_layout_right',
      'load_layout_center',
      'save_layout_center',
    ]) {
      expect(text).not.toMatch(new RegExp(`flexlayout::${name}\\b`));
    }
    expect(text).toMatch(/flexlayout::load_shell_layout\b/);
    expect(text).toMatch(/flexlayout::save_shell_layout\b/);
  });
});

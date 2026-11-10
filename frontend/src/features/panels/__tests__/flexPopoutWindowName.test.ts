import { describe, it, expect } from 'vitest';
import { Model, TabNode } from 'flexlayout-react';
import type { IJsonModel } from 'flexlayout-react';
import { flexPopoutWindowName } from '../flexPopoutWindowName';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

describe('flexPopoutWindowName', () => {
  it('strips FlexLayout #uuid so window.open names stay distinct', () => {
    const a = flexPopoutWindowName('#aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee');
    const b = flexPopoutWindowName('#ffffffff-0000-1111-2222-333333333333');
    expect(a).toBe('flex-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee');
    expect(b).toBe('flex-ffffffff-0000-1111-2222-333333333333');
    expect(a).not.toBe(b);
    // Truncating at `#` (WKWebView) would collapse both to '' — must not happen.
    expect(a.includes('#')).toBe(false);
    expect(b.includes('#')).toBe(false);
  });

  it('yields distinct names for real FlexLayout tab ids', () => {
    const json: IJsonModel = {
      global: {},
      borders: [],
      layout: {
        type: 'row',
        weight: 100,
        children: [
          {
            type: 'tabset',
            weight: 50,
            children: [{ type: 'tab', name: 'Effect', component: 'effect' }],
          },
          {
            type: 'tabset',
            weight: 50,
            children: [{ type: 'tab', name: 'Color Lab', component: 'colorlab' }],
          },
        ],
      },
    };
    const model = Model.fromJson(json);
    const ids: string[] = [];
    model.visitNodes((node) => {
      if (node.getType() === TabNode.TYPE) ids.push((node as TabNode).getId());
    });
    expect(ids).toHaveLength(2);
    expect(ids[0]!.startsWith('#')).toBe(true);

    const names = ids.map(flexPopoutWindowName);
    expect(new Set(names).size).toBe(2);
    for (const name of names) {
      expect(name.startsWith('flex-')).toBe(true);
      expect(name.includes('#')).toBe(false);
    }
  });

  it('keeps the FloatingWindow patch calling flexPopoutWindowName(id)', () => {
    const patchPath = resolve(
      __dirname,
      '../../../patches/flexlayoutFloatingWindow.cjs',
    );
    const src = readFileSync(patchPath, 'utf8');
    expect(src).toContain('flexPopoutWindowName(id)');
    expect(src).toMatch(/window\.open\(\s*url,\s*flexPopoutWindowName\(id\)/);
  });

  it('installed FloatingWindow.js is patched (not stock window.open(url, id))', () => {
    const installed = resolve(
      __dirname,
      '../../../../node_modules/flexlayout-react/lib/view/FloatingWindow.js',
    );
    try {
      const src = readFileSync(installed, 'utf8');
      expect(src).toMatch(/window\.open\(\s*url,\s*flexPopoutWindowName\(id\)/);
      expect(src).not.toMatch(/window\.open\(\s*url,\s*id\s*,/);
    } catch {
      // Package absent in some CI slices — skip.
    }
  });
});

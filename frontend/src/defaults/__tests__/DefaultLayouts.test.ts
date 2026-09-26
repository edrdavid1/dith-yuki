import { describe, it, expect } from 'vitest';
import {
  getDefaultFlexLayoutJson,
  isValidFlexLayoutJson,
  parseFlexLayoutJson,
} from '../DefaultLayouts';

describe('DefaultLayouts (B5)', () => {
  it('center default hosts Preview as the only tab', () => {
    const model = getDefaultFlexLayoutJson('center');
    expect(isValidFlexLayoutJson(model)).toBe(true);
    const tabset = model.layout.children[0] as {
      type: string;
      children: Array<{ name: string; component: string }>;
    };
    expect(tabset.type).toBe('tabset');
    expect(tabset.children).toHaveLength(1);
    expect(tabset.children[0]).toMatchObject({ name: 'Preview', component: 'preview' });
  });

  it('left default hosts Layers; right hosts Effect + Color Lab', () => {
    const left = getDefaultFlexLayoutJson('left');
    const right = getDefaultFlexLayoutJson('right');
    const leftTabs = (left.layout.children[0] as { children: Array<{ component: string }> })
      .children;
    const rightTabs = (right.layout.children[0] as { children: Array<{ component: string }> })
      .children;
    expect(leftTabs.map((t) => t.component)).toEqual(['layers']);
    expect(rightTabs.map((t) => t.component)).toEqual(['effect', 'colorlab']);
  });

  it('parseFlexLayoutJson falls back cleanly on garbage', () => {
    expect(parseFlexLayoutJson('not-json')).toBeNull();
  });
});

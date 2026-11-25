import { describe, expect, it } from 'vitest';
import { Model } from 'flexlayout-react';
import { getDefaultFlexLayoutJson } from '../../../defaults/DefaultLayouts';
import { preferredSideForGamedevPanel } from '../gamedevPanelPlacement';

describe('preferredSideForGamedevPanel', () => {
  it('prefers the side with fewer docked panels (default: left)', () => {
    const left = Model.fromJson(getDefaultFlexLayoutJson('left'));
    const right = Model.fromJson(getDefaultFlexLayoutJson('right'));
    // left = layers (1); right = effect + colorlab (2)
    expect(preferredSideForGamedevPanel(left, right)).toBe('left');
  });

  it('prefers right when right is less crowded', () => {
    const left = Model.fromJson({
      ...getDefaultFlexLayoutJson('left'),
      layout: {
        type: 'row',
        weight: 100,
        children: [
          {
            type: 'tabset',
            weight: 100,
            children: [
              { type: 'tab', name: 'Layers', component: 'layers' },
              { type: 'tab', name: 'Effect', component: 'effect' },
            ],
          },
        ],
      },
    });
    const right = Model.fromJson({
      ...getDefaultFlexLayoutJson('right'),
      layout: {
        type: 'row',
        weight: 100,
        children: [
          {
            type: 'tabset',
            weight: 100,
            children: [{ type: 'tab', name: 'Color Lab', component: 'colorlab' }],
          },
        ],
      },
    });
    expect(preferredSideForGamedevPanel(left, right)).toBe('right');
  });
});

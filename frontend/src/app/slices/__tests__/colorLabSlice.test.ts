import { describe, it, expect } from 'vitest';
import {
  addColor,
  deleteColor,
  resetDraft,
  setColors,
  setSelectedColorIndex,
} from '../colorLabSlice';
import { createColorEntry } from '../../../features/color-lab/types';
import { createTestStore } from '../../__tests__/testStore';

describe('colorLabSlice selectedColorIndex', () => {
  it('selects a swatch index', () => {
    const store = createTestStore();
    store.dispatch(addColor('#112233'));
    store.dispatch(addColor('#445566'));
    store.dispatch(setSelectedColorIndex(1));
    expect(store.getState().colorLab.selectedColorIndex).toBe(1);
  });

  it('ignores out-of-range selection', () => {
    const store = createTestStore();
    store.dispatch(addColor('#112233'));
    store.dispatch(setSelectedColorIndex(4));
    expect(store.getState().colorLab.selectedColorIndex).toBeNull();
  });

  it('clears selection when the selected color is deleted', () => {
    const store = createTestStore();
    store.dispatch(setColors([createColorEntry('#111111'), createColorEntry('#222222')]));
    store.dispatch(setSelectedColorIndex(1));
    store.dispatch(deleteColor(1));
    expect(store.getState().colorLab.selectedColorIndex).toBeNull();
    expect(store.getState().colorLab.colors).toHaveLength(1);
  });

  it('shifts selection down when an earlier color is deleted', () => {
    const store = createTestStore();
    store.dispatch(
      setColors([
        createColorEntry('#111111'),
        createColorEntry('#222222'),
        createColorEntry('#333333'),
      ])
    );
    store.dispatch(setSelectedColorIndex(2));
    store.dispatch(deleteColor(0));
    expect(store.getState().colorLab.selectedColorIndex).toBe(1);
  });

  it('resetDraft clears selection', () => {
    const store = createTestStore();
    store.dispatch(addColor('#ffffff'));
    store.dispatch(setSelectedColorIndex(0));
    store.dispatch(resetDraft());
    expect(store.getState().colorLab.selectedColorIndex).toBeNull();
  });
});

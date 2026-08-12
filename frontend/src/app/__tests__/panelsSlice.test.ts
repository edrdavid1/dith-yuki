import { describe, it, expect } from 'vitest';
import panelsReducer, {
  applyPanelEvent,
  applySnapshot,
  selectVisibleDocked,
  type PanelsState,
} from '../slices/panelsSlice';
import type { PanelStateSnapshot } from '../../types/panels';

function emptyState(): PanelsState {
  return { entities: [], leftOrder: [], rightOrder: [], error: null };
}

describe('panelsSlice dual orders', () => {
  const snapshot: PanelStateSnapshot = {
    panels: [
      {
        id: 'layers',
        docked: true,
        visible: true,
        window_label: null,
        saved_bounds: null,
        dock_side: 'left',
      },
      {
        id: 'effect',
        docked: true,
        visible: true,
        window_label: null,
        saved_bounds: null,
        dock_side: 'right',
      },
      {
        id: 'colorlab',
        docked: true,
        visible: false,
        window_label: null,
        saved_bounds: null,
        dock_side: 'right',
      },
      {
        id: 'preview',
        docked: true,
        visible: true,
        window_label: null,
        saved_bounds: null,
        dock_side: null,
      },
    ],
    left_order: ['layers'],
    right_order: ['effect', 'colorlab'],
  };

  it('applySnapshot stores dual orders and dock_side', () => {
    const state = emptyState();
    applySnapshot(state, snapshot);
    expect(state.leftOrder).toEqual(['layers']);
    expect(state.rightOrder).toEqual(['effect', 'colorlab']);
    expect(state.entities.find((p) => p.id === 'layers')?.dock_side).toBe('left');
  });

  it('applyPanelEvent applies full snapshot', () => {
    const next = panelsReducer(emptyState(), applyPanelEvent(snapshot));
    expect(next.leftOrder).toEqual(['layers']);
    expect(next.rightOrder).toEqual(['effect', 'colorlab']);
  });

  it('selectVisibleDocked filters hidden and floating-only', () => {
    const state = emptyState();
    applySnapshot(state, snapshot);
    expect(selectVisibleDocked(state.entities, state.leftOrder, state.rightOrder, 'left')).toEqual([
      'layers',
    ]);
    expect(selectVisibleDocked(state.entities, state.leftOrder, state.rightOrder, 'right')).toEqual([
      'effect',
    ]);
  });

  it('legacy panels-only event keeps previous orders', () => {
    const seeded = panelsReducer(emptyState(), applyPanelEvent(snapshot));
    const next = panelsReducer(
      seeded,
      applyPanelEvent([
        {
          id: 'layers',
          docked: false,
          visible: true,
          window_label: 'panel-layers',
          saved_bounds: null,
          dock_side: null,
        },
      ])
    );
    expect(next.entities).toHaveLength(1);
    expect(next.leftOrder).toEqual(['layers']);
    expect(next.rightOrder).toEqual(['effect', 'colorlab']);
  });
});

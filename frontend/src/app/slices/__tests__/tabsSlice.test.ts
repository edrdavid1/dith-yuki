import { describe, it, expect } from 'vitest';
import { createTestStore } from '../../__tests__/testStore';
import tabsReducer, { patchTabDirty, tabsChanged } from '../tabsSlice';

describe('tabsSlice.patchTabDirty', () => {
  it('updates dirty for a known tab and leaves others alone', () => {
    const store = createTestStore({
      tabs: {
        tabs: [
          { id: 1, title: 'a', dirty: false, path: '/a.dyproj' },
          { id: 2, title: 'b', dirty: false, path: '/b.dyproj' },
        ],
        activeId: 2,
      },
    });

    store.dispatch(patchTabDirty({ id: 1, dirty: true }));

    const tabs = store.getState().tabs.tabs;
    expect(tabs.find((t) => t.id === 1)?.dirty).toBe(true);
    expect(tabs.find((t) => t.id === 2)?.dirty).toBe(false);
  });

  it('ignores unknown tab ids', () => {
    const before = tabsReducer(
      {
        tabs: [{ id: 1, title: 'a', dirty: false, path: null }],
        activeId: 1,
      },
      patchTabDirty({ id: 99, dirty: true })
    );
    expect(before.tabs).toEqual([{ id: 1, title: 'a', dirty: false, path: null }]);
  });

  it('tabsChanged replaces live dirty patches', () => {
    const store = createTestStore({
      tabs: {
        tabs: [
          { id: 1, title: 'a', dirty: true, path: '/a.dyproj' },
          { id: 2, title: 'b', dirty: true, path: '/b.dyproj' },
        ],
        activeId: 1,
      },
    });

    store.dispatch(
      tabsChanged({
        tabs: [
          { id: 1, title: 'a', dirty: false, path: '/a.dyproj' },
          { id: 2, title: 'b', dirty: true, path: '/b.dyproj' },
        ],
        active_id: 1,
      })
    );

    const tabs = store.getState().tabs.tabs;
    expect(tabs.find((t) => t.id === 1)?.dirty).toBe(false);
    expect(tabs.find((t) => t.id === 2)?.dirty).toBe(true);
  });
});

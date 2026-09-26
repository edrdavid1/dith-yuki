import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createTestStore } from '../__tests__/testStore';
import { startEngineEventBridge } from '../listeners';
import type { DirtyDto } from '../../shared/ipc/undo';

const dirtyHandlers: Array<(event: { payload: DirtyDto }) => void> = [];

vi.mock('../../shared/ipc', async () => {
  const actual = await vi.importActual<typeof import('../../shared/ipc')>('../../shared/ipc');
  return {
    ...actual,
    onDirtyChanged: vi.fn(async (handler: (event: { payload: DirtyDto }) => void) => {
      dirtyHandlers.push(handler);
      return () => {
        const i = dirtyHandlers.indexOf(handler);
        if (i >= 0) dirtyHandlers.splice(i, 1);
      };
    }),
    onDocumentChanged: vi.fn(async () => () => {}),
    onUndoStateChanged: vi.fn(async () => () => {}),
    onSelectionChanged: vi.fn(async () => () => {}),
    onColorLabDraftChanged: vi.fn(async () => () => {}),
    onPaletteBindingChanged: vi.fn(async () => () => {}),
    isDocumentDirty: vi.fn(async () => false),
  };
});

describe('startEngineEventBridge dirty-changed', () => {
  beforeEach(() => {
    dirtyHandlers.length = 0;
  });

  it('patches inactive tab dirty without touching document.dirty', async () => {
    const store = createTestStore({
      document: {
        docId: 2,
        width: 10,
        height: 10,
        hasDocument: true,
        hydrated: true,
        loading: false,
        saving: false,
        notification: null,
        error: null,
        layerId: 1,
        projectPath: null,
        sourcePath: null,
        dirty: false,
        documentEpoch: 0,
      },
      tabs: {
        tabs: [
          { id: 1, title: 'a', dirty: false, path: '/a.dyproj' },
          { id: 2, title: 'b', dirty: false, path: '/b.dyproj' },
        ],
        activeId: 2,
      },
    });

    const stop = startEngineEventBridge(store as never);
    // Allow async listen setup
    await Promise.resolve();
    expect(dirtyHandlers.length).toBeGreaterThan(0);

    dirtyHandlers[0]!({ payload: { dirty: true, doc_id: 1 } });

    expect(store.getState().tabs.tabs.find((t) => t.id === 1)?.dirty).toBe(true);
    expect(store.getState().tabs.tabs.find((t) => t.id === 2)?.dirty).toBe(false);
    expect(store.getState().document.dirty).toBe(false);

    stop();
  });

  it('patches active tab and document.dirty together', async () => {
    const store = createTestStore({
      document: {
        docId: 2,
        width: 10,
        height: 10,
        hasDocument: true,
        hydrated: true,
        loading: false,
        saving: false,
        notification: null,
        error: null,
        layerId: 1,
        projectPath: null,
        sourcePath: null,
        dirty: false,
        documentEpoch: 0,
      },
      tabs: {
        tabs: [
          { id: 1, title: 'a', dirty: false, path: '/a.dyproj' },
          { id: 2, title: 'b', dirty: false, path: '/b.dyproj' },
        ],
        activeId: 2,
      },
    });

    const stop = startEngineEventBridge(store as never);
    await Promise.resolve();

    dirtyHandlers[0]!({ payload: { dirty: true, doc_id: 2 } });

    expect(store.getState().tabs.tabs.find((t) => t.id === 2)?.dirty).toBe(true);
    expect(store.getState().document.dirty).toBe(true);

    stop();
  });
});

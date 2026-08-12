import { describe, it, expect, vi, beforeEach } from 'vitest';
import { applyRemote, setSelection, fetchSelection } from '../slices/selectionSlice';
import { createTestStore } from './testStore';

vi.mock('../../shared/ipc', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../shared/ipc')>();
  return {
    ...actual,
    getSelection: vi.fn(),
    setSelection: vi.fn(),
  };
});

import { getSelection, setSelection as setSelectionIPC } from '../../shared/ipc';

const mockGetSelection = vi.mocked(getSelection);
const mockSetSelectionIPC = vi.mocked(setSelectionIPC);

describe('selection slice sync', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('fetchSelection hydrates store from IPC', async () => {
    mockGetSelection.mockResolvedValue({
      selected_layer_id: 2,
      selected_filter_id: 'f-1',
    });

    const store = createTestStore();
    await store.dispatch(fetchSelection());

    expect(store.getState().selection.layerId).toBe(2);
    expect(store.getState().selection.filterId).toBe('f-1');
    expect(mockGetSelection).toHaveBeenCalled();
  });

  it('setSelection updates local state and calls IPC', async () => {
    mockSetSelectionIPC.mockResolvedValue(undefined);
    const store = createTestStore();

    await store.dispatch(setSelection({ layerId: 1, filterId: 'abc' }));

    expect(store.getState().selection.layerId).toBe(1);
    expect(store.getState().selection.filterId).toBe('abc');
    expect(mockSetSelectionIPC).toHaveBeenCalledWith(1, 'abc');
  });

  it('applyRemote updates selection when not suppressed', () => {
    const store = createTestStore();
    store.dispatch(
      applyRemote({ layerId: 5, filterId: 'remote-f' })
    );
    expect(store.getState().selection.layerId).toBe(5);
    expect(store.getState().selection.filterId).toBe('remote-f');
  });

  it('applyRemote is ignored while suppressRemote is true (local echo)', async () => {
    mockSetSelectionIPC.mockResolvedValue(undefined);
    const store = createTestStore();

    // Local set leaves suppressRemote=true until timeout
    const pending = store.dispatch(setSelection({ layerId: 1, filterId: 'local' }));
    store.dispatch(applyRemote({ layerId: 99, filterId: 'echo' }));

    expect(store.getState().selection.layerId).toBe(1);
    expect(store.getState().selection.filterId).toBe('local');

    await pending;
  });
});

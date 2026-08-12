import React from 'react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, waitFor, act } from '@testing-library/react';
import { useSelectionState } from '../useSelectionState';
import { StoreProvider, createTestStore } from '../../app/__tests__/testStore';
import { applyRemote } from '../../app/slices/selectionSlice';

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

function makeWrapper(store = createTestStore()) {
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return <StoreProvider store={store}>{children}</StoreProvider>;
  };
}

describe('useSelectionState', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetSelection.mockResolvedValue({
      selected_layer_id: null,
      selected_filter_id: null,
    });
    mockSetSelectionIPC.mockResolvedValue(undefined);
  });

  it('fetches initial selection on mount', async () => {
    mockGetSelection.mockResolvedValueOnce({
      selected_layer_id: 1,
      selected_filter_id: 'filter-abc',
    });

    const { result } = renderHook(() => useSelectionState(), {
      wrapper: makeWrapper(),
    });

    await waitFor(() => {
      expect(result.current.selectedLayerId).toBe(1);
    });

    expect(result.current.selectedFilterId).toBe('filter-abc');
    expect(result.current.error).toBeNull();
    expect(mockGetSelection).toHaveBeenCalled();
  });

  it('setSelection broadcasts via IPC and updates store', async () => {
    // Prevent mount fetch from racing and overwriting local selection
    mockGetSelection.mockImplementation(() => new Promise(() => {}));

    const { result } = renderHook(() => useSelectionState(), {
      wrapper: makeWrapper(),
    });

    await act(async () => {
      result.current.setSelection(3, 'f-9');
    });

    await waitFor(() => {
      expect(result.current.selectedLayerId).toBe(3);
      expect(result.current.selectedFilterId).toBe('f-9');
    });
    expect(mockSetSelectionIPC).toHaveBeenCalledWith(3, 'f-9');
  });

  it('applies remote selection via store action', async () => {
    mockGetSelection.mockImplementation(() => new Promise(() => {}));
    const store = createTestStore();
    const { result } = renderHook(() => useSelectionState(), {
      wrapper: makeWrapper(store),
    });

    await act(async () => {
      store.dispatch(applyRemote({ layerId: 7, filterId: 'remote' }));
    });

    expect(result.current.selectedLayerId).toBe(7);
    expect(result.current.selectedFilterId).toBe('remote');
  });
});

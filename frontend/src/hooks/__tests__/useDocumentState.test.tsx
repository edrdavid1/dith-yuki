import React from 'react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, waitFor } from '@testing-library/react';
import { useDocumentState } from '../useDocumentState';
import { StoreProvider, createTestStore } from '../../app/__tests__/testStore';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
  emit: vi.fn(),
  emitTo: vi.fn(),
}));

import { invoke } from '@tauri-apps/api/core';

const mockInvoke = vi.mocked(invoke);

function makeSnapshot(layers: unknown[] = [{ id: 1 }]) {
  return {
    snapshot: {
      id: 42,
      width: 800,
      height: 600,
      layers,
    },
  };
}

function wrapper({ children }: { children: React.ReactNode }) {
  return <StoreProvider store={createTestStore()}>{children}</StoreProvider>;
}

describe('useDocumentState', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('loads document meta from snapshot', async () => {
    mockInvoke.mockResolvedValue(makeSnapshot());

    const { result } = renderHook(() => useDocumentState(), { wrapper });

    await waitFor(() => {
      expect(result.current.docId).toBe(42);
    });
    expect(result.current.width).toBe(800);
    expect(result.current.height).toBe(600);
    expect(result.current.hasDocument).toBe(true);
    expect(result.current.error).toBeNull();
  });

  it('sets error when snapshot fails', async () => {
    mockInvoke.mockRejectedValue('no doc');

    const { result } = renderHook(() => useDocumentState(), { wrapper });

    await waitFor(() => {
      expect(result.current.error).toBe('no doc');
    });
    expect(result.current.hasDocument).toBe(false);
  });
});

import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import React, { StrictMode, useEffect } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react';
import { createLayoutStoreForTests, type LayoutStore } from '../layoutStore';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockRejectedValue(new Error('no tauri in test')),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockImplementation(async () => vi.fn()),
}));

vi.mock('../../features/panels/closeAllFlexPopouts', () => ({
  closeAllFlexPopouts: vi.fn().mockResolvedValue(undefined),
}));

function Probe({ store }: { store: LayoutStore }) {
  useEffect(() => store.ensureInitialized(), [store]);
  return null;
}

describe('LayoutStore StrictMode', () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    container.remove();
    vi.clearAllMocks();
  });

  it('double mount under StrictMode keeps one dock-listener refcount after settle', async () => {
    const { listen } = await import('@tauri-apps/api/event');
    const store = createLayoutStoreForTests();

    await act(async () => {
      root.render(
        <StrictMode>
          <Probe store={store} />
        </StrictMode>,
      );
    });

    // Allow listen() promise to resolve.
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    // StrictMode: mount → unmount → remount. Refcount must settle at 1.
    expect(store.getDockListenerRefCountForTests()).toBe(1);
    // listen registered once per attach cycle start; remount after cleanup may
    // call listen again — never more live unlisten handles than refcount.
    expect(vi.mocked(listen).mock.calls.length).toBeGreaterThanOrEqual(1);
    expect(store.getVersion()).toBeGreaterThanOrEqual(0);
  });

  it('two providers share one store instance and do not stack listeners beyond refcount', async () => {
    const store = createLayoutStoreForTests();

    await act(async () => {
      root.render(
        <>
          <Probe store={store} />
          <Probe store={store} />
        </>,
      );
    });

    await act(async () => {
      await Promise.resolve();
    });

    expect(store.getDockListenerRefCountForTests()).toBe(2);

    await act(async () => {
      root.render(<Probe store={store} />);
    });

    await act(async () => {
      await Promise.resolve();
    });

    expect(store.getDockListenerRefCountForTests()).toBe(1);
  });
});

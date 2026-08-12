import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook } from '@testing-library/react';
import { useCloseRequested } from '../useCloseRequested';

// Mock @tauri-apps/api/window
const mockDestroy = vi.fn().mockResolvedValue(undefined);
const mockUnlisten = vi.fn();
let closeRequestedHandler: ((event: any) => void) | null = null;

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    onCloseRequested: vi.fn((handler: (event: any) => void) => {
      closeRequestedHandler = handler;
      return Promise.resolve(mockUnlisten);
    }),
    destroy: mockDestroy,
  }),
}));

// Mock panelCommands
const mockDockPanel = vi.fn().mockResolvedValue(undefined);
vi.mock('../../ipc/panelCommands', () => ({
  dockPanel: (...args: any[]) => mockDockPanel(...args),
}));

describe('useCloseRequested', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    closeRequestedHandler = null;
  });

  it('registers onCloseRequested listener on mount', async () => {
    renderHook(() => useCloseRequested('layers'));

    // Wait for the async listener setup
    await vi.waitFor(() => {
      expect(closeRequestedHandler).not.toBeNull();
    });
  });

  it('calls event.preventDefault when close is requested', async () => {
    renderHook(() => useCloseRequested('layers'));

    await vi.waitFor(() => {
      expect(closeRequestedHandler).not.toBeNull();
    });

    const mockEvent = { preventDefault: vi.fn() };
    await closeRequestedHandler!(mockEvent);

    expect(mockEvent.preventDefault).toHaveBeenCalled();
  });

  it('calls dockPanel with the panelId then destroys the window', async () => {
    renderHook(() => useCloseRequested('layers'));

    await vi.waitFor(() => {
      expect(closeRequestedHandler).not.toBeNull();
    });

    const mockEvent = { preventDefault: vi.fn() };
    await closeRequestedHandler!(mockEvent);

    expect(mockDockPanel).toHaveBeenCalledWith('layers');
    expect(mockDestroy).toHaveBeenCalled();
  });

  it('destroys window even if dockPanel fails', async () => {
    mockDockPanel.mockRejectedValueOnce(new Error('IPC failed'));
    const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    renderHook(() => useCloseRequested('effects'));

    await vi.waitFor(() => {
      expect(closeRequestedHandler).not.toBeNull();
    });

    const mockEvent = { preventDefault: vi.fn() };
    await closeRequestedHandler!(mockEvent);

    expect(mockDockPanel).toHaveBeenCalledWith('effects');
    expect(consoleSpy).toHaveBeenCalledWith(
      expect.stringContaining('dock_panel failed for "effects"'),
      expect.any(Error),
    );
    expect(mockDestroy).toHaveBeenCalled();

    consoleSpy.mockRestore();
  });

  it('cleans up listener on unmount', async () => {
    const { unmount } = renderHook(() => useCloseRequested('layers'));

    await vi.waitFor(() => {
      expect(closeRequestedHandler).not.toBeNull();
    });

    // Allow the .then() callback to assign unlistenFn
    await new Promise((r) => setTimeout(r, 0));

    unmount();

    expect(mockUnlisten).toHaveBeenCalled();
  });
});

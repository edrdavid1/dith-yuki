import { beforeEach, describe, expect, it, vi } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import type { ReactNode } from 'react';
import { ShellProvider } from '../../app/shell/ShellContext';

vi.mock('tauri-macos-haptics-api', () => ({
  isSupported: vi.fn(),
  perform: vi.fn(),
  HapticFeedbackPattern: { Alignment: 0, LevelChange: 1, Generic: 2 },
  PerformanceTime: { Default: 0, Now: 1, DrawCompleted: 2 },
  HapticError: class HapticError extends Error {},
}));

import { isSupported, perform } from 'tauri-macos-haptics-api';
import { useHapticFeedback } from '../useHapticFeedback';

const mockIsSupported = vi.mocked(isSupported);
const mockPerform = vi.mocked(perform);

function wrapper({ children }: { children: ReactNode }) {
  return <ShellProvider>{children}</ShellProvider>;
}

async function flushHaptic() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe('useHapticFeedback', () => {
  beforeEach(() => {
    mockIsSupported.mockReset();
    mockPerform.mockReset();
    mockIsSupported.mockResolvedValue(true);
    mockPerform.mockResolvedValue(undefined);
    localStorage.clear();
  });

  it('calls perform when supported (fire-and-forget)', async () => {
    const { result } = renderHook(() => useHapticFeedback(), { wrapper });
    act(() => {
      result.current.triggerHaptic('levelChange');
    });
    await flushHaptic();
    expect(mockPerform).toHaveBeenCalledWith(
      expect.anything(),
      1 // PerformanceTime.Now in the mock enum
    );
  });

  it('no-ops when isSupported is false', async () => {
    mockIsSupported.mockResolvedValue(false);
    const { result } = renderHook(() => useHapticFeedback(), { wrapper });
    act(() => {
      result.current.triggerHaptic('levelChange');
    });
    await flushHaptic();
    expect(mockPerform).not.toHaveBeenCalled();
  });

  it('no-ops when preference is off', async () => {
    localStorage.setItem(
      'dither.shellPrefs',
      JSON.stringify({
        version: 2,
        hapticFeedback: false,
        leftSidebar: { width: 332, collapsed: false },
        rightSidebar: { width: 332, collapsed: false },
        leftSplitRatio: 0.5,
        rightSplitRatio: 0.5,
        effectPanelRatio: 0.5,
        autoExtractPalettes: true,
        previewBackground: 'gray',
        welcomeBackground: 'artwork',
        hideRecentList: false,
        appIconId: 'default',
      })
    );
    const { result } = renderHook(() => useHapticFeedback(), { wrapper });
    expect(result.current.prefEnabled).toBe(false);
    act(() => {
      result.current.triggerHaptic('levelChange');
    });
    await flushHaptic();
    expect(mockPerform).not.toHaveBeenCalled();
  });

  it('does not serialize rapid calls behind await perform', async () => {
    let release!: () => void;
    mockPerform.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        })
    );
    const { result } = renderHook(() => useHapticFeedback(), { wrapper });
    act(() => {
      result.current.triggerHaptic('levelChange');
      result.current.triggerHaptic('levelChange');
      result.current.triggerHaptic('levelChange');
    });
    await flushHaptic();
    expect(mockPerform).toHaveBeenCalledTimes(3);
    release();
  });
});

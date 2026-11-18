import { beforeEach, describe, expect, it, vi } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import type { ReactNode } from 'react';
import { ShellProvider } from '../../app/shell/ShellContext';

const { mockPerformSliderHaptic } = vi.hoisted(() => ({
  mockPerformSliderHaptic: vi.fn(),
}));

vi.mock('tauri-macos-haptics-api', () => ({
  isSupported: vi.fn(),
  perform: vi.fn(),
  HapticFeedbackPattern: { Alignment: 0, LevelChange: 1, Generic: 2 },
  PerformanceTime: { Default: 0, Now: 1, DrawCompleted: 2 },
  HapticError: class HapticError extends Error {},
}));

vi.mock('../../shared/ipc', async () => {
  const actual = await vi.importActual<typeof import('../../shared/ipc')>('../../shared/ipc');
  return {
    ...actual,
    performSliderHaptic: mockPerformSliderHaptic,
  };
});

import { perform } from 'tauri-macos-haptics-api';
import { useHapticFeedback } from '../useHapticFeedback';

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
    mockPerform.mockReset();
    mockPerformSliderHaptic.mockReset();
    mockPerform.mockResolvedValue(undefined);
    mockPerformSliderHaptic.mockResolvedValue(undefined);
    localStorage.clear();
  });

  it('fires main-thread command and plugin perform', async () => {
    const { result } = renderHook(() => useHapticFeedback(), { wrapper });
    act(() => {
      result.current.triggerHaptic('alignment');
    });
    await flushHaptic();
    expect(mockPerformSliderHaptic).toHaveBeenCalledWith(0);
    expect(mockPerform).toHaveBeenCalledWith(
      expect.anything(),
      1 // PerformanceTime.Now
    );
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
      result.current.triggerHaptic('alignment');
    });
    await flushHaptic();
    expect(mockPerformSliderHaptic).not.toHaveBeenCalled();
    expect(mockPerform).not.toHaveBeenCalled();
  });

  it('does not serialize rapid calls', async () => {
    let release!: () => void;
    mockPerformSliderHaptic.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        })
    );
    const { result } = renderHook(() => useHapticFeedback(), { wrapper });
    act(() => {
      result.current.triggerHaptic('alignment');
      result.current.triggerHaptic('alignment');
      result.current.triggerHaptic('alignment');
    });
    await flushHaptic();
    expect(mockPerformSliderHaptic).toHaveBeenCalledTimes(3);
    release();
  });
});

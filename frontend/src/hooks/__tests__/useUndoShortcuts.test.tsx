import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import NumberInput from '../../components/common/NumberInput';
import { useUndoShortcuts } from '../useUndoShortcuts';
import { StoreProvider, createTestStore } from '../../app/__tests__/testStore';
import type { ReactNode } from 'react';

vi.mock('../../shared/ipc', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../shared/ipc')>();
  return {
    ...actual,
    undo: vi.fn(),
    redo: vi.fn(),
  };
});

import { undo as undoIPC, redo as redoIPC } from '../../shared/ipc';

const mockUndo = vi.mocked(undoIPC);
const mockRedo = vi.mocked(redoIPC);

function Harness() {
  useUndoShortcuts();
  return (
    <NumberInput
      label="Levels"
      value={4}
      min={2}
      max={32}
      step={1}
      onChange={() => {}}
    />
  );
}

function renderHarness(opts: { canUndo?: boolean; canRedo?: boolean; hasDocument?: boolean }) {
  const store = createTestStore({
    document: {
      docId: opts.hasDocument === false ? null : 1,
      width: 8,
      height: 8,
      hasDocument: opts.hasDocument !== false,
      loading: false,
      notification: null,
      error: null,
      layerId: 1,
      projectPath: null,
    },
    undo: {
      canUndo: opts.canUndo ?? true,
      canRedo: opts.canRedo ?? false,
    },
  });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <StoreProvider store={store}>{children}</StoreProvider>
  );
  return { store, ...render(<Harness />, { wrapper }) };
}

describe('useUndoShortcuts', () => {
  beforeEach(() => {
    mockUndo.mockReset();
    mockRedo.mockReset();
    mockUndo.mockResolvedValue({ can_undo: false, can_redo: true });
    mockRedo.mockResolvedValue({ can_undo: true, can_redo: false });
  });

  it('invokes undo when NumberInput is focused and canUndo', () => {
    renderHarness({ canUndo: true });
    const input = screen.getByLabelText('Levels');
    input.focus();
    expect(input).toHaveFocus();

    fireEvent.keyDown(window, { key: 'z', metaKey: true });
    expect(mockUndo).toHaveBeenCalledTimes(1);
  });

  it('invokes undo with Ctrl+Z', () => {
    renderHarness({ canUndo: true });
    screen.getByLabelText('Levels').focus();
    fireEvent.keyDown(window, { key: 'z', ctrlKey: true });
    expect(mockUndo).toHaveBeenCalledTimes(1);
  });

  it('does not preventDefault native text undo when !canUndo', () => {
    renderHarness({ canUndo: false });
    screen.getByLabelText('Levels').focus();
    const event = new KeyboardEvent('keydown', {
      key: 'z',
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });
    const prevented = !window.dispatchEvent(event);
    expect(mockUndo).not.toHaveBeenCalled();
    expect(prevented).toBe(false);
  });

  it('is a no-op without a document', () => {
    renderHarness({ hasDocument: false, canUndo: true });
    fireEvent.keyDown(window, { key: 'z', metaKey: true });
    expect(mockUndo).not.toHaveBeenCalled();
  });

  it('invokes redo on shift+mod+z when canRedo', () => {
    renderHarness({ canUndo: false, canRedo: true });
    screen.getByLabelText('Levels').focus();
    fireEvent.keyDown(window, { key: 'z', metaKey: true, shiftKey: true });
    expect(mockRedo).toHaveBeenCalledTimes(1);
    expect(mockUndo).not.toHaveBeenCalled();
  });
});

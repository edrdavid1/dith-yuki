import { describe, it, expect, vi } from 'vitest';
import {
  confirmUnsavedDocuments,
  confirmUnsavedIfNeeded,
  projectBasename,
  type UnsavedDocumentRef,
} from '../unsavedGuard';

describe('projectBasename', () => {
  it('returns Untitled when path is missing', () => {
    expect(projectBasename(null)).toBe('Untitled');
    expect(projectBasename(undefined)).toBe('Untitled');
  });

  it('strips directories', () => {
    expect(projectBasename('/tmp/foo.dyproj')).toBe('foo.dyproj');
    expect(projectBasename('C:\\proj\\bar.dyproj')).toBe('bar.dyproj');
  });
});

describe('confirmUnsavedIfNeeded', () => {
  it('skips the prompt when clean', async () => {
    const prompt = vi.fn();
    const save = vi.fn();
    await expect(
      confirmUnsavedIfNeeded({ hasDocument: true, dirty: false, prompt, save })
    ).resolves.toBe(true);
    expect(prompt).not.toHaveBeenCalled();
  });

  it('skips when there is no document', async () => {
    const prompt = vi.fn();
    await expect(
      confirmUnsavedIfNeeded({
        hasDocument: false,
        dirty: true,
        prompt: async () => 'cancel',
        save: vi.fn(),
      })
    ).resolves.toBe(true);
    expect(prompt).not.toHaveBeenCalled();
  });

  it('Cancel aborts', async () => {
    await expect(
      confirmUnsavedIfNeeded({
        hasDocument: true,
        dirty: true,
        prompt: async () => 'cancel',
        save: vi.fn(),
      })
    ).resolves.toBe(false);
  });

  it('Don’t Save proceeds without save', async () => {
    const save = vi.fn();
    await expect(
      confirmUnsavedIfNeeded({
        hasDocument: true,
        dirty: true,
        prompt: async () => 'discard',
        save,
      })
    ).resolves.toBe(true);
    expect(save).not.toHaveBeenCalled();
  });

  it('Save failure aborts', async () => {
    await expect(
      confirmUnsavedIfNeeded({
        hasDocument: true,
        dirty: true,
        prompt: async () => 'save',
        save: async () => false,
      })
    ).resolves.toBe(false);
  });
});

describe('confirmUnsavedDocuments', () => {
  const promptMulti = vi.fn();

  it('skips clean documents', async () => {
    const promptSingle = vi.fn();
    const save = vi.fn();
    await expect(
      confirmUnsavedDocuments({
        documents: [
          { id: 1, dirty: false, path: '/a.dyproj' },
          { id: 2, dirty: false, path: '/b.dyproj' },
        ],
        promptSingle,
        promptMulti,
        save,
      })
    ).resolves.toBe(true);
    expect(promptSingle).not.toHaveBeenCalled();
    expect(promptMulti).not.toHaveBeenCalled();
  });

  it('uses single prompt when only one dirty document', async () => {
    const save = vi.fn(async () => true);
    const promptSingle = vi.fn(async () => 'save' as const);
    await expect(
      confirmUnsavedDocuments({
        documents: [
          { id: 1, dirty: true, path: '/a.dyproj' },
          { id: 2, dirty: false, path: '/b.dyproj' },
        ],
        promptSingle,
        promptMulti,
        save,
      })
    ).resolves.toBe(true);
    expect(promptSingle).toHaveBeenCalledOnce();
    expect(promptMulti).not.toHaveBeenCalled();
    expect(save).toHaveBeenCalledWith(expect.objectContaining({ id: 1 }));
  });

  it('uses one multi prompt for several dirty documents', async () => {
    const save = vi.fn(async (_doc: UnsavedDocumentRef) => true);
    const promptSingle = vi.fn();
    await expect(
      confirmUnsavedDocuments({
        documents: [
          { id: 1, dirty: true, path: '/a.dyproj' },
          { id: 2, dirty: false, path: '/b.dyproj' },
          { id: 3, dirty: true, path: '/c.dyproj' },
        ],
        promptSingle,
        promptMulti: async () => ({ kind: 'save-selected', selectedIds: [1, 3] }),
        save,
      })
    ).resolves.toBe(true);
    expect(promptSingle).not.toHaveBeenCalled();
    expect(save.mock.calls.map((c) => c[0].id)).toEqual([1, 3]);
  });

  it('save-selected only saves checked ids; unchecked are discarded', async () => {
    const save = vi.fn(async (_doc: UnsavedDocumentRef) => true);
    await expect(
      confirmUnsavedDocuments({
        documents: [
          { id: 1, dirty: true, path: '/a.dyproj' },
          { id: 2, dirty: true, path: '/b.dyproj' },
          { id: 3, dirty: true, path: '/c.dyproj' },
        ],
        promptSingle: vi.fn(),
        promptMulti: async () => ({ kind: 'save-selected', selectedIds: [2] }),
        save,
      })
    ).resolves.toBe(true);
    expect(save.mock.calls.map((c) => c[0].id)).toEqual([2]);
  });

  it('Discard All proceeds without save', async () => {
    const save = vi.fn();
    await expect(
      confirmUnsavedDocuments({
        documents: [
          { id: 1, dirty: true, path: '/a.dyproj' },
          { id: 2, dirty: true, path: '/b.dyproj' },
        ],
        promptSingle: vi.fn(),
        promptMulti: async () => ({ kind: 'discard-all' }),
        save,
      })
    ).resolves.toBe(true);
    expect(save).not.toHaveBeenCalled();
  });

  it('Cancel aborts without saving', async () => {
    const save = vi.fn();
    await expect(
      confirmUnsavedDocuments({
        documents: [
          { id: 1, dirty: true, path: '/a.dyproj' },
          { id: 2, dirty: true, path: '/b.dyproj' },
        ],
        promptSingle: vi.fn(),
        promptMulti: async () => ({ kind: 'cancel' }),
        save,
      })
    ).resolves.toBe(false);
    expect(save).not.toHaveBeenCalled();
  });

  it('Save failure aborts the quit', async () => {
    await expect(
      confirmUnsavedDocuments({
        documents: [
          { id: 1, dirty: true, path: '/a.dyproj' },
          { id: 2, dirty: true, path: '/b.dyproj' },
        ],
        promptSingle: vi.fn(),
        promptMulti: async () => ({ kind: 'save-selected', selectedIds: [1, 2] }),
        save: async (doc) => doc.id !== 1,
      })
    ).resolves.toBe(false);
  });
});

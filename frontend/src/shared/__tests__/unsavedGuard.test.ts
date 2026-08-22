import { describe, it, expect, vi } from 'vitest';
import {
  confirmUnsavedDocuments,
  confirmUnsavedIfNeeded,
  projectBasename,
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
  it('skips clean documents', async () => {
    const promptFor = vi.fn();
    const save = vi.fn();
    await expect(
      confirmUnsavedDocuments({
        documents: [
          { id: 1, dirty: false, path: '/a.dyproj' },
          { id: 2, dirty: false, path: '/b.dyproj' },
        ],
        promptFor,
        save,
      })
    ).resolves.toBe(true);
    expect(promptFor).not.toHaveBeenCalled();
  });

  it('prompts each dirty document in order (VS Code quit)', async () => {
    const seen: number[] = [];
    const save = vi.fn(async (doc: { id: number }) => {
      seen.push(doc.id);
      return true;
    });
    await expect(
      confirmUnsavedDocuments({
        documents: [
          { id: 1, dirty: true, path: '/a.dyproj' },
          { id: 2, dirty: false, path: '/b.dyproj' },
          { id: 3, dirty: true, path: '/c.dyproj' },
        ],
        promptFor: async () => 'save',
        save,
      })
    ).resolves.toBe(true);
    expect(seen).toEqual([1, 3]);
  });

  it('Cancel on second dirty aborts without saving further', async () => {
    const save = vi.fn(async () => true);
    let n = 0;
    await expect(
      confirmUnsavedDocuments({
        documents: [
          { id: 1, dirty: true, path: '/a.dyproj' },
          { id: 2, dirty: true, path: '/b.dyproj' },
        ],
        promptFor: async () => {
          n += 1;
          return n === 1 ? 'discard' : 'cancel';
        },
        save,
      })
    ).resolves.toBe(false);
    expect(save).not.toHaveBeenCalled();
  });

  it('Save failure aborts the quit walk', async () => {
    await expect(
      confirmUnsavedDocuments({
        documents: [
          { id: 1, dirty: true, path: '/a.dyproj' },
          { id: 2, dirty: true, path: '/b.dyproj' },
        ],
        promptFor: async () => 'save',
        save: async (doc) => doc.id !== 1,
      })
    ).resolves.toBe(false);
  });
});

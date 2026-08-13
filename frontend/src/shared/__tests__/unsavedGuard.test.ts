import { describe, it, expect, vi } from 'vitest';
import { confirmUnsavedIfNeeded, projectBasename } from '../unsavedGuard';

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
        prompt,
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

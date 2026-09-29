import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({
  writeText: vi.fn(),
}));

import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { copyTextToClipboard } from '../clipboard';

const mockWriteText = vi.mocked(writeText);

describe('copyTextToClipboard', () => {
  beforeEach(() => {
    mockWriteText.mockReset();
    mockWriteText.mockResolvedValue(undefined);
  });

  it('writes via clipboard-manager plugin', async () => {
    await copyTextToClipboard('hello');
    expect(mockWriteText).toHaveBeenCalledWith('hello');
  });

  it('logs and rethrows on failure (D8)', async () => {
    const err = new Error('not allowed by ACL permissions config');
    mockWriteText.mockRejectedValue(err);
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
    await expect(copyTextToClipboard('x')).rejects.toThrow(/ACL/);
    expect(spy).toHaveBeenCalled();
    spy.mockRestore();
  });
});

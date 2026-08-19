import { describe, expect, it } from 'vitest';
import { windowChromeTitle } from '../windowTitle';

describe('windowChromeTitle', () => {
  it('uses only the app name with no file', () => {
    expect(
      windowChromeTitle({
        dirty: false,
        hasDocument: false,
        projectPath: null,
        sourcePath: null,
      })
    ).toBe('Dither Yuki');
  });

  it('shows a saved project name', () => {
    expect(
      windowChromeTitle({
        dirty: false,
        hasDocument: true,
        projectPath: '/tmp/cats.dyproj',
        sourcePath: '/tmp/photo.png',
      })
    ).toBe('cats.dyproj — Dither Yuki');
  });

  it('falls back to the opened image name', () => {
    expect(
      windowChromeTitle({
        dirty: false,
        hasDocument: true,
        projectPath: null,
        sourcePath: '/Users/a/photo.png',
      })
    ).toBe('photo.png — Dither Yuki');
  });

  it('prefixes a dirty marker', () => {
    expect(
      windowChromeTitle({
        dirty: true,
        hasDocument: true,
        projectPath: '/tmp/a.dyproj',
        sourcePath: null,
      })
    ).toBe('* a.dyproj — Dither Yuki');
  });
});

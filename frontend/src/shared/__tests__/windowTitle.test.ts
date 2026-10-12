import { describe, expect, it } from 'vitest';
import { appNameForWorkspace, windowChromeTitle } from '../windowTitle';

describe('appNameForWorkspace', () => {
  it('keeps the base name in general', () => {
    expect(appNameForWorkspace('general')).toBe('Dither Yuki');
  });

  it('appends GameDev in gamedev', () => {
    expect(appNameForWorkspace('gamedev')).toBe('Dither Yuki\u2003\u2003▪\u2003\u2003Game Dev');
  });
});

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

  it('shows GameDev in the app name when workspace is gamedev', () => {
    expect(
      windowChromeTitle({
        dirty: false,
        hasDocument: false,
        projectPath: null,
        sourcePath: null,
        workspace: 'gamedev',
      })
    ).toBe('Dither Yuki\u2003\u2003▪\u2003\u2003Game Dev');
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

  it('keeps GameDev after the file name', () => {
    expect(
      windowChromeTitle({
        dirty: false,
        hasDocument: true,
        projectPath: '/tmp/cats.dyproj',
        sourcePath: null,
        workspace: 'gamedev',
      })
    ).toBe('cats.dyproj — Dither Yuki\u2003\u2003▪\u2003\u2003Game Dev');
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

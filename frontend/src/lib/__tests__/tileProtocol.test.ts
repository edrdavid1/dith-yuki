import { describe, expect, it } from 'vitest';
import { buildTileUrl, tileProtocolBase } from '../tileProtocol';

describe('tileProtocolBase', () => {
  it('uses http://tile.localhost on Windows and Android', () => {
    expect(tileProtocolBase('windows')).toBe('http://tile.localhost');
    expect(tileProtocolBase('android')).toBe('http://tile.localhost');
  });

  it('uses tile://localhost on macOS and Linux', () => {
    expect(tileProtocolBase('macos')).toBe('tile://localhost');
    expect(tileProtocolBase('linux')).toBe('tile://localhost');
  });
});

describe('buildTileUrl', () => {
  it('builds Windows WebView2 form', () => {
    expect(
      buildTileUrl(1, { level: 0, x: 2, y: 3 }, 9, { platform: 'windows' }),
    ).toBe(
      'http://tile.localhost/doc/1/layer/composite/stage/composite/l/0/2/3?g=9',
    );
  });

  it('builds macOS custom-scheme form', () => {
    expect(
      buildTileUrl(1, { level: 0, x: 2, y: 3 }, undefined, {
        platform: 'macos',
      }),
    ).toBe(
      'tile://localhost/doc/1/layer/composite/stage/composite/l/0/2/3',
    );
  });

  it('honors an explicit base override', () => {
    expect(
      buildTileUrl(4, { level: 1, x: 0, y: 0 }, 1, {
        base: 'http://tile.localhost',
      }),
    ).toBe(
      'http://tile.localhost/doc/4/layer/composite/stage/composite/l/1/0/0?g=1',
    );
  });
});

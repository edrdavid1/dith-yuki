import { beforeEach, describe, expect, it } from 'vitest';
import {
  isFavorite,
  isHidden,
  readAlgorithmPrefs,
  toggleFavorite,
  toggleHidden,
  writeAlgorithmPrefs,
} from './algorithmPrefs';

describe('algorithmPrefs', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('starts empty', () => {
    const prefs = readAlgorithmPrefs();
    expect(prefs.favorites).toEqual([]);
    expect(prefs.hidden).toEqual([]);
  });

  it('toggles favorite and hide', () => {
    let prefs = toggleFavorite('atkinson');
    expect(isFavorite(prefs, 'atkinson')).toBe(true);
    prefs = toggleFavorite('atkinson');
    expect(isFavorite(prefs, 'atkinson')).toBe(false);

    prefs = toggleHidden('wave');
    expect(isHidden(prefs, 'wave')).toBe(true);
    prefs = toggleHidden('wave');
    expect(isHidden(prefs, 'wave')).toBe(false);
  });

  it('persists across read', () => {
    writeAlgorithmPrefs({
      version: 1,
      favorites: ['floyd_steinberg'],
      hidden: ['blue_noise'],
    });
    const prefs = readAlgorithmPrefs();
    expect(prefs.favorites).toEqual(['floyd_steinberg']);
    expect(prefs.hidden).toEqual(['blue_noise']);
  });
});

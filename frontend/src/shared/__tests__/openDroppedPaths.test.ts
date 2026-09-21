import { describe, expect, it, vi } from 'vitest';
import { classifyDroppedPath, openDroppedPaths } from '../openDroppedPaths';

describe('classifyDroppedPath', () => {
  it('detects projects', () => {
    expect(classifyDroppedPath('/tmp/a.dyproj')).toBe('project');
    expect(classifyDroppedPath('C:\\proj\\B.DYPROJ')).toBe('project');
  });

  it('detects images', () => {
    expect(classifyDroppedPath('/tmp/a.png')).toBe('image');
    expect(classifyDroppedPath('/tmp/a.JPG')).toBe('image');
    expect(classifyDroppedPath('/tmp/a.webp')).toBe('image');
  });

  it('rejects unsupported files', () => {
    expect(classifyDroppedPath('/tmp/a.txt')).toBeNull();
    expect(classifyDroppedPath('/tmp/noext')).toBeNull();
  });
});

describe('openDroppedPaths', () => {
  it('routes each path to the matching opener', async () => {
    const openImageAt = vi.fn();
    const openProjectAt = vi.fn();
    const n = await openDroppedPaths(
      ['/a.png', '/b.dyproj', '/c.txt', '/d.jpeg'],
      { openImageAt, openProjectAt },
    );
    expect(n).toBe(3);
    expect(openImageAt).toHaveBeenCalledWith('/a.png');
    expect(openImageAt).toHaveBeenCalledWith('/d.jpeg');
    expect(openProjectAt).toHaveBeenCalledWith('/b.dyproj');
  });
});

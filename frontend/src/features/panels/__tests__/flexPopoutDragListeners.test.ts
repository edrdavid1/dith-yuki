import { describe, expect, it } from 'vitest';
import { __flexPopoutDragListenerStats } from '../useFlexPopoutJsDrag';

describe('flex popout drag listener stats', () => {
  it('reset clears counters used for leak checks', () => {
    __flexPopoutDragListenerStats.added = 3;
    __flexPopoutDragListenerStats.removed = 1;
    __flexPopoutDragListenerStats.reset();
    expect(__flexPopoutDragListenerStats.added).toBe(0);
    expect(__flexPopoutDragListenerStats.removed).toBe(0);
  });
});

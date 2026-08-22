import { describe, it, expect, beforeEach } from 'vitest';
import {
  OPEN_DOC_MEMORY_WARN_AT,
  resetOpenDocMemoryWarnState,
  shouldWarnOpenDocMemory,
} from '../memoryWarning';

describe('shouldWarnOpenDocMemory', () => {
  beforeEach(() => {
    resetOpenDocMemoryWarnState();
  });

  it('warns once when crossing the threshold', () => {
    expect(shouldWarnOpenDocMemory(OPEN_DOC_MEMORY_WARN_AT - 1)).toBe(false);
    expect(shouldWarnOpenDocMemory(OPEN_DOC_MEMORY_WARN_AT)).toBe(true);
    expect(shouldWarnOpenDocMemory(OPEN_DOC_MEMORY_WARN_AT + 1)).toBe(false);
  });

  it('warns again after dropping below then re-crossing', () => {
    expect(shouldWarnOpenDocMemory(3)).toBe(true);
    expect(shouldWarnOpenDocMemory(2)).toBe(false);
    expect(shouldWarnOpenDocMemory(3)).toBe(true);
  });
});

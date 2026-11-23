import { describe, expect, it } from 'vitest';
import { getDefaultFlexLayoutJson } from '../../defaults/DefaultLayouts';
import { findTabByComponent, listFlexComponents } from '../layoutModelOps';
import { createLayoutStoreForTests, modelFromJson } from '../layoutStore';

describe('optional gamedev panel (layoutStore)', () => {
  it('ensureOptionalPanel injects gamedev on the preferred side once', () => {
    const store = createLayoutStoreForTests();
    store.left = modelFromJson(getDefaultFlexLayoutJson('left'));
    store.right = modelFromJson(getDefaultFlexLayoutJson('right'));
    store.center = modelFromJson(getDefaultFlexLayoutJson('center'));

    expect(listFlexComponents(store.left)).not.toContain('gamedev');

    store.ensureOptionalPanel('gamedev', 'left');
    expect(listFlexComponents(store.left)).toContain('gamedev');
    expect(findTabByComponent(store.left!, 'gamedev')).not.toBeNull();

    const version = store.getSnapshot().version;
    store.ensureOptionalPanel('gamedev', 'left');
    expect(store.getSnapshot().version).toBe(version);
  });

  it('removeOptionalPanel deletes gamedev when present', () => {
    const store = createLayoutStoreForTests();
    store.left = modelFromJson(getDefaultFlexLayoutJson('left'));
    store.right = modelFromJson(getDefaultFlexLayoutJson('right'));
    store.center = modelFromJson(getDefaultFlexLayoutJson('center'));

    store.ensureOptionalPanel('gamedev', 'left');
    expect(listFlexComponents(store.left)).toContain('gamedev');

    store.removeOptionalPanel('gamedev');
    expect(listFlexComponents(store.left)).not.toContain('gamedev');

    const version = store.getSnapshot().version;
    store.removeOptionalPanel('gamedev');
    expect(store.getSnapshot().version).toBe(version);
  });
});

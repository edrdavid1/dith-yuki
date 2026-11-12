import { describe, it, expect, beforeEach } from 'vitest';
import {
  forgetFlexPopoutLabel,
  forgetFlexPopoutLabelByWindowLabel,
  lookupFlexPopoutLabel,
  lookupPanelIdByLabel,
  rememberFlexPopoutLabel,
  resetFlexPopoutRegistryForTests,
} from '../flexPopoutRegistry';

describe('flexPopoutRegistry', () => {
  beforeEach(() => {
    resetFlexPopoutRegistryForTests();
  });

  it('remembers and looks up labels by panel id', () => {
    rememberFlexPopoutLabel('effect', 'flex-popout-2');
    expect(lookupFlexPopoutLabel('effect')).toBe('flex-popout-2');
    forgetFlexPopoutLabel('effect');
    expect(lookupFlexPopoutLabel('effect')).toBeNull();
  });

  it('prefers DOM-window binding over panel id when both exist', () => {
    const dom = { closed: false } as Window;
    rememberFlexPopoutLabel('layers', 'flex-popout-1');
    rememberFlexPopoutLabel('effect', 'flex-popout-9', dom);
    expect(lookupFlexPopoutLabel('layers', dom)).toBe('flex-popout-9');
    expect(lookupFlexPopoutLabel('effect', dom)).toBe('flex-popout-9');
  });

  it('reverse-looks up panel id by window label (W2 Destroyed)', () => {
    rememberFlexPopoutLabel('colorlab', 'flex-popout-7');
    expect(lookupPanelIdByLabel('flex-popout-7')).toBe('colorlab');
    expect(lookupPanelIdByLabel('flex-popout-missing')).toBeNull();
  });

  it('removes registry entry on Destroyed label (W6)', () => {
    rememberFlexPopoutLabel('layers', 'flex-popout-3');
    forgetFlexPopoutLabelByWindowLabel('flex-popout-3');
    expect(lookupFlexPopoutLabel('layers')).toBeNull();
    expect(lookupPanelIdByLabel('flex-popout-3')).toBeNull();
  });

  it('does not close other entries when a nonexistent label is forgotten', () => {
    rememberFlexPopoutLabel('layers', 'flex-popout-1');
    rememberFlexPopoutLabel('effect', 'flex-popout-2');
    forgetFlexPopoutLabelByWindowLabel('flex-popout-missing');
    expect(lookupFlexPopoutLabel('layers')).toBe('flex-popout-1');
    expect(lookupFlexPopoutLabel('effect')).toBe('flex-popout-2');
  });
});

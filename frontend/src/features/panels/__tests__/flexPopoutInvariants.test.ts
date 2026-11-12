import { describe, it, expect, beforeEach, vi } from 'vitest';
import {
  rememberFlexPopoutLabel,
  resetFlexPopoutRegistryForTests,
} from '../flexPopoutRegistry';
import { checkFlexPopoutInvariants } from '../flexPopoutInvariants';

vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getAllWebviewWindows: vi.fn(async () => [
    { label: 'main' },
    { label: 'flex-popout-1' },
  ]),
}));

describe('flexPopoutInvariants (W6)', () => {
  beforeEach(() => {
    resetFlexPopoutRegistryForTests();
  });

  it('flags registry labels without a live window', async () => {
    rememberFlexPopoutLabel('layers', 'flex-popout-1');
    rememberFlexPopoutLabel('effect', 'flex-popout-missing');
    const v = await checkFlexPopoutInvariants({
      left: null,
      center: null,
      right: null,
    });
    expect(v.some((x) => x.kind === 'registry-label-missing-window')).toBe(
      true,
    );
    expect(
      v.find((x) => x.detail.includes('flex-popout-missing')),
    ).toBeTruthy();
  });
});

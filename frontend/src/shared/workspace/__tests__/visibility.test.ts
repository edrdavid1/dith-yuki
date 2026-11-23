import { describe, expect, it } from 'vitest';
import { uiVisibilityFromConfig } from '../visibility';

describe('uiVisibilityFromConfig', () => {
  it('visibility_general_hides_tiling', () => {
    const v = uiVisibilityFromConfig({
      workspace: 'general',
      advanced: { tiling_advanced: true },
    });
    expect(v.showTilingSection).toBe(false);
    expect(v.showSeamlessPreview).toBe(false);
    expect(v.showWrapPadding).toBe(false);
    expect(v.showAdvancedToggle).toBe(false);
    expect(v.showGamedevStrip).toBe(false);
    expect(v.showPreviewViewModes).toBe(false);
  });

  it('visibility_gamedev_shows_tiling', () => {
    const v = uiVisibilityFromConfig({
      workspace: 'gamedev',
      advanced: { tiling_advanced: false },
    });
    expect(v.showTilingSection).toBe(true);
    expect(v.showSeamlessPreview).toBe(true);
    expect(v.showEdTemporalWarning).toBe(true);
    expect(v.showWrapPadding).toBe(false);
    expect(v.showAdvancedToggle).toBe(true);
    expect(v.showGamedevStrip).toBe(true);
    expect(v.showPreviewViewModes).toBe(true);
  });

  it('visibility_gamedev_advanced_shows_wrap_padding', () => {
    const v = uiVisibilityFromConfig({
      workspace: 'gamedev',
      advanced: { tiling_advanced: true },
    });
    expect(v.showWrapPadding).toBe(true);
  });

  it('from_config_pure', () => {
    const cfg = {
      workspace: 'gamedev' as const,
      advanced: { tiling_advanced: true },
    };
    expect(uiVisibilityFromConfig(cfg)).toEqual(uiVisibilityFromConfig(cfg));
  });
});

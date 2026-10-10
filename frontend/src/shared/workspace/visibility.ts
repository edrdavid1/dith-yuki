/**
 * UiVisibility — sole authority for workspace-gated UI (FR-4 / FR-5).
 * Mirrors `src-tauri/src/config/visibility.rs`. UI must not branch on Workspace.
 */

export type Workspace = 'general' | 'gamedev';

export interface AdvancedConfig {
  tiling_advanced: boolean;
}

export interface AppConfigLike {
  workspace: Workspace;
  advanced: AdvancedConfig;
}

export interface UiVisibility {
  showTilingSection: boolean;
  showSeamlessPreview: boolean;
  showEdTemporalWarning: boolean;
  showWrapPadding: boolean;
  showTileSizeSetting: boolean;
  showAdvancedToggle: boolean;
}

/** Pure; allocates nothing beyond the returned object (NFR-1). */
export function uiVisibilityFromConfig(cfg: AppConfigLike): UiVisibility {
  if (cfg.workspace === 'general') {
    return {
      showTilingSection: false,
      showSeamlessPreview: false,
      showEdTemporalWarning: false,
      showWrapPadding: false,
      showTileSizeSetting: false,
      showAdvancedToggle: false,
    };
  }
  return {
    showTilingSection: true,
    showSeamlessPreview: true,
    showEdTemporalWarning: true,
    showWrapPadding: Boolean(cfg.advanced?.tiling_advanced),
    showTileSizeSetting: true,
    showAdvancedToggle: true,
  };
}

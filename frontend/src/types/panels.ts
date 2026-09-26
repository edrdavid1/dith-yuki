// =============================================================================
// FlexLayout panel types (Preferences / Help are dialogs, not panels)
// =============================================================================

export type DockSide = 'left' | 'right';

/** Dockable / floatable FlexLayout panel components. */
export type PanelId = 'effect' | 'layers' | 'colorlab' | 'preview';

export const PANEL_IDS: PanelId[] = ['effect', 'layers', 'colorlab', 'preview'];

export const PANEL_DISPLAY_NAMES: Record<PanelId, string> = {
  effect: 'Effect Settings',
  layers: 'Layers',
  colorlab: 'Color Lab',
  preview: 'Preview',
};

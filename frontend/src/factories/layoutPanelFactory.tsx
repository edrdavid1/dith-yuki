/**
 * layoutPanelFactory — maps FlexLayout component IDs to React nodes.
 *
 * Docked: FlexLayout tab strip is the chrome (hideChrome).
 * Floated OS window: Color Lab–style FlexPopoutChrome (close = dock).
 */

import React from 'react';
import type { TabNode } from 'flexlayout-react';
import LayersFeature from '../features/layers/LayersFeature';
import EffectsFeature from '../features/effects/EffectsFeature';
import ColorLabFeature from '../features/color-lab/ColorLabFeature';
import PreviewFeature from '../features/preview/PreviewFeature';
import FlexPopoutChrome from '../features/panels/FlexPopoutChrome';
import type { FlexSide } from '../defaults/DefaultLayouts';
import type { DockSide } from '../types/panels';
import type { PanelChromeProps } from '../features/panels/PanelChrome';
import type { WelcomeActions } from '../hooks/useWelcomeScreen';

export type LayoutPanelFactoryOptions = {
  side: FlexSide;
  onMoveToSide?: (side: DockSide) => void;
  onPopOut?: () => void;
  onDockBack?: () => void;
  /** Welcome actions for Preview (main + flex-popout share the main React tree). */
  welcome?: WelcomeActions;
};

/**
 * Returns the React node to render for a given FlexLayout tab.
 */
export function layoutPanelFactory(
  node: TabNode,
  options?: LayoutPanelFactoryOptions,
): React.ReactNode {
  const componentId = node.getComponent() ?? '';
  const floating = node.isFloating();
  const chrome: PanelChromeProps = {
    // Docked: tab strip chrome. Floated: FlexPopoutChrome owns the titlebar.
    hideChrome: true,
    dockSide: options?.side === 'center' ? undefined : (options?.side as DockSide | undefined),
    onMoveToSide: floating || options?.side === 'center' ? undefined : options?.onMoveToSide,
    onPopOut: floating ? undefined : options?.onPopOut,
    onDockBack: undefined,
  };

  const body = (() => {
    switch (componentId) {
      case 'layers':
        return <LayersFeature {...chrome} />;
      case 'effect':
        return <EffectsFeature {...chrome} />;
      case 'colorlab':
        // One TabNode; docked = full Color Lab stacked in one column.
        return (
          <ColorLabFeature
            variant={floating ? 'full' : 'sidebar'}
            {...chrome}
          />
        );
      case 'preview':
        return (
          <PreviewFeature
            {...chrome}
            hideTitleBar
            fill={floating}
            welcome={options?.welcome}
          />
        );
      default:
        return <ErrorPanel componentId={componentId} />;
    }
  })();

  if (!floating) return body;

  return (
    <FlexPopoutChrome
      title={node.getName() || getPanelDisplayName(componentId)}
      panelId={componentId}
      onDockBack={() => options?.onDockBack?.()}
      closeLabel={
        componentId === 'preview'
          ? 'Return preview to main window (or double-click titlebar)'
          : 'Dock panel back to sidebar'
      }
    >
      {body}
    </FlexPopoutChrome>
  );
}

// ─── Internal sub-components ─────────────────────────────────────────────────

interface ErrorPanelProps {
  componentId: string;
}

const ErrorPanel: React.FC<ErrorPanelProps> = ({ componentId }) => (
  <div
    style={{
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      justifyContent: 'center',
      height: '100%',
      color: '#c00',
      fontSize: 13,
      padding: 16,
    }}
  >
    <div style={{ fontWeight: 600, marginBottom: 4 }}>Unknown panel</div>
    <code style={{ fontSize: 11 }}>{componentId}</code>
  </div>
);

// ─── Helpers ─────────────────────────────────────────────────────────────────

export function getPanelDisplayName(panelId: string): string {
  switch (panelId) {
    case 'layers':   return 'Layers';
    case 'effect':   return 'Effect Settings';
    case 'colorlab': return 'Color Lab';
    case 'preview':  return 'Preview';
    default:         return panelId;
  }
}

/** Returns true if the panel is allowed inside a sidebar FlexLayout. */
export function isPanelDockable(panelId: string): boolean {
  return panelId === 'layers' || panelId === 'effect' || panelId === 'colorlab';
}

/**
 * Returns true if the panel is currently rendered by FlexLayout.
 * B3: Layers. B4a: Effect. B4b: Color Lab. B5: Preview (center model).
 */
export function isPanelOnFlexLayout(panelId: string): boolean {
  return (
    panelId === 'layers' ||
    panelId === 'effect' ||
    panelId === 'colorlab' ||
    panelId === 'preview'
  );
}

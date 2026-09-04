/**
 * FlexLayoutContainer — renders one side's FlexLayout Model as a dockable tab UI.
 *
 * Docked chrome matches Color Lab WindowTitlebar (squares + stripes).
 * Float: Color Lab–style frameless popout (FlexPopoutChrome).
 * Drag titlebar out of the column → undock (same gesture as Color Lab).
 */

import React, { useEffect, useRef, useCallback } from 'react';
import {
  Layout,
  Actions,
  Action,
  type Model,
  type TabNode,
  type ITabRenderValues,
  I18nLabel,
} from 'flexlayout-react';
import { invoke } from '@tauri-apps/api/core';
import {
  listDockedFlexComponents,
  normalizeSideToVerticalStack,
  useLayoutContext,
  useSideLayout,
} from '../contexts/LayoutContext';
import { layoutPanelFactory } from '../factories/layoutPanelFactory';
import type { FlexSide } from '../defaults/DefaultLayouts';
import { useShell } from '../app/shell/ShellContext';
import WindowTitlebar from '../shared/ui/WindowTitlebar';
import { useFlexTitlebarUndock } from '../features/panels/useFlexTitlebarUndock';
import { useDockZoneReporter } from '../hooks/useDockZoneReporter';
import 'flexlayout-react/style/dark.css';
import '../shared/styles/flexlayout-theme.css';

export interface FlexLayoutContainerProps {
  /** Which sidebar this container manages. */
  side: FlexSide;
  /** Extra CSS class for the wrapper div */
  className?: string;
  /** Inline styles for the wrapper div */
  style?: React.CSSProperties;
}

export const FlexLayoutContainer: React.FC<FlexLayoutContainerProps> = ({
  side,
  className,
  style,
}) => {
  const { model, setModel, isLoading } = useSideLayout(side);
  const { movePanelBetweenSides, floatPanel, dockPanel } = useLayoutContext();
  const { leftSidebar, rightSidebar, setSidebarCollapsed } = useShell();
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const columnRef = useRef<HTMLDivElement>(null);
  const sidebarPrefs = side === 'left' ? leftSidebar : rightSidebar;
  const dockedCount = listDockedFlexComponents(model).length;

  useDockZoneReporter({
    sidebarRef: columnRef,
    sidebarSide: side,
    sidebarCollapsed: sidebarPrefs.collapsed,
    sidebarWidth: sidebarPrefs.width,
    hasDockTargets: dockedCount > 0,
    // Always report a zone so floated panels can redock onto this side.
    reportEmptyEdge: true,
  });

  useEffect(() => {
    return () => {
      if (saveTimerRef.current !== null) {
        clearTimeout(saveTimerRef.current);
      }
    };
  }, []);

  // Strip FlexLayout native `title=` browser tooltips (float/overflow/helpText).
  useEffect(() => {
    const root = columnRef.current;
    if (!root) return;

    const scrub = () => {
      root.querySelectorAll<HTMLElement>('[title]').forEach((el) => {
        el.removeAttribute('title');
      });
    };
    scrub();
    const mo = new MutationObserver(scrub);
    mo.observe(root, {
      subtree: true,
      attributes: true,
      attributeFilter: ['title'],
      childList: true,
    });
    return () => mo.disconnect();
  }, [model]);

  const saveCommand = side === 'left' ? 'save_layout_left' : 'save_layout_right';

  const scheduleSave = useCallback(
    (m: Model) => {
      if (saveTimerRef.current !== null) {
        clearTimeout(saveTimerRef.current);
      }
      saveTimerRef.current = setTimeout(() => {
        saveTimerRef.current = null;
        const json = JSON.stringify(m.toJson());
        invoke<void>(saveCommand, { json }).catch((err) => {
          console.error(`[FlexLayoutContainer:${side}] ${saveCommand} failed:`, err);
        });
      }, 500);
    },
    [saveCommand, side],
  );

  const normalizingRef = useRef(false);

  const handleModelChange = useCallback(
    (newModel: Model, _action: Action) => {
      if (!normalizingRef.current) {
        normalizingRef.current = true;
        try {
          // Keep Color Lab–style vertical stack if a drop somehow created peer tabs.
          normalizeSideToVerticalStack(newModel);
        } finally {
          normalizingRef.current = false;
        }
      }
      setModel(newModel);
      scheduleSave(newModel);
    },
    [setModel, scheduleSave],
  );

  /** Sidebar is narrow — never form peer tabs / side-by-side splits inside a column. */
  const onAction = useCallback((action: Action) => {
    if (
      action.type === Actions.MOVE_NODE ||
      action.type === Actions.ADD_NODE
    ) {
      const loc = action.data?.location as string | undefined;
      if (loc === 'center' || loc === 'left' || loc === 'right') {
        return new Action(action.type, { ...action.data, location: 'bottom' });
      }
    }
    return action;
  }, []);

  const factory = useCallback(
    (node: TabNode): React.ReactNode => {
      const component = node.getComponent() ?? '';
      return layoutPanelFactory(node, {
        side,
        onMoveToSide: (target) => {
          movePanelBetweenSides(component, target);
          setSidebarCollapsed(target, false);
        },
        onPopOut: () => floatPanel(side, component),
        onDockBack: () => dockPanel(side, component),
      });
    },
    [side, movePanelBetweenSides, setSidebarCollapsed, floatPanel, dockPanel],
  );

  // Latest float target for undock-drag (set in onRenderTab per tab).
  const undockComponentRef = useRef<string>('');
  const onUndockDrag = useFlexTitlebarUndock({
    columnRef,
    onUndock: () => {
      const component = undockComponentRef.current;
      if (component) floatPanel(side, component);
    },
  });

  const onRenderTab = useCallback(
    (node: TabNode, renderValues: ITabRenderValues) => {
      const component = node.getComponent() ?? '';
      const floating = node.isFloating();
      renderValues.leading = <></>;
      renderValues.buttons = [];
      // Floating tabs stay in the model (for OS popouts) but leave the strip so
      // the remaining docked panel can stretch like a single titlebar.
      if (floating) {
        renderValues.content = <span data-flex-floating-tab hidden />;
        return;
      }
      renderValues.content = (
        <WindowTitlebar
          title={node.getName()}
          className="flex-dock-titlebar"
          dockSide={side}
          onMouseDown={(e) => {
            undockComponentRef.current = component;
            onUndockDrag(e);
          }}
          onMoveToSide={(target) => {
            movePanelBetweenSides(component, target);
            setSidebarCollapsed(target, false);
          }}
          onPopOut={() => floatPanel(side, component)}
        />
      );
    },
    [side, movePanelBetweenSides, setSidebarCollapsed, floatPanel, onUndockDrag],
  );

  const onRenderTabSet = useCallback((_node: unknown, renderValues: {
    stickyButtons: React.ReactNode[];
    buttons: React.ReactNode[];
    headerButtons: React.ReactNode[];
  }) => {
    // Drop any FL chrome before the library appends float/overflow controls.
    renderValues.stickyButtons = [];
    renderValues.buttons = [];
    renderValues.headerButtons = [];
  }, []);

  // No dock placeholder — floated panels free the column; redock via drag or titlebar.
  const onRenderFloatingTabPlaceholder = useCallback(
    () => undefined,
    [],
  );

  const i18nMapper = useCallback((id: I18nLabel, _param?: string) => {
    switch (id) {
      case I18nLabel.Floating_Window_Message:
        return '';
      case I18nLabel.Floating_Window_Show_Window:
        return 'Show window';
      case I18nLabel.Floating_Window_Dock_Window:
        return 'Dock back';
      case I18nLabel.Float_Tab:
        return 'Open in separate window';
      default:
        return undefined;
    }
  }, []);

  if (isLoading || model === null) {
    return (
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          height: '100%',
          color: 'var(--color-text-secondary, #999)',
          fontSize: 13,
          ...style,
        }}
        className={className}
      >
        Loading layout…
      </div>
    );
  }

  return (
    <div
      ref={columnRef}
      className={className}
      style={{ position: 'relative', height: '100%', overflow: 'hidden', ...style }}
    >
      <Layout
        model={model as Model}
        factory={factory}
        onAction={onAction}
        onModelChange={handleModelChange}
        onRenderTab={onRenderTab}
        onRenderTabSet={onRenderTabSet}
        onRenderFloatingTabPlaceholder={onRenderFloatingTabPlaceholder}
        i18nMapper={i18nMapper}
        supportsPopout={true}
        popoutURL={`${typeof window !== 'undefined' ? window.location.origin : ''}/popout.html`}
        realtimeResize={false}
      />
    </div>
  );
};

export default FlexLayoutContainer;

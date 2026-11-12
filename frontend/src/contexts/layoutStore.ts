/**
 * W4 — LayoutStore: single owner of FlexLayout Model instances.
 *
 * Models live outside React state. React subscribes via useSyncExternalStore
 * (layoutEpoch === store version). HMR preserves the singleton via import.meta.hot.
 *
 * Model.fromJson / new Model ownership: this module only (see
 * __tests__/noModelFromJsonOutsideStore.test.ts). In-place doAction mutations
 * keep identity; replace paths closeAllFlexPopouts first.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Actions, DockLocation, Model } from 'flexlayout-react';
import type { IJsonModel } from 'flexlayout-react';
import {
  getDefaultFlexLayoutJson,
  isValidFlexLayoutJson,
  LAYOUT_TOAST_MESSAGES,
} from '../defaults/DefaultLayouts';
import type { FlexSide } from '../defaults/DefaultLayouts';
import { isPanelOnFlexLayout } from '../factories/layoutPanelFactory';
import { closeAllFlexPopouts } from '../features/panels/closeAllFlexPopouts';
import { closeFlexPopoutShell } from '../features/panels/closeFlexPopoutShell';
import { forgetFlexPopoutLabel } from '../features/panels/flexPopoutRegistry';
import { floatUiAllowed } from '../features/panels/flexlayoutPatchGate';
import {
  applyAppChromePolicy,
  dockAllFloating,
  emptySideJson,
  findFirstTabSetId,
  findTabByComponent,
  listFlexComponents,
  listFloatingFlexComponents,
  modelForSide,
  normalizeSideToVerticalStack,
  rebalanceFloatingTabsets,
  tabDisplayName,
} from './layoutModelOps';
import { normalizeLayoutEnvelope } from './layoutNormalizer';

const PERSIST_DEBOUNCE_MS = 500;

export interface LayoutStoreSnapshot {
  version: number;
  left: Model | null;
  right: Model | null;
  center: Model | null;
  leftLoading: boolean;
  rightLoading: boolean;
  centerLoading: boolean;
  layoutToast: string | null;
}

type Listener = () => void;

/** Sole Model.fromJson entry point for product code (W4). */
export function modelFromJson(json: IJsonModel): Model {
  return applyAppChromePolicy(Model.fromJson(json));
}

function sideJsonOrDefault(value: unknown, side: FlexSide): IJsonModel {
  if (isValidFlexLayoutJson(value)) return value;
  return getDefaultFlexLayoutJson(side);
}

function toastForNotice(notice: string | null | undefined): string | null {
  switch (notice) {
    case 'migrated':
      return LAYOUT_TOAST_MESSAGES.MIGRATED_ENVELOPE;
    case 'recovered_bak':
      return LAYOUT_TOAST_MESSAGES.RECOVERED_BAK;
    case 'corrupt_reset':
      return LAYOUT_TOAST_MESSAGES.CORRUPT_RECOVERED;
    default:
      return null;
  }
}

export class LayoutStore {
  private version = 0;
  private left: Model | null = null;
  private right: Model | null = null;
  private center: Model | null = null;
  private leftLoading = true;
  private rightLoading = true;
  private centerLoading = true;
  private layoutToast: string | null = null;

  private listeners = new Set<Listener>();
  private loadPromise: Promise<void> | null = null;
  private dockListenerRefCount = 0;
  private dockListenGeneration = 0;
  private dockUnlisten: (() => void) | null = null;
  private flushUnlisten: (() => void) | null = null;
  private persistTimer: ReturnType<typeof setTimeout> | null = null;
  private persistInFlight: Promise<void> | null = null;

  /** Cached snapshot so useSyncExternalStore gets a stable ref until bump. */
  private snapshot: LayoutStoreSnapshot = this.buildSnapshot();

  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  getSnapshot = (): LayoutStoreSnapshot => this.snapshot;

  getVersion(): number {
    return this.version;
  }

  /**
   * Idempotent init: start load once; attach dock listener with refcount so
   * StrictMode double-mount never stacks two `flex-panel-dock-request` listeners.
   */
  ensureInitialized = (): (() => void) => {
    void this.startLoadOnce();
    this.attachFlushListener();
    return this.attachDockListener();
  };

  private attachFlushListener(): void {
    if (this.flushUnlisten) return;
    void listen('flush-layout', () => {
      void this.flushForClose();
    })
      .then((fn) => {
        this.flushUnlisten = fn;
      })
      .catch((err) => {
        console.error('[flex-dock] flush-layout listen failed:', err);
      });
  }

  /** W7: debounce 500ms — used by FlexLayoutContainer onModelChange. */
  schedulePersist = (): void => {
    if (this.persistTimer != null) clearTimeout(this.persistTimer);
    this.persistTimer = setTimeout(() => {
      this.persistTimer = null;
      void this.persistEnvelope();
    }, PERSIST_DEBOUNCE_MS);
  };

  /** W7: immediate envelope write (float/dock/move/redock/swap/reset). */
  persistNow = (): void => {
    if (this.persistTimer != null) {
      clearTimeout(this.persistTimer);
      this.persistTimer = null;
    }
    void this.persistEnvelope();
  };

  /** W7 close flush: write then ack Rust. */
  flushForClose = async (): Promise<void> => {
    if (this.persistTimer != null) {
      clearTimeout(this.persistTimer);
      this.persistTimer = null;
    }
    await this.persistEnvelope();
    try {
      await invoke<void>('layout_flushed');
    } catch (err) {
      console.error('[flex-dock] layout_flushed failed:', err);
    }
  };

  private async persistEnvelope(): Promise<void> {
    if (!this.left || !this.right || !this.center) return;
    if (this.persistInFlight) {
      await this.persistInFlight;
    }
    const run = (async () => {
      const envelope = {
        version: 1,
        savedAt: new Date().toISOString(),
        left: this.left!.toJson(),
        center: this.center!.toJson(),
        right: this.right!.toJson(),
      };
      try {
        await invoke<void>('save_shell_layout', {
          json: JSON.stringify(envelope),
        });
      } catch (err) {
        console.error('[flex-dock] save_shell_layout failed:', err);
      }
    })();
    this.persistInFlight = run;
    try {
      await run;
    } finally {
      if (this.persistInFlight === run) this.persistInFlight = null;
    }
  }

  private startLoadOnce(): Promise<void> {
    if (this.loadPromise) return this.loadPromise;
    this.loadPromise = this.loadAll();
    return this.loadPromise;
  }

  private attachDockListener(): () => void {
    this.dockListenerRefCount += 1;
    if (this.dockListenerRefCount === 1) {
      const generation = ++this.dockListenGeneration;
      void listen<{ panelId: string; side: 'left' | 'right' }>(
        'flex-panel-dock-request',
        (event) => {
          const { panelId, side } = event.payload;
          if (!isPanelOnFlexLayout(panelId)) return;
          if (panelId === 'preview') {
            this.dockPanel('center', 'preview');
          } else if (side === 'left' || side === 'right') {
            this.redockFlexPanelToSide(panelId, side);
          }
        },
      ).then((fn) => {
        // Drop stale listen() from StrictMode unmount/remount races.
        if (
          this.dockListenerRefCount === 0 ||
          generation !== this.dockListenGeneration
        ) {
          fn();
          return;
        }
        this.dockUnlisten = fn;
      }).catch((err) => {
        console.error('[flex-dock] flex-panel-dock-request listen failed:', err);
      });
    }

    return () => {
      this.dockListenerRefCount -= 1;
      if (this.dockListenerRefCount <= 0) {
        this.dockListenerRefCount = 0;
        this.dockListenGeneration += 1;
        this.dockUnlisten?.();
        this.dockUnlisten = null;
      }
    };
  }

  /** Test helper — how many live dock listeners the store thinks it owns. */
  getDockListenerRefCountForTests(): number {
    return this.dockListenerRefCount;
  }

  private async loadAll(): Promise<void> {
    let notice: string | null = null;
    let leftJson = getDefaultFlexLayoutJson('left');
    let rightJson = getDefaultFlexLayoutJson('right');
    let centerJson = getDefaultFlexLayoutJson('center');

    try {
      const raw = await invoke<string>('load_shell_layout');
      const envelope = JSON.parse(raw) as {
        left?: unknown;
        center?: unknown;
        right?: unknown;
        notice?: string | null;
      };
      notice = envelope.notice ?? null;
      leftJson = sideJsonOrDefault(envelope.left, 'left');
      rightJson = sideJsonOrDefault(envelope.right, 'right');
      centerJson = sideJsonOrDefault(envelope.center, 'center');
    } catch (err) {
      console.error('[flex-dock] load_shell_layout failed, using defaults:', err);
      notice = 'corrupt_reset';
    }

    const normalized = normalizeLayoutEnvelope({
      left: leftJson,
      center: centerJson,
      right: rightJson,
    });
    if (normalized.changes.length > 0) {
      console.info('[flex-dock] layout normalizer:', normalized.changes);
    }

    const left = modelFromJson(normalized.left);
    const right = modelFromJson(normalized.right);
    const center = modelFromJson(normalized.center);
    dockAllFloating(left);
    dockAllFloating(right);
    dockAllFloating(center);

    await closeAllFlexPopouts();

    this.left = left;
    this.right = right;
    this.center = center;
    this.leftLoading = false;
    this.rightLoading = false;
    this.centerLoading = false;

    this.layoutToast =
      toastForNotice(notice) ??
      (normalized.changes.some((c) => c.startsWith('injected colorlab'))
        ? LAYOUT_TOAST_MESSAGES.COLORLAB_ADDED
        : normalized.changes.some((c) => c.startsWith('injected preview'))
          ? LAYOUT_TOAST_MESSAGES.PREVIEW_ADDED
          : null);

    // Persist normalized envelope when migration / recovery / inject happened.
    if (notice || normalized.changes.length > 0) {
      this.persistNow();
    }

    this.bump();
  }

  /**
   * W4 reset/load: close all flex-popouts → wait → replace models → bump.
   */
  resetToDefaults = (): void => {
    void this.resetToDefaultsAsync();
  };

  private async resetToDefaultsAsync(): Promise<void> {
    await closeAllFlexPopouts();
    const left = modelFromJson(getDefaultFlexLayoutJson('left'));
    const right = modelFromJson(getDefaultFlexLayoutJson('right'));
    const center = modelFromJson(getDefaultFlexLayoutJson('center'));
    dockAllFloating(left);
    dockAllFloating(right);
    dockAllFloating(center);
    this.left = left;
    this.right = right;
    this.center = center;
    this.persistNow();
    this.bump();
    console.info('[flex-dock] resetToDefaults');
  }

  clearLayoutToast = (): void => {
    if (this.layoutToast == null) return;
    this.layoutToast = null;
    this.bump();
  };

  /** One-shot shell toast (W6 label-miss, recovery notices). */
  setLayoutToast = (message: string | null): void => {
    this.layoutToast = message;
    this.bump();
  };

  /**
   * FlexLayout onModelChange / container setModel — same identity preferred.
   * Identity change still bumps; replace paths should use replaceSides.
   */
  setSideModel = (side: FlexSide, model: Model): void => {
    const cur = modelForSide(side, this.left, this.right, this.center);
    if (cur && cur !== model) {
      console.warn(
        '[flex-dock] setSideModel identity change without replaceSides',
        side,
      );
    }
    if (side === 'left') this.left = model;
    else if (side === 'right') this.right = model;
    else this.center = model;
    this.bump();
  };

  private async replaceSides( partial: {
    left?: Model;
    right?: Model;
    center?: Model;
  }): Promise<void> {
    await closeAllFlexPopouts();
    if (partial.left !== undefined) this.left = partial.left;
    if (partial.right !== undefined) this.right = partial.right;
    if (partial.center !== undefined) this.center = partial.center;
    this.bump();
  }

  movePanelBetweenSides = (panelComponent: string, to: 'left' | 'right'): void => {
    void this.movePanelBetweenSidesAsync(panelComponent, to);
  };

  private async movePanelBetweenSidesAsync(
    panelComponent: string,
    to: 'left' | 'right',
  ): Promise<void> {
    if (panelComponent === 'preview') return;
    const from: 'left' | 'right' = to === 'left' ? 'right' : 'left';
    const fromModel = from === 'left' ? this.left : this.right;
    const toModel = to === 'left' ? this.left : this.right;
    if (!fromModel || !toModel) {
      console.warn('[flex-dock] movePanelBetweenSides: model missing', { from, to });
      return;
    }

    if (findTabByComponent(toModel, panelComponent)) return;

    const tab = findTabByComponent(fromModel, panelComponent);
    if (!tab) {
      console.warn('[flex-dock] movePanelBetweenSides: tab not found', panelComponent, 'on', from);
      return;
    }

    const raw = tab.toJson() as Record<string, unknown>;
    const { id: _omitId, floating: _omitFloat, ...rest } = raw;
    const json = {
      ...rest,
      type: 'tab',
      name: (typeof rest.name === 'string' && rest.name) || tabDisplayName(panelComponent),
      component: panelComponent,
      enableFloat: true,
    };

    if (tab.isFloating()) {
      try {
        void closeFlexPopoutShell(tab.getWindow?.() ?? null, {
          panelId: panelComponent,
        });
      } catch (err) {
        console.error('[flex-dock] movePanelBetweenSides closeFlexPopoutShell failed', err);
      }
      forgetFlexPopoutLabel(panelComponent);
    }

    fromModel.doAction(Actions.deleteTab(tab.getId()));

    let targetTabSetId = findFirstTabSetId(toModel);
    if (!targetTabSetId) {
      const seed = modelFromJson({
        global: getDefaultFlexLayoutJson(to).global,
        borders: [],
        layout: {
          type: 'row',
          weight: 100,
          children: [
            {
              type: 'tabset',
              weight: 100,
              children: [json],
            },
          ],
        },
      } as IJsonModel);
      const fromEmpty = listFlexComponents(fromModel).length === 0;
      const fromNext = fromEmpty ? modelFromJson(emptySideJson(from)) : fromModel;
      if (!fromEmpty) applyAppChromePolicy(fromModel);
      await this.replaceSides(
        to === 'left'
          ? { left: seed, right: fromNext }
          : { right: seed, left: fromNext },
      );
      this.persistNow();
      this.persistNow();
      return;
    }

    try {
      toModel.doAction(Actions.addNode(json, targetTabSetId, DockLocation.BOTTOM, -1, true));
    } catch (err) {
      console.error('[flex-dock] addNode failed', err);
      return;
    }

    normalizeSideToVerticalStack(toModel);
    normalizeSideToVerticalStack(fromModel);

    if (listFlexComponents(fromModel).length === 0) {
      const fromNext = modelFromJson(emptySideJson(from));
      await this.replaceSides(from === 'left' ? { left: fromNext } : { right: fromNext });
      this.persistNow();
      this.persistNow();
      return;
    }

    applyAppChromePolicy(fromModel);
    applyAppChromePolicy(toModel);
    this.persistNow();
    this.persistNow();
    this.bump();
  }

  floatPanel = (side: FlexSide, panelComponent: string): void => {
    if (!floatUiAllowed()) {
      console.error(
        '[flex-dock] floatPanel blocked — flexlayout-react patch missing',
      );
      return;
    }
    const model = modelForSide(side, this.left, this.right, this.center);
    if (!model) {
      console.warn('[flex-dock] floatPanel: model missing', side);
      return;
    }
    const tab = findTabByComponent(model, panelComponent);
    if (!tab) {
      console.warn('[flex-dock] floatPanel: tab not found', panelComponent);
      return;
    }
    if (tab.isFloating()) return;
    // Keep the SAME model instance — cloning kills FlexLayout's FloatingWindow.
    model.doAction(Actions.floatTab(tab.getId()));
    normalizeSideToVerticalStack(model);
    rebalanceFloatingTabsets(model);
    this.persistNow();
    this.bump();
  };

  dockPanel = (side: FlexSide, panelComponent: string): void => {
    const model = modelForSide(side, this.left, this.right, this.center);
    if (!model) {
      console.warn('[flex-dock] dockPanel: model missing', side);
      return;
    }
    const tab = findTabByComponent(model, panelComponent);
    if (!tab) {
      console.warn('[flex-dock] dockPanel: tab not found', panelComponent);
      return;
    }
    if (!tab.isFloating()) return;

    try {
      void closeFlexPopoutShell(tab.getWindow?.() ?? null, {
        panelId: panelComponent,
      });
    } catch (err) {
      console.error('[flex-dock] dockPanel closeFlexPopoutShell failed', err);
    }
    forgetFlexPopoutLabel(panelComponent);

    model.doAction(Actions.unFloatTab(tab.getId()));
    normalizeSideToVerticalStack(model);
    rebalanceFloatingTabsets(model);
    this.persistNow();
    this.bump();
  };

  redockFlexPanelToSide = (panelComponent: string, to: 'left' | 'right'): void => {
    if (panelComponent === 'preview') {
      this.dockPanel('center', 'preview');
      return;
    }
    const onLeft = this.left ? findTabByComponent(this.left, panelComponent) : null;
    const onRight = this.right ? findTabByComponent(this.right, panelComponent) : null;
    const from: 'left' | 'right' | null = onLeft?.isFloating()
      ? 'left'
      : onRight?.isFloating()
        ? 'right'
        : null;
    if (!from) return;

    if (from === to) {
      this.dockPanel(from, panelComponent);
    } else {
      this.movePanelBetweenSides(panelComponent, to);
    }
  };

  returnAllPanelsToDock = (): void => {
    const sides: FlexSide[] = ['left', 'right', 'center'];
    for (const side of sides) {
      const model = modelForSide(side, this.left, this.right, this.center);
      if (!model) continue;
      for (const component of listFloatingFlexComponents(model)) {
        this.dockPanel(side, component);
      }
    }
    console.info('[flex-dock] returnAllPanelsToDock');
  };

  swapFlexSides = (): void => {
    const left = this.left;
    const right = this.right;
    if (!left || !right) {
      console.warn('[flex-dock] swapFlexSides: model missing');
      return;
    }
    // Exchange models as-is (including floating tabs / OS popouts).
    this.left = right;
    this.right = left;
    this.persistNow();
    this.persistNow();
    this.bump();
  };

  private buildSnapshot(): LayoutStoreSnapshot {
    return {
      version: this.version,
      left: this.left,
      right: this.right,
      center: this.center,
      leftLoading: this.leftLoading,
      rightLoading: this.rightLoading,
      centerLoading: this.centerLoading,
      layoutToast: this.layoutToast,
    };
  }

  private bump(): void {
    this.version += 1;
    this.snapshot = this.buildSnapshot();
    for (const listener of this.listeners) {
      listener();
    }
  }
}

function createLayoutStore(): LayoutStore {
  return new LayoutStore();
}

type HotData = { layoutStore?: LayoutStore };

function resolveLayoutStore(): LayoutStore {
  const hot = import.meta.hot;
  if (hot) {
    // Vite provides hot.data as a mutable bag; vitest may leave it undefined.
    const data = hot.data as HotData | undefined;
    if (data?.layoutStore) {
      return data.layoutStore;
    }
    const store = createLayoutStore();
    hot.dispose(() => {
      const slot = hot.data as HotData | undefined;
      if (slot) slot.layoutStore = store;
    });
    return store;
  }
  return createLayoutStore();
}

/** Module-level singleton — preserved across Vite HMR. */
export const layoutStore: LayoutStore = resolveLayoutStore();

if (import.meta.hot) {
  import.meta.hot.accept();
}

/** Test helper — fresh store (does not replace the module singleton). */
export function createLayoutStoreForTests(): LayoutStore {
  return createLayoutStore();
}

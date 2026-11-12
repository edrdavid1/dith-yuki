/**
 * Maps floated panel component ids / DOM windows → Tauri `flex-popout-*` labels.
 *
 * Close/redock must prefer this over focused/last or ±8px position matching when
 * several OS popouts are open.
 */

const labelByPanelId = new Map<string, string>();
const labelByDomWin = new WeakMap<Window, string>();

export function rememberFlexPopoutLabel(
  panelId: string,
  label: string,
  domWin?: Window | null,
): void {
  labelByPanelId.set(panelId, label);
  if (domWin && !domWin.closed) {
    labelByDomWin.set(domWin, label);
  }
}

export function forgetFlexPopoutLabel(panelId: string): void {
  labelByPanelId.delete(panelId);
}

export function lookupFlexPopoutLabel(
  panelId?: string | null,
  domWin?: Window | null,
): string | null {
  if (domWin && !domWin.closed) {
    const fromDom = labelByDomWin.get(domWin);
    if (fromDom) return fromDom;
  }
  if (panelId) {
    return labelByPanelId.get(panelId) ?? null;
  }
  return null;
}

/** Reverse lookup for W2 Destroyed → dock by panel id. */
export function lookupPanelIdByLabel(label: string): string | null {
  for (const [panelId, bound] of labelByPanelId) {
    if (bound === label) return panelId;
  }
  return null;
}

export function forgetFlexPopoutLabelByWindowLabel(label: string): void {
  const panelId = lookupPanelIdByLabel(label);
  if (panelId) labelByPanelId.delete(panelId);
}

/** Snapshot of panelId → label (W6 invariant checks). */
export function listFlexPopoutRegistryEntries(): Array<{
  panelId: string;
  label: string;
}> {
  return Array.from(labelByPanelId.entries()).map(([panelId, label]) => ({
    panelId,
    label,
  }));
}

/** Labels currently claimed by a panelId (for unbound-shell matching). */
export function listBoundFlexPopoutLabels(): Set<string> {
  return new Set(labelByPanelId.values());
}

/** Clear all panel→label bindings (mass popout close / model replace). */
export function clearFlexPopoutRegistry(): void {
  labelByPanelId.clear();
}

/** @deprecated use clearFlexPopoutRegistry */
export function resetFlexPopoutRegistryForTests(): void {
  clearFlexPopoutRegistry();
}

/**
 * In-memory diagnostic log (`.local-doc/diagnostic.md` §2.5, §10.11).
 * Off until `setPreviewDiagnostics` is called from app config.
 */

export const RING_CAP = 4096;

let level: 0 | 1 | 2 = 0;
const ring: string[] = [];

/** `enabled: false` forces level 0. Enabled levels clamp to 1 or 2. */
export function setPreviewDiagnostics(enabled: boolean, requestedLevel: number): void {
  if (!enabled) {
    level = 0;
    return;
  }
  level = requestedLevel >= 2 ? 2 : 1;
}

export function previewDiagLevel(): 0 | 1 | 2 {
  return level;
}

export function pushDiagLine(line: string): void {
  ring.push(line);
  if (ring.length > RING_CAP) {
    ring.splice(0, ring.length - RING_CAP);
  }
}

export function diagRingLines(): readonly string[] {
  return ring;
}

export function resetDiagnosticsForTests(): void {
  level = 0;
  ring.length = 0;
}

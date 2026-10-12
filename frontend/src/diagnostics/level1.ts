/**
 * Level 1 gesture marks (`.local-doc/diagnostic.md` §5).
 * Every entry point returns before `performance.now` when diagnostics are off.
 */

import { previewDiagLevel, pushDiagLine, resetDiagnosticsForTests } from './session';

export type GestureScenario =
  | 'slider'
  | 'algo_switch'
  | 'pixel_size'
  | 'zoom'
  | 'pan'
  | 'open_doc';

export type ClosedBy = 'all_visible' | 'commit_timeout';

export interface GestureMarks {
  session: string;
  scenario: GestureScenario;
  doc: number | null;
  viewport: string;
  algo: string | null;
  T_input: number;
  T_ipc_sent: number | null;
  T_ipc_done: number | null;
  T_first_post: number | null;
  T_first_decoded: number | null;
  T_first_draw: number | null;
  T_last_decoded: number | null;
  T_last_draw: number | null;
  T_css_pan: number | null;
  update_filter_calls: number;
  set_viewport_calls: number;
  display_tiles: number;
  decoded_tiles: number;
  http_202: number;
  stale_hits: number;
  pyramid_fallbacks: number;
  closed_by: ClosedBy | null;
  expected: string[];
  decodedKeys: string[];
}

export interface Level1Phases {
  debounce_ms: number;
  param_ipc_ms: number;
  backend_wait_ms: number;
  spread_ms: number;
  commit_draw_ms: number;
  total_ms: number;
  css_pan_ms: number | null;
}

interface ActiveGesture extends GestureMarks {
  expectedSet: Set<string>;
  decodedSet: Set<string>;
  ready: ClosedBy | null;
  emitted: boolean;
}

let active: ActiveGesture | null = null;
let seq = 0;

function now(): number {
  return performance.now();
}

function newSession(): string {
  seq += 1;
  const rand = Math.random().toString(16).slice(2, 10);
  return `${Date.now().toString(16)}-${seq.toString(16)}-${rand}`;
}

function blank(scenario: GestureScenario, t: number): ActiveGesture {
  return {
    session: newSession(),
    scenario,
    doc: null,
    viewport: '',
    algo: null,
    T_input: t,
    T_ipc_sent: null,
    T_ipc_done: null,
    T_first_post: null,
    T_first_decoded: null,
    T_first_draw: null,
    T_last_decoded: null,
    T_last_draw: null,
    T_css_pan: null,
    update_filter_calls: 0,
    set_viewport_calls: 0,
    display_tiles: 0,
    decoded_tiles: 0,
    http_202: 0,
    stale_hits: 0,
    pyramid_fallbacks: 0,
    closed_by: null,
    expected: [],
    decodedKeys: [],
    expectedSet: new Set(),
    decodedSet: new Set(),
    ready: null,
    emitted: false,
  };
}

/** Start or continue a gesture. A different in-flight kind is dropped, not merged. */
export function noteUserInput(scenario: GestureScenario, algo?: string | null): void {
  if (previewDiagLevel() === 0) return;
  if (active && !active.emitted && active.scenario === scenario) {
    if (algo) active.algo = algo;
    return;
  }
  active = blank(scenario, now());
  if (algo) active.algo = algo;
}

export function noteIpcSent(which: 'update_filter' | 'set_viewport'): void {
  if (previewDiagLevel() === 0) return;
  if (!active || active.emitted) return;
  const t = now();
  if (active.T_ipc_sent == null) active.T_ipc_sent = t;
  if (which === 'update_filter') active.update_filter_calls += 1;
  else active.set_viewport_calls += 1;
}

export function noteIpcDone(): void {
  if (previewDiagLevel() === 0) return;
  if (!active || active.emitted) return;
  if (active.T_ipc_done == null) active.T_ipc_done = now();
}

/**
 * Visible display tiles posted to the worker. The first post of a gesture
 * fixes the expected set (diagnostic.md §5.1 `display_tiles`).
 */
export function noteDisplayPost(
  keys: readonly string[],
  doc: number,
  zoom: number,
  canvasW: number,
  canvasH: number,
): void {
  if (previewDiagLevel() === 0) return;
  if (!active || active.emitted) return;
  const t = now();
  if (active.T_first_post == null) active.T_first_post = t;
  active.doc = doc;
  active.viewport = `z${zoom.toFixed(2)}_${Math.round(canvasW)}x${Math.round(canvasH)}`;
  if (active.expectedSet.size === 0 && keys.length > 0) {
    for (const key of keys) active.expectedSet.add(key);
    active.display_tiles = active.expectedSet.size;
    active.expected = [...active.expectedSet];
  }
}

/** Pan/zoom found every visible tile already on screen. */
export function noteDisplayCurrent(displayTiles: number): void {
  if (previewDiagLevel() === 0) return;
  if (!active || active.emitted) return;
  const t = now();
  if (active.T_ipc_sent == null) active.T_ipc_sent = t;
  if (active.T_ipc_done == null) active.T_ipc_done = t;
  if (active.T_first_decoded == null) active.T_first_decoded = t;
  if (active.T_last_decoded == null) active.T_last_decoded = t;
  active.display_tiles = displayTiles;
  active.decoded_tiles = displayTiles;
  active.ready = 'all_visible';
}

export function noteCssPan(): void {
  if (previewDiagLevel() === 0) return;
  if (!active || active.emitted || active.scenario !== 'pan') return;
  if (active.T_css_pan == null) active.T_css_pan = now();
}

export interface DecodedDiag {
  retries?: number;
  stale?: boolean;
  pyramidFallback?: boolean;
}

export function noteTileDecoded(key: string, diag?: DecodedDiag): void {
  if (previewDiagLevel() === 0) return;
  if (!active || active.emitted) return;
  if (active.expectedSet.size > 0 && !active.expectedSet.has(key)) return;
  if (active.decodedSet.has(key)) return;
  const t = now();
  active.decodedSet.add(key);
  active.decodedKeys.push(key);
  active.decoded_tiles = active.decodedSet.size;
  if (active.T_first_decoded == null) active.T_first_decoded = t;
  active.T_last_decoded = t;
  const retries = diag?.retries ?? 0;
  if (retries > 0) active.http_202 += retries;
  if (diag?.stale) active.stale_hits += 1;
  if (diag?.pyramidFallback) active.pyramid_fallbacks += 1;
  if (
    active.expectedSet.size > 0 &&
    active.decodedSet.size >= active.expectedSet.size
  ) {
    active.ready = 'all_visible';
  }
}

/** `COMMIT_WAIT_MS` fired. The following paint closes the gesture. */
export function noteCommitTimeout(): void {
  if (previewDiagLevel() === 0) return;
  if (!active || active.emitted) return;
  if (active.ready == null) active.ready = 'commit_timeout';
}

/** Call at the end of the rAF that ran `drawTiles`. */
export function noteFramePainted(): void {
  if (previewDiagLevel() === 0) return;
  if (!active || active.emitted) return;
  const t = now();
  if (active.T_first_decoded != null && active.T_first_draw == null) {
    active.T_first_draw = t;
  }
  if (active.ready == null) return;
  active.T_last_draw = t;
  active.closed_by = active.ready;
  // Open has no param IPC. Treat the input instant as a zero-width IPC so
  // the Level 1 sum still holds (diagnostic.md §5.2).
  if (active.T_ipc_sent == null) active.T_ipc_sent = active.T_input;
  if (active.T_ipc_done == null) active.T_ipc_done = active.T_ipc_sent;
  if (active.T_first_decoded == null) active.T_first_decoded = t;
  if (active.T_last_decoded == null) active.T_last_decoded = active.T_first_decoded;
  emit(active);
  active.emitted = true;
}

export function deriveLevel1(g: GestureMarks): Level1Phases | null {
  if (
    g.T_ipc_sent == null ||
    g.T_ipc_done == null ||
    g.T_first_decoded == null ||
    g.T_last_decoded == null ||
    g.T_last_draw == null
  ) {
    return null;
  }
  return {
    debounce_ms: g.T_ipc_sent - g.T_input,
    param_ipc_ms: g.T_ipc_done - g.T_ipc_sent,
    backend_wait_ms: g.T_first_decoded - g.T_ipc_done,
    spread_ms: g.T_last_decoded - g.T_first_decoded,
    commit_draw_ms: g.T_last_draw - g.T_last_decoded,
    total_ms: g.T_last_draw - g.T_input,
    css_pan_ms: g.T_css_pan == null ? null : g.T_css_pan - g.T_input,
  };
}

function round1(n: number): number {
  return Math.round(n * 10) / 10;
}

export function formatLevel1(g: GestureMarks, phases: Level1Phases): string {
  const body = {
    diag: 'L1',
    side: 'fe',
    session: g.session,
    scenario: g.scenario,
    doc: g.doc,
    viewport: g.viewport,
    algo: g.algo,
    debounce_ms: round1(phases.debounce_ms),
    param_ipc_ms: round1(phases.param_ipc_ms),
    backend_wait_ms: round1(phases.backend_wait_ms),
    spread_ms: round1(phases.spread_ms),
    commit_draw_ms: round1(phases.commit_draw_ms),
    total_ms: round1(phases.total_ms),
    css_pan_ms: phases.css_pan_ms == null ? null : round1(phases.css_pan_ms),
    display_tiles: g.display_tiles,
    decoded_tiles: g.decoded_tiles,
    http_202: g.http_202,
    stale_hits: g.stale_hits,
    pyramid_fallbacks: g.pyramid_fallbacks,
    closed_by: g.closed_by,
    update_filter_calls: g.update_filter_calls,
    set_viewport_calls: g.set_viewport_calls,
  };
  return `[diag/L1] ${JSON.stringify(body)}`;
}

function emit(g: ActiveGesture): void {
  const phases = deriveLevel1(g);
  if (!phases) return;
  const line = formatLevel1(g, phases);
  pushDiagLine(line);
  console.info(line);
}

/** Worker messages gain `diag: true` only while a level is enabled. */
export function stampWorkerMessage<T extends object>(msg: T): T & { diag?: true } {
  if (previewDiagLevel() === 0) return msg;
  return { ...msg, diag: true };
}

export function gestureSnapshot(): GestureMarks | null {
  if (!active) return null;
  return { ...active, expected: [...active.expectedSet], decodedKeys: [...active.decodedSet] };
}

export function resetLevel1ForTests(): void {
  active = null;
  seq = 0;
  resetDiagnosticsForTests();
}

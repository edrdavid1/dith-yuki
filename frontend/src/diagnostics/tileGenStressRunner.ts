/**
 * Live preview stress runner — drives useEffectLayer.updateParams (real debounce).
 */

import { invoke } from '@tauri-apps/api/core';
import {
  beginTraceRun,
  busySnapshot,
  noteTraceEvent,
  tileGenTraceEnabled,
} from './tileGenTrace';
import {
  allDrawn,
  armInjectDrop,
  checkVisibleInvariant,
  getCounters,
  getDrawn,
  injectEnabledFlag,
  lastInjectDroppedKey,
  lastTraceActivityMs,
  noteTraceActivity,
  resetSeriesRegistry,
  setInjectEnabled,
  type Violation,
} from './tileGenCanvasRegistry';

export type StressScenario = 'S1' | 'S2' | 'S3' | 'S5' | 'S8';
/** Any DitherV2 mode string the slider path accepts (CPU + GPU-capable). */
export type StressMode = string;

/** Modes offered in the Stress panel (CPU algos + GPU-capable blocked_fs/blue_noise). */
export const STRESS_MODES: readonly string[] = [
  // Ordered / Path B (CPU-resident)
  'bayer_2x2',
  'bayer_4x4',
  'bayer_8x8',
  'bayer_16x16',
  'clustered_dot_ordered',
  'dispersed_dot_ordered',
  'halftone_screen_angled',
  // Error diffusion (CPU)
  'floyd_steinberg',
  'atkinson',
  'jarvis_judice_ninke',
  'stucki',
  'burkes',
  'fan93',
  'ostromoukhov',
  'sierra',
  'sierra_lite',
  'sierra_two_row',
  'shiau_fan',
  'stevenson_arce',
  'zhou_fang',
  'riemersma',
  // Other CPU
  'void_and_cluster',
  'crosshatch_dither',
  'line_screen',
  'voronoi_stipple',
  'random_dot_stipple',
  'cmyk_halftone',
  'wave',
  // GPU-capable (also run on CPU when path=cpu)
  'blocked_fs',
  'blue_noise',
] as const;

/** Live path override via `tile_gen_trace_set_compute_path`. */
export type StressComputePath = 'auto' | 'cpu' | 'gpu';

export type StressConfig = {
  mode: StressMode;
  scenario: StressScenario;
  intervalMs: number;
  changes: number;
  series: number;
  seed: number;
  /** Force CPU / GPU dither path for this campaign. */
  computePath?: StressComputePath;
  /** Current dither params baseline (from effect layer). */
  baseParams: Record<string, unknown>;
  updateParams: (patch: Record<string, unknown>) => void;
  /** Visible tile keys at check time. */
  getVisibleKeys: () => string[];
  getViewportLevel: () => number;
  layerId: number;
  /** Optional: pan/zoom for S5. */
  onPanZoom?: (step: number) => void;
};

export async function setComputePath(path: StressComputePath): Promise<void> {
  await invoke('tile_gen_trace_set_compute_path', { path });
  noteTraceEvent('compute_path_set', { details: { path } });
}

export type SeriesReport = {
  runId: number;
  scenario: StressScenario;
  seed: number;
  silence: 'ok' | 'STUCK_BUSY';
  violations: Violation[];
  injectKey?: string | null;
  s8?: { afterNoop: Violation[]; afterChange: Violation[] };
};

function nextF64(seed: { v: number }): number {
  // xorshift32
  let x = seed.v >>> 0;
  x ^= x << 13;
  x ^= x >>> 17;
  x ^= x << 5;
  seed.v = x >>> 0;
  return seed.v / 0xffffffff;
}

function jitterMs(seed: { v: number }, nominal: number): number {
  if (nominal <= 0) return 0;
  const f = 0.5 + nextF64(seed);
  return Math.round(nominal * f);
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

async function probeInject(): Promise<boolean> {
  try {
    return await invoke<boolean>('tile_gen_trace_inject_enabled');
  } catch {
    return false;
  }
}

async function fetchLayerGen(layerId: number): Promise<{ doc_gen: number; layer_gen: number } | null> {
  try {
    const v = await invoke<{ doc_gen: number; layer_gen: number }>('tile_gen_trace_layer_gen', {
      req: { layerId },
    });
    return v;
  } catch {
    return null;
  }
}

/** Silence: backend idle + 1500ms with no FE/backend trace activity we can see. */
export async function waitCanvasSilence(
  timeoutMs = 30_000,
  settleMs = 1500,
): Promise<'ok' | 'STUCK_BUSY'> {
  const start = performance.now();
  while (performance.now() - start < timeoutMs) {
    const snap = await busySnapshot();
    const idleFe = performance.now() - lastTraceActivityMs() >= settleMs || lastTraceActivityMs() === 0;
    if (snap?.silent && idleFe) {
      await sleep(settleMs);
      const again = await busySnapshot();
      const idleFe2 = performance.now() - lastTraceActivityMs() >= settleMs;
      if (again?.silent && idleFe2) return 'ok';
    }
    await sleep(50);
  }
  return 'STUCK_BUSY';
}

function patchForStep(
  mode: StressMode,
  base: Record<string, unknown>,
  step: number,
  scenario: StressScenario,
): Record<string, unknown> {
  const thr = 0.8 + step * 0.02;
  return {
    ...base,
    mode,
    threshold_scale: thr,
    // Keep identity fields if present
    type: base.type ?? 'DitherV2',
    levels: base.levels ?? 16,
    pixel_size: base.pixel_size ?? 1,
    ...(scenario === 'S2' && step === 0 ? { threshold_scale: 1.0 } : {}),
    ...(scenario === 'S2' && step === 1 ? { threshold_scale: 1.2 } : {}),
    ...(scenario === 'S2' && step === 2 ? { threshold_scale: 1.0 } : {}),
  };
}

async function runParamSeries(
  cfg: StressConfig,
  seed: { v: number },
  changes: number,
): Promise<void> {
  if (cfg.scenario === 'S2') {
    cfg.updateParams(patchForStep(cfg.mode, cfg.baseParams, 0, 'S2'));
    await sleep(jitterMs(seed, cfg.intervalMs));
    cfg.updateParams(patchForStep(cfg.mode, cfg.baseParams, 1, 'S2'));
    await sleep(jitterMs(seed, cfg.intervalMs));
    cfg.updateParams(patchForStep(cfg.mode, cfg.baseParams, 2, 'S2'));
    return;
  }
  for (let i = 0; i < changes; i++) {
    cfg.updateParams(patchForStep(cfg.mode, cfg.baseParams, i, cfg.scenario));
    if (cfg.scenario === 'S5' && cfg.onPanZoom) {
      cfg.onPanZoom(i);
    }
    const wait = jitterMs(seed, cfg.intervalMs);
    if (wait > 0) await sleep(wait);
  }
}

export async function runStressCampaign(cfg: StressConfig): Promise<SeriesReport[]> {
  if (!tileGenTraceEnabled()) {
    throw new Error('DITHER_TRACE not enabled');
  }
  if (cfg.computePath) {
    await setComputePath(cfg.computePath);
  }
  const inject = await probeInject();
  setInjectEnabled(inject);

  const reports: SeriesReport[] = [];
  const seedState = { v: (cfg.seed || 1) >>> 0 };

  for (let s = 0; s < cfg.series; s++) {
    resetSeriesRegistry();
    noteTraceActivity();
    const runId =
      (await beginTraceRun(`${cfg.scenario}-${cfg.mode}`, seedState.v)) ?? s + 1;

    if (inject && (cfg.scenario === 'S8' || cfg.scenario === 'S1')) {
      armInjectDrop(1);
    }

    await runParamSeries(cfg, seedState, cfg.changes);
    const silence = await waitCanvasSilence();
    if (silence === 'STUCK_BUSY') {
      noteTraceEvent('violation', {
        details: { class: 'STUCK_BUSY', scenario: cfg.scenario, series: s },
      });
    }

    const gens = await fetchLayerGen(cfg.layerId);
    // Spec: compare to layer_gen from backend; doc_gen is also useful for composite.
    const expectedGen = gens?.doc_gen ?? gens?.layer_gen ?? -1;
    const visible = cfg.getVisibleKeys();
    let violations = checkVisibleInvariant({
      visibleKeys: visible,
      expectedGen,
      level: cfg.getViewportLevel(),
    });
    if (silence === 'STUCK_BUSY') {
      violations = [
        ...violations,
        { class: 'STUCK_BUSY', key: '*', detail: 'silence timeout 30s', expectedGen },
      ];
    }

    const report: SeriesReport = {
      runId,
      scenario: cfg.scenario,
      seed: seedState.v,
      silence,
      violations,
      injectKey: lastInjectDroppedKey(),
    };

    if (cfg.scenario === 'S8' && inject) {
      // Same params again (no-op path)
      cfg.updateParams(patchForStep(cfg.mode, cfg.baseParams, cfg.changes - 1, 'S1'));
      await waitCanvasSilence();
      const afterNoop = checkVisibleInvariant({
        visibleKeys: cfg.getVisibleKeys(),
        expectedGen: (await fetchLayerGen(cfg.layerId))?.doc_gen ?? expectedGen,
        level: cfg.getViewportLevel(),
      });
      // Different value
      cfg.updateParams(patchForStep(cfg.mode, cfg.baseParams, cfg.changes + 5, 'S1'));
      await waitCanvasSilence();
      const afterChange = checkVisibleInvariant({
        visibleKeys: cfg.getVisibleKeys(),
        expectedGen: (await fetchLayerGen(cfg.layerId))?.doc_gen ?? expectedGen,
        level: cfg.getViewportLevel(),
      });
      report.s8 = { afterNoop, afterChange };
    }

    noteTraceEvent('series_report', {
      details: {
        scenario: cfg.scenario,
        violations: report.violations,
        injectKey: report.injectKey,
        s8: report.s8,
        drawn: Object.fromEntries(
          [...allDrawn().entries()].map(([k, v]) => [k, v.gen]),
        ),
        countersSample: Object.fromEntries(
          visible.slice(0, 8).map((k) => [k, getCounters(k) ?? null]),
        ),
      },
    });

    reports.push(report);
    // Advance seed
    nextF64(seedState);
  }

  return reports;
}

export function summarizeReports(reports: SeriesReport[]): string {
  const classes = [
    'MISSING',
    'STALE',
    'MIXED',
    'STUCK_202',
    'LOST_EVENT',
    'STUCK_BUSY',
    'STALE_DRAWN',
  ] as const;
  const lines: string[] = ['# Live canvas stress report', ''];
  lines.push(`series=${reports.length} inject=${injectEnabledFlag()}`, '');
  lines.push('| series | scenario | silence | violations |');
  lines.push('| ---: | --- | --- | --- |');
  for (const r of reports) {
    const v = r.violations.map((x) => x.class).join(',') || 'none';
    lines.push(`| ${r.runId} | ${r.scenario} | ${r.silence} | ${v} |`);
  }
  lines.push('', '## Counts by class');
  for (const c of classes) {
    const n = reports.reduce(
      (acc, r) => acc + r.violations.filter((v) => v.class === c).length,
      0,
    );
    const seriesWith = reports.filter((r) => r.violations.some((v) => v.class === c)).length;
    lines.push(`- ${c}: events=${n} series_with=${seriesWith}/${reports.length}`);
  }
  const s8 = reports.find((r) => r.s8);
  if (s8?.s8) {
    lines.push('', '## S8');
    lines.push(
      `- after noop violations: ${s8.s8.afterNoop.map((v) => v.class).join(',') || 'none'}`,
    );
    lines.push(
      `- after change violations: ${s8.s8.afterChange.map((v) => v.class).join(',') || 'none'}`,
    );
    lines.push(`- inject key: ${s8.injectKey ?? 'none'}`);
  }
  if (injectEnabledFlag()) {
    lines.push('', '## Self-check (inject)');
    for (const r of reports) {
      if (!r.injectKey) continue;
      const hit = r.violations.some(
        (v) =>
          (v.class === 'MISSING' || v.class === 'LOST_EVENT') &&
          (v.key === r.injectKey || v.detail.includes(r.injectKey!)),
      );
      lines.push(
        `- run ${r.runId}: inject=${r.injectKey} checker_hit=${hit} violations=${r.violations.map((v) => `${v.class}:${v.key}`).join('; ')}`,
      );
    }
  }
  // Sample drawn
  const sample = reports[0];
  if (sample) {
    lines.push('', '## Sample drawn gens (first series)');
    for (const [k, rec] of [...allDrawn().entries()].slice(0, 12)) {
      lines.push(`- ${k} gen=${rec.gen} via202=${rec.via202 ?? false}`);
    }
  }
  void getDrawn;
  return lines.join('\n');
}

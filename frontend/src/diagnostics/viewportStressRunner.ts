/**
 * Z1–Z7 viewport stress via the same camera handlers as wheel/controls.
 */

import { invoke } from '@tauri-apps/api/core';
import {
  beginTraceRun,
  noteTraceEvent,
  subscribeTraceFeEvents,
  tileGenTraceEnabled,
  waitForSilence,
  type TraceCameraApi,
} from './tileGenTrace';
import { armInjectSkipVpSend } from '../hooks/useViewport';
import {
  setComputePath,
  type StressComputePath,
} from './tileGenStressRunner';
import {
  emptySeriesTrace,
  feSnapshotFromTiles,
  ingestFeEventIntoSeriesTrace,
  runQuietThenCheck,
  type SeriesTileTrace,
  type VpViolation,
} from './viewportTraceChecker';

function attachSeriesTrace(): { trace: SeriesTileTrace; detach: () => void } {
  const trace = emptySeriesTrace();
  const detach = subscribeTraceFeEvents((event, opts) => {
    ingestFeEventIntoSeriesTrace(trace, event, opts);
  });
  return { trace, detach };
}

export type ZoomScenario =
  | 'Z1'
  | 'Z2'
  | 'Z3'
  | 'Z4'
  | 'Z5'
  | 'Z6'
  | 'Z7'
  | 'Z9'
  | 'Z_INJECT';

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

function camera(): TraceCameraApi {
  const c = window.__ditherTileGenTrace?.camera;
  if (!c) throw new Error('camera API not registered (PreviewFeature)');
  return c;
}

async function injectEnabled(): Promise<boolean> {
  try {
    return await invoke<boolean>('tile_gen_trace_inject_enabled');
  } catch {
    return false;
  }
}

export type ZoomSeriesReport = {
  scenario: ZoomScenario;
  series: number;
  silence: 'ok' | 'STUCK_BUSY' | 'STUCK_MOVING';
  violations: VpViolation[];
  /** Present after quiet check (Z3 and others). */
  timeToFreshMs?: number | null;
  holes?: number;
  visible?: number;
};

export async function runZoomScenario(args: {
  scenario: ZoomScenario;
  series: number;
  seed: number;
  /** Override; Z1 defaults to cpu, Z2 to gpu. */
  computePath?: StressComputePath;
  /** Optional: apply dither params before zoom-out (Z1/Z2). */
  applyFilter?: () => Promise<void> | void;
  /** Optional: no-op / change params for Z7. */
  noopParams?: () => Promise<void> | void;
  changeParams?: () => Promise<void> | void;
}): Promise<ZoomSeriesReport[]> {
  if (!tileGenTraceEnabled()) throw new Error('DITHER_TRACE not enabled');
  const reports: ZoomSeriesReport[] = [];
  const cam = camera();

  const pathForScenario = (): StressComputePath => {
    if (args.computePath) return args.computePath;
    if (args.scenario === 'Z1' || args.scenario === 'Z3') return 'cpu';
    if (args.scenario === 'Z2') return 'gpu';
    return 'auto';
  };

  for (let s = 0; s < args.series; s++) {
    const path = pathForScenario();
    await setComputePath(path);
    const runId = await beginTraceRun(args.scenario, args.seed + s);
    noteTraceEvent('run_begin', {
      runId,
      details: {
        scenario: args.scenario,
        series: s,
        seed: args.seed + s,
        compute_path: path,
      },
    });

    const jitter = 40 + ((args.seed + s) % 80);

    const { trace, detach } = attachSeriesTrace();
    try {
      if (args.scenario === 'Z_INJECT') {
        if (!(await injectEnabled())) {
          const violations = [
            {
              class: 'INJECT_SELFCHECK_FAILED' as const,
              detail: 'DITHER_TRACE_INJECT not set — Z_INJECT is not a clean pass',
            },
          ];
          noteTraceEvent('inject_selfcheck', {
            details: { skipped: true, reason: 'DITHER_TRACE_INJECT not set', failed: true },
          });
          reports.push({
            scenario: args.scenario,
            series: s,
            silence: 'ok',
            violations,
          });
          continue;
        }
        armInjectSkipVpSend(1);
        cam.setZoom(Math.max(0.125, cam.getZoom() * 0.5));
        await sleep(jitter);
        // Do not flush: inject skip must stay in the ledger as vp_not_sent.
        const { silence, violations: base } = await runQuietThenCheck({
          getFeSnapshot: () =>
            feSnapshotFromTiles(cam.getLevel(), cam.getVisibleTiles(), cam.getTileRev()),
          trace,
        });
        const hitVp = base.some((v) => v.class === 'VP_NOT_SENT');
        const selfOk = trace.injectSkipSeen && hitVp;
        const violations: VpViolation[] = selfOk
          ? // Expected VP_NOT_SENT proves the checker; strip it from the pass set.
            base.filter((v) => v.class !== 'VP_NOT_SENT')
          : [
              ...base,
              ...(!trace.injectSkipSeen
                ? [
                    {
                      class: 'INJECT_SELFCHECK_FAILED' as const,
                      detail: 'expected vp_not_sent reason=inject_skip in ledger',
                    },
                  ]
                : []),
              ...(!hitVp
                ? [
                    {
                      class: 'INJECT_SELFCHECK_FAILED' as const,
                      detail: 'expected VP_NOT_SENT violation after inject skip',
                    },
                  ]
                : []),
            ];
        noteTraceEvent('inject_selfcheck', {
          details: {
            hit_VP_NOT_SENT: hitVp,
            inject_skip_seen: trace.injectSkipSeen,
            selfcheck_ok: selfOk,
            violations: violations.map((v) => v.class),
          },
        });
        reports.push({ scenario: args.scenario, series: s, silence, violations });
        continue;
      }

      // Shared: zoom in
      cam.setZoom(2);
      await sleep(jitter);
      await waitForSilence(15_000);

      if (args.scenario === 'Z1' || args.scenario === 'Z2' || args.scenario === 'Z6') {
        if (args.applyFilter) await args.applyFilter();
        await waitForSilence(120_000);
      }

      let freshnessT0: number | undefined;
      if (args.scenario === 'Z9') {
        // H4 race: 5–10 quick param changes over ~300–500 ms, then quiet check.
        if (!args.changeParams) {
          throw new Error('Z9 requires changeParams (threshold / dither slider)');
        }
        if (args.applyFilter) await args.applyFilter();
        await waitForSilence(60_000);
        const bursts = 8;
        const gapMs = 50; // 8 × 50 = 400 ms window
        noteTraceEvent('z9_burst_begin', {
          details: { bursts, gap_ms: gapMs, window_ms: bursts * gapMs },
        });
        for (let i = 0; i < bursts; i++) {
          await args.changeParams();
          noteTraceEvent('z9_param_pulse', { details: { i, gap_ms: gapMs } });
          await sleep(gapMs);
        }
        noteTraceEvent('z9_burst_end', { details: { bursts } });
        freshnessT0 = performance.now();
        // Stay at current zoom — isolate param-change race (pan/zoom already heals).
      } else if (args.scenario === 'Z3') {
        // Cold Processed/Composite (keep Raw); prove via `cache_cold_start` in trace.
        try {
          await invoke('tile_gen_trace_cold_cache');
          noteTraceEvent('z3_cold_cache_ok', { details: { via: 'tile_gen_trace_cold_cache' } });
        } catch (e) {
          noteTraceEvent('z3_cold_cache_failed', {
            details: { error: String(e) },
          });
        }
        // Visible set over the document bottom-right before mid-pass zooms.
        cam.panToBottomRight?.();
        cam.flushViewport?.();
        await sleep(50);
        noteTraceEvent('z3_viewport_corner', {
          details: {
            corner: 'bottom_right',
            visible: cam.getVisibleTiles(),
            level: cam.getLevel(),
            zoom: cam.getZoom(),
          },
        });
        if (args.applyFilter) void args.applyFilter();
        await sleep(30);
        cam.setZoom(0.5);
        cam.panToBottomRight?.();
        await sleep(jitter);
        cam.setZoom(2);
        cam.panToBottomRight?.();
        await sleep(jitter);
        // Freshness clock: final zoom-out (cold pyramid level under load).
        freshnessT0 = performance.now();
        cam.setZoom(0.25);
        cam.panToBottomRight?.();
        cam.flushViewport?.();
      } else if (args.scenario === 'Z5') {
        // no filter
        freshnessT0 = performance.now();
        cam.setZoom(0.25);
      } else {
        freshnessT0 = performance.now();
        cam.setZoom(0.25);
      }

      await sleep(jitter);
      const quietArgs = {
        getFeSnapshot: () =>
          feSnapshotFromTiles(cam.getLevel(), cam.getVisibleTiles(), cam.getTileRev()),
        trace,
        flushViewport: () => cam.flushViewport?.(),
        freshnessT0,
        // 12 MPix cold ED zoom-out budget from Step 1 baseline (~2s L0 + headroom).
        timeToFreshBudgetMs: args.scenario === 'Z3' ? 30_000 : undefined,
      };

      let { silence, violations, freshness } = await runQuietThenCheck(quietArgs);

      if (args.scenario === 'Z4' && violations.length > 0) {
        cam.panBy(8, 0);
        await sleep(jitter);
        ({ silence, violations, freshness } = await runQuietThenCheck(quietArgs));
      }

      if (args.scenario === 'Z7' && violations.length > 0) {
        if (args.noopParams) await args.noopParams();
        await waitForSilence(10_000);
        let r = await runQuietThenCheck(quietArgs);
        noteTraceEvent('z7_after_noop', { details: { n: r.violations.length } });
        if (args.changeParams) await args.changeParams();
        await waitForSilence(10_000);
        r = await runQuietThenCheck(quietArgs);
        noteTraceEvent('z7_after_slider', { details: { n: r.violations.length } });
        cam.panBy(6, 4);
        await sleep(jitter);
        r = await runQuietThenCheck(quietArgs);
        noteTraceEvent('z7_after_pan', { details: { n: r.violations.length } });
        cam.setZoom(2);
        await sleep(jitter);
        r = await runQuietThenCheck(quietArgs);
        noteTraceEvent('z7_after_restore_scale', { details: { n: r.violations.length } });
        silence = r.silence;
        violations = r.violations;
        freshness = r.freshness;
      }

      noteTraceEvent('series_trace_summary', {
        details: {
          requested: trace.requested.size,
          response200: trace.response200.size,
          stuck: trace.responseStuck.size,
          drawn: trace.drawn.size,
          sent_seqs: trace.sentSeqs.length,
          violation_count: violations.length,
          classes: violations.map((v) => v.class),
          time_to_fresh_ms: freshness.timeToFreshMs,
          holes: freshness.holes,
          visible: freshness.visible,
        },
      });

      reports.push({
        scenario: args.scenario,
        series: s,
        silence,
        violations,
        timeToFreshMs: freshness.timeToFreshMs,
        holes: freshness.holes,
        visible: freshness.visible,
      });
    } finally {
      detach();
    }
  }

  return reports;
}

export function summarizeZoomReports(reports: ZoomSeriesReport[]): string {
  const byClass = new Map<string, number>();
  let withV = 0;
  let stuck = 0;
  const freshSamples: number[] = [];
  let holeSum = 0;
  for (const r of reports) {
    if (r.silence !== 'ok') stuck += 1;
    if (r.violations.length) withV += 1;
    if (typeof r.timeToFreshMs === 'number') freshSamples.push(r.timeToFreshMs);
    holeSum += r.holes ?? 0;
    for (const v of r.violations) {
      byClass.set(v.class, (byClass.get(v.class) ?? 0) + 1);
    }
  }
  const classes = [...byClass.entries()].map(([k, n]) => `${k}=${n}`).join(' ') || 'none';
  const stuckBit = stuck ? `, ${stuck} not-quiet (${reports.filter((r) => r.silence !== 'ok').map((r) => r.silence).join('|')})` : '';
  const freshBit =
    freshSamples.length > 0
      ? `, TIME_TO_FRESH ms=[${freshSamples.map((n) => n.toFixed(0)).join(',')}] holes_sum=${holeSum}`
      : holeSum > 0
        ? `, holes_sum=${holeSum}`
        : '';
  return `${reports.length} series, ${withV} with violations${stuckBit}${freshBit}; ${classes}`;
}

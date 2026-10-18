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

export type ZoomScenario = 'Z1' | 'Z2' | 'Z3' | 'Z4' | 'Z5' | 'Z6' | 'Z7' | 'Z_INJECT';

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

      if (args.scenario === 'Z3') {
        if (args.applyFilter) void args.applyFilter();
        await sleep(30);
        cam.setZoom(0.5);
        await sleep(jitter);
        cam.setZoom(2);
        await sleep(jitter);
        cam.setZoom(0.25);
      } else if (args.scenario === 'Z5') {
        // no filter
        cam.setZoom(0.25);
      } else {
        cam.setZoom(0.25);
      }

      await sleep(jitter);
      const quietArgs = {
        getFeSnapshot: () =>
          feSnapshotFromTiles(cam.getLevel(), cam.getVisibleTiles(), cam.getTileRev()),
        trace,
        flushViewport: () => cam.flushViewport?.(),
      };

      let { silence, violations } = await runQuietThenCheck(quietArgs);

      if (args.scenario === 'Z4' && violations.length > 0) {
        cam.panBy(8, 0);
        await sleep(jitter);
        ({ silence, violations } = await runQuietThenCheck(quietArgs));
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
        },
      });

      reports.push({ scenario: args.scenario, series: s, silence, violations });
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
  for (const r of reports) {
    if (r.silence !== 'ok') stuck += 1;
    if (r.violations.length) withV += 1;
    for (const v of r.violations) {
      byClass.set(v.class, (byClass.get(v.class) ?? 0) + 1);
    }
  }
  const classes = [...byClass.entries()].map(([k, n]) => `${k}=${n}`).join(' ') || 'none';
  const stuckBit = stuck ? `, ${stuck} not-quiet (${reports.filter((r) => r.silence !== 'ok').map((r) => r.silence).join('|')})` : '';
  return `${reports.length} series, ${withV} with violations${stuckBit}; ${classes}`;
}

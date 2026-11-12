import { describe, expect, it } from 'vitest';
import {
  EXIT_SLACK_PX,
  FLEX_CENTER_DROP_MAX,
  FLEX_CENTER_DROP_MIN,
  FLEX_EDGE_DOCK_PX,
  MAX_SIDE_COLUMNS,
} from '../gestureConfig';
import { MAX_SIDE_COLUMNS as dockMax } from '../horizontalDock';

describe('gestureConfig', () => {
  it('keeps EXIT_SLACK larger than the patched edge-dock strip', () => {
    expect(EXIT_SLACK_PX).toBeGreaterThan(FLEX_EDGE_DOCK_PX);
  });

  it('exposes MAX_SIDE_COLUMNS for dock policy (single source)', () => {
    expect(MAX_SIDE_COLUMNS).toBe(2);
    expect(dockMax).toBe(MAX_SIDE_COLUMNS);
  });

  it('documents the patched center drop band', () => {
    expect(FLEX_CENTER_DROP_MIN).toBe(0.38);
    expect(FLEX_CENTER_DROP_MAX).toBe(0.62);
  });
});

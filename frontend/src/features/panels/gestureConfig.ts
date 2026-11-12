/**
 * W9: single source of truth for titlebar / dock gesture constants.
 *
 * Changing EXIT_SLACK / arm thresholds here changes undock behavior without
 * edits elsewhere. Edge-dock / center-band values must stay in sync with
 * `patches/flexlayout-react+0.7.15.patch` (FE cannot override stock FL geometry).
 */

/**
 * Past this gap outside the column, the gesture is float (not side-dock).
 * Must stay comfortably larger than {@link FLEX_EDGE_DOCK_PX} so a side-place
 * drag that overshoots the column edge does not instantly undock.
 */
export const EXIT_SLACK_PX = 64;

/** Minimum pointer travel before sidebar undock tracking arms. */
export const UNDOCK_ARM_THRESHOLD_PX = 8;

/** Preview (center) undock — short drag in any direction. */
export const PREVIEW_UNDOCK_DISTANCE_PX = 12;

/**
 * Hard cap on side-by-side columns per sidebar.
 * Equals floor(SIDEBAR_EXPANDED_MAX_WIDTH / DOCK_TABSET_MIN_WIDTH) = floor(600/240).
 */
export const MAX_SIDE_COLUMNS = 2;

/**
 * Widened FlexLayout edge-dock strip (stock was 10).
 * Must match patch: RowNode `margin` / Layout `edgeRectWidth`.
 */
export const FLEX_EDGE_DOCK_PX = 40;

/**
 * Narrowed center drop band (stock was 0.25–0.75).
 * Must match patch: DockLocation center test.
 */
export const FLEX_CENTER_DROP_MIN = 0.38;
export const FLEX_CENTER_DROP_MAX = 0.62;

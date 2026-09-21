/**
 * Which edges of the main app window a surface touches.
 * Layout (AppLayout) owns this; CSS only styles `data-app-edges`.
 */
export type AppEdge = 'top' | 'right' | 'bottom' | 'left';

/**
 * Build a space-separated `data-app-edges` value.
 * Falsy entries are skipped so callers can write `leftW === 0 && 'left'`.
 */
export function appEdgesAttr(
  ...edges: Array<AppEdge | false | null | undefined>
): string | undefined {
  const list = edges.filter((e): e is AppEdge => typeof e === 'string');
  return list.length > 0 ? list.join(' ') : undefined;
}

/** Inline corner radii for hosts that touch the app window (backs up CSS). */
export function appEdgesCornerStyle(
  edges: string | undefined,
): { borderBottomLeftRadius?: string; borderBottomRightRadius?: string } {
  if (!edges) return {};
  const tokens = new Set(edges.split(/\s+/));
  if (!tokens.has('bottom')) return {};
  return {
    ...(tokens.has('left')
      ? { borderBottomLeftRadius: 'var(--dock-outer-radius)' }
      : {}),
    ...(tokens.has('right')
      ? { borderBottomRightRadius: 'var(--dock-outer-radius)' }
      : {}),
  };
}

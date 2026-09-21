/** Extensions accepted for drag-open from the welcome / preview empty state. */
const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'webp']);

export type DroppedFileKind = 'image' | 'project';

export function classifyDroppedPath(path: string): DroppedFileKind | null {
  const base = path.replace(/\\/g, '/').split('/').pop() ?? path;
  const lower = base.toLowerCase();
  if (lower.endsWith('.dyproj')) return 'project';
  const dot = lower.lastIndexOf('.');
  if (dot < 0) return null;
  const ext = lower.slice(dot + 1);
  if (IMAGE_EXTENSIONS.has(ext)) return 'image';
  return null;
}

/**
 * Open each dropped path with the matching helper. Skips unsupported files.
 * Returns how many paths were opened.
 */
export async function openDroppedPaths(
  paths: string[],
  helpers: {
    openImageAt: (path: string) => void | Promise<void>;
    openProjectAt: (path: string) => void | Promise<void>;
  },
): Promise<number> {
  let opened = 0;
  for (const path of paths) {
    const kind = classifyDroppedPath(path);
    if (kind === 'image') {
      await helpers.openImageAt(path);
      opened += 1;
    } else if (kind === 'project') {
      await helpers.openProjectAt(path);
      opened += 1;
    }
  }
  return opened;
}

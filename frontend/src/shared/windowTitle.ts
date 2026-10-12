import { APP_NAME } from './appMeta';
import { projectBasename } from './unsavedGuard';
import type { Workspace } from './workspace/visibility';

/** App name in the window chrome; Gamedev appends a mode suffix. */
export function appNameForWorkspace(workspace?: Workspace | null): string {
  return workspace === 'gamedev' ? `${APP_NAME}\u2003\u2003▪\u2003\u2003Game Dev` : APP_NAME;
}

export function windowChromeTitle(opts: {
  dirty: boolean;
  hasDocument: boolean;
  projectPath: string | null;
  sourcePath: string | null;
  appName?: string;
  workspace?: Workspace | null;
}): string {
  const appName = opts.appName ?? appNameForWorkspace(opts.workspace);
  const bullet = opts.hasDocument && opts.dirty ? '* ' : '';
  const filePath = opts.projectPath ?? opts.sourcePath;
  const fileName = filePath ? projectBasename(filePath) : null;
  if (fileName && fileName !== 'Untitled') {
    return `${bullet}${fileName} — ${appName}`;
  }
  return `${bullet}${appName}`;
}

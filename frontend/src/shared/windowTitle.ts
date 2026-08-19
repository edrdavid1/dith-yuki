import { APP_NAME } from './appMeta';
import { projectBasename } from './unsavedGuard';

export function windowChromeTitle(opts: {
  dirty: boolean;
  hasDocument: boolean;
  projectPath: string | null;
  sourcePath: string | null;
  appName?: string;
}): string {
  const appName = opts.appName ?? APP_NAME;
  const bullet = opts.hasDocument && opts.dirty ? '* ' : '';
  const filePath = opts.projectPath ?? opts.sourcePath;
  const fileName = filePath ? projectBasename(filePath) : null;
  if (fileName && fileName !== 'Untitled') {
    return `${bullet}${fileName} — ${appName}`;
  }
  return `${bullet}${appName}`;
}

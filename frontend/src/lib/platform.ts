// src/lib/platform.ts
import { platform } from '@tauri-apps/plugin-os';

export type PlatformValue = 'macos' | 'windows' | 'linux' | 'unknown';

let resolvedPlatform: PlatformValue = 'unknown';

/**
 * Initialize platform detection. Must be called (and awaited) before
 * the React root renders. Resolves within 500ms or falls back to 'unknown'.
 */
export async function initPlatform(): Promise<void> {
  try {
    const result = await Promise.race([
      platform(),
      new Promise<string>((_, reject) =>
        setTimeout(() => reject(new Error('timeout')), 500)
      ),
    ]);
    if (result === 'macos' || result === 'windows' || result === 'linux') {
      resolvedPlatform = result;
    } else {
      resolvedPlatform = 'unknown';
    }
    document.documentElement.dataset.platform = resolvedPlatform;
  } catch {
    resolvedPlatform = 'unknown';
  }
  document.documentElement.dataset.platform = resolvedPlatform;
}

/**
 * Returns the current platform synchronously. Must be called after initPlatform().
 */
export function getPlatform(): PlatformValue {
  return resolvedPlatform;
}

/** Convenience checks */
export function isMacOS(): boolean {
  return resolvedPlatform === 'macos';
}
export function isWindows(): boolean {
  return resolvedPlatform === 'windows';
}
export function isLinux(): boolean {
  return resolvedPlatform === 'linux';
}

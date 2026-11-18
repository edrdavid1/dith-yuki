import { invoke } from '@tauri-apps/api/core';
import { open as pluginOpen, save as pluginSave } from '@tauri-apps/plugin-dialog';

/**
 * Store the security-scoped bookmark while the panel grant is still alive.
 * Direct builds no-op inside Rust when macOS will not issue a bookmark.
 */
async function rememberPicked(selected: string | string[] | null | undefined): Promise<void> {
  const paths = Array.isArray(selected) ? selected : selected ? [selected] : [];
  for (const path of paths) {
    if (!path) continue;
    try {
      await invoke('remember_user_file', { path });
    } catch {
      // Non-macOS and unsigned dev builds have nothing to store.
    }
  }
}

/** File dialogs that also record a sandbox bookmark for the chosen path. */
export async function openDialog(
  options?: Parameters<typeof pluginOpen>[0],
): Promise<Awaited<ReturnType<typeof pluginOpen>>> {
  const selected = await pluginOpen(options);
  if (typeof selected === 'string' || Array.isArray(selected)) {
    await rememberPicked(selected);
  }
  return selected;
}

export async function saveDialog(
  options?: Parameters<typeof pluginSave>[0],
): Promise<Awaited<ReturnType<typeof pluginSave>>> {
  const selected = await pluginSave(options);
  if (typeof selected === 'string') {
    await rememberPicked(selected);
  }
  return selected;
}

export { openDialog as open, saveDialog as save };

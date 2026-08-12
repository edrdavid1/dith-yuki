import { open, save } from '@tauri-apps/plugin-dialog';

/** Thin wrappers over `@tauri-apps/plugin-dialog` for the IPC_Layer boundary. */
export const openDialog = open;
export const saveDialog = save;

export { open, save };

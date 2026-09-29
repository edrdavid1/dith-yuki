import { writeText } from '@tauri-apps/plugin-clipboard-manager';

/** Write plain text via Tauri clipboard-manager (requires ACL allow-write-text). */
export async function copyTextToClipboard(text: string): Promise<void> {
  try {
    await writeText(text);
  } catch (err) {
    console.error('clipboard write failed', err);
    throw err;
  }
}

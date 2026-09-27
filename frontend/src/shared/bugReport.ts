import { getVersion } from '@tauri-apps/api/app';
import { arch, platform, version as osVersion } from '@tauri-apps/plugin-os';

/** Public Web3Forms access key (frontend-safe; routes mail to the project inbox). */
export const WEB3FORMS_ACCESS_KEY = '5040e9f8-75b7-4a81-9838-a5de345bab04';

export const WEB3FORMS_ENDPOINT = 'https://api.web3forms.com/submit';

export interface BugReportEnv {
  appVersion: string;
  os: string;
  arch: string;
  osVersion: string;
  /** Human-readable line shown to the user and emailed. */
  summary: string;
}

export async function gatherBugReportEnv(): Promise<BugReportEnv> {
  const [appVersion, os, cpuArch, osVer] = await Promise.all([
    getVersion(),
    platform(),
    arch(),
    osVersion(),
  ]);
  const summary = `${os} ${osVer} (${cpuArch}), v${appVersion}`;
  return {
    appVersion,
    os,
    arch: cpuArch,
    osVersion: osVer,
    summary,
  };
}

export interface BugReportPayload {
  name: string;
  email: string;
  message: string;
  steps?: string;
  env: BugReportEnv;
}

export type BugReportSubmitResult =
  | { ok: true }
  | { ok: false; error: string };

/** POST the report to Web3Forms (same flow as their ContactForm snippet). */
export async function submitBugReport(
  payload: BugReportPayload,
): Promise<BugReportSubmitResult> {
  const formData = new FormData();
  formData.append('access_key', WEB3FORMS_ACCESS_KEY);
  formData.append('subject', 'Dither Yuki Bug Report');
  formData.append('from_name', payload.name.trim() || 'Dither Yuki user');
  if (payload.email.trim()) {
    formData.append('email', payload.email.trim());
  }
  formData.append('message', payload.message.trim());
  if (payload.steps?.trim()) {
    formData.append('steps', payload.steps.trim());
  }
  formData.append('app_version', payload.env.appVersion);
  formData.append('os_info', payload.env.summary);

  try {
    const response = await fetch(WEB3FORMS_ENDPOINT, {
      method: 'POST',
      body: formData,
    });
    const data = (await response.json()) as { success?: boolean; message?: string };
    if (data.success) {
      return { ok: true };
    }
    return { ok: false, error: data.message || 'Could not send report.' };
  } catch {
    return { ok: false, error: 'Network error. Check your connection and try again.' };
  }
}

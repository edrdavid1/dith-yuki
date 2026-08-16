import { useCallback, useEffect, useRef, useState } from 'react';
import UpdateAvailableDialog from '../components/UpdateAvailableDialog';
import FileTooNewDialog from '../components/FileTooNewDialog';
import {
  checkForAppUpdate,
  getAppVersion,
  isReleaseBuild,
  relaunchApp,
  type AppUpdateHandle,
  type DownloadProgress,
  type UpdateCheckResult,
} from '../shared/ipc/updates';
import { confirmAndInstallUpdate, isTooNewFileError } from '../shared/appUpdates';

const LAUNCH_CHECK_DELAY_MS = 3000;

export function useAppUpdates(opts: {
  autoCheckOnLaunch: boolean;
  confirmRestart: () => Promise<boolean>;
  fileError?: string | null;
  onStatus?: (message: string, kind: 'success' | 'error') => void;
  clearFileError?: () => void;
}) {
  const [version, setVersion] = useState<string>('');
  const [checking, setChecking] = useState(false);
  const [available, setAvailable] = useState<AppUpdateHandle | null>(null);
  const [phase, setPhase] = useState<'prompt' | 'downloading'>('prompt');
  const [progress, setProgress] = useState<DownloadProgress>({
    contentLength: null,
    downloaded: 0,
  });
  const [installError, setInstallError] = useState<string | null>(null);
  const [tooNewOpen, setTooNewOpen] = useState(false);
  const [tooNewCheck, setTooNewCheck] = useState<UpdateCheckResult | null>(null);
  const cancelledRef = useRef(false);
  const confirmRestartRef = useRef(opts.confirmRestart);
  confirmRestartRef.current = opts.confirmRestart;
  const onStatusRef = useRef(opts.onStatus);
  onStatusRef.current = opts.onStatus;
  const clearFileErrorRef = useRef(opts.clearFileError);
  clearFileErrorRef.current = opts.clearFileError;

  useEffect(() => {
    void getAppVersion()
      .then(setVersion)
      .catch(() => setVersion(''));
  }, []);

  useEffect(() => {
    if (isTooNewFileError(opts.fileError)) {
      setTooNewOpen(true);
      setTooNewCheck(null);
    }
  }, [opts.fileError]);

  const runCheck = useCallback(async (): Promise<UpdateCheckResult> => {
    setChecking(true);
    try {
      return await checkForAppUpdate();
    } finally {
      setChecking(false);
    }
  }, []);

  const presentResult = useCallback((result: UpdateCheckResult, source: 'manual' | 'launch') => {
    if (result.status === 'available') {
      setAvailable(result.update);
      setPhase('prompt');
      setInstallError(null);
      return;
    }
    if (source === 'launch') return;
    if (result.status === 'none') {
      onStatusRef.current?.('You’re up to date.', 'success');
    } else {
      onStatusRef.current?.(result.message, 'error');
    }
  }, []);

  const checkForUpdates = useCallback(async () => {
    const result = await runCheck();
    presentResult(result, 'manual');
  }, [presentResult, runCheck]);

  useEffect(() => {
    if (!opts.autoCheckOnLaunch) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void (async () => {
        const release = await isReleaseBuild().catch(() => false);
        if (cancelled || !release) return;
        const result = await runCheck();
        if (cancelled) return;
        presentResult(result, 'launch');
      })();
    }, LAUNCH_CHECK_DELAY_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [opts.autoCheckOnLaunch, presentResult, runCheck]);

  const dismissAvailable = useCallback(() => {
    cancelledRef.current = true;
    setAvailable(null);
    setPhase('prompt');
    setInstallError(null);
  }, []);

  const install = useCallback(async (update: AppUpdateHandle) => {
    cancelledRef.current = false;
    setPhase('downloading');
    setInstallError(null);
    try {
      const outcome = await confirmAndInstallUpdate({
        confirmRestart: () => confirmRestartRef.current(),
        update,
        relaunch: relaunchApp,
        onProgress: setProgress,
        isCancelled: () => cancelledRef.current,
      });
      if (outcome === 'cancelled') {
        setPhase('prompt');
      }
    } catch (err) {
      setPhase('prompt');
      setInstallError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  const checkTooNew = useCallback(async () => {
    const result = await runCheck();
    setTooNewCheck(result);
  }, [runCheck]);

  const dialogs = (
    <>
      <UpdateAvailableDialog
        isOpen={available != null}
        version={available?.version ?? ''}
        notes={available?.notes ?? ''}
        phase={phase}
        downloaded={progress.downloaded}
        contentLength={progress.contentLength}
        error={installError}
        onLater={dismissAvailable}
        onInstall={() => {
          if (available) void install(available);
        }}
        onCancelDownload={dismissAvailable}
      />
      <FileTooNewDialog
        isOpen={tooNewOpen && isTooNewFileError(opts.fileError)}
        message={opts.fileError ?? ''}
        checking={checking}
        checkResult={tooNewCheck}
        onClose={() => {
          setTooNewOpen(false);
          clearFileErrorRef.current?.();
        }}
        onCheck={() => void checkTooNew()}
        onInstall={() => {
          if (tooNewCheck?.status !== 'available') return;
          setTooNewOpen(false);
          setAvailable(tooNewCheck.update);
          void install(tooNewCheck.update);
        }}
      />
    </>
  );

  return {
    version,
    checking,
    checkForUpdates,
    dialogs,
  };
}

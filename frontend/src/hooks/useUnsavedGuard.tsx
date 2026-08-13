import { useCallback, useRef, useState } from 'react';
import UnsavedGuardDialog from '../components/UnsavedGuardDialog';
import {
  confirmUnsavedIfNeeded,
  projectBasename,
  type UnsavedGuardChoice,
} from '../shared/unsavedGuard';

export function useUnsavedGuard(opts: {
  hasDocument: boolean;
  dirty: boolean;
  projectPath: string | null;
  save: () => Promise<boolean>;
}) {
  const [open, setOpen] = useState(false);
  const resolver = useRef<((choice: UnsavedGuardChoice) => void) | null>(null);

  const prompt = useCallback(() => {
    return new Promise<UnsavedGuardChoice>((resolve) => {
      resolver.current = resolve;
      setOpen(true);
    });
  }, []);

  const finish = useCallback((choice: UnsavedGuardChoice) => {
    setOpen(false);
    resolver.current?.(choice);
    resolver.current = null;
  }, []);

  const confirmReplace = useCallback(async () => {
    return confirmUnsavedIfNeeded({
      hasDocument: opts.hasDocument,
      dirty: opts.dirty,
      prompt,
      save: opts.save,
    });
  }, [opts.dirty, opts.hasDocument, opts.save, prompt]);

  const dialog = (
    <UnsavedGuardDialog
      isOpen={open}
      basename={projectBasename(opts.projectPath)}
      onSave={() => finish('save')}
      onDiscard={() => finish('discard')}
      onCancel={() => finish('cancel')}
    />
  );

  return { confirmReplace, dialog };
}

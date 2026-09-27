import { useCallback, useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import overlayStyles from '../features/document/NewProjectDialog.module.css';
import helpStyles from './HelpDialog.module.css';
import styles from './BugReportDialog.module.css';
import { bind } from '../shared/ui/cn';
import { DialogTitlebar } from '../shared/ui/WindowTitlebar';
import {
  gatherBugReportEnv,
  submitBugReport,
  type BugReportEnv,
} from '../shared/bugReport';

const cn = bind({ ...overlayStyles, ...helpStyles, ...styles });

export interface BugReportDialogProps {
  isOpen: boolean;
  onClose: () => void;
}

export default function BugReportDialog({ isOpen, onClose }: BugReportDialogProps) {
  const [env, setEnv] = useState<BugReportEnv | null>(null);
  const [name, setName] = useState('');
  const [email, setEmail] = useState('');
  const [message, setMessage] = useState('');
  const [steps, setSteps] = useState('');
  const [result, setResult] = useState('');
  const [sending, setSending] = useState(false);

  useEffect(() => {
    if (!isOpen) return;
    setResult('');
    setSending(false);
    let cancelled = false;
    void gatherBugReportEnv().then((info) => {
      if (!cancelled) setEnv(info);
    });
    return () => {
      cancelled = true;
    };
  }, [isOpen]);

  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget && !sending) onClose();
    },
    [onClose, sending],
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Escape' && !sending) {
        e.preventDefault();
        onClose();
      }
    },
    [onClose, sending],
  );

  const onSubmit = useCallback(
    async (event: React.FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      if (!env || !message.trim()) return;
      setSending(true);
      setResult('Sending…');
      const outcome = await submitBugReport({
        name,
        email,
        message,
        steps,
        env,
      });
      setSending(false);
      if (outcome.ok) {
        setResult('Sent — thank you.');
        setName('');
        setEmail('');
        setMessage('');
        setSteps('');
        onClose();
      } else {
        setResult(outcome.error);
      }
    },
    [email, env, message, name, onClose, steps],
  );

  if (!isOpen) return null;

  return createPortal(
    <div
      className={cn('new-project-overlay')}
      onClick={handleOverlayClick}
      data-testid="bug-report-overlay"
    >
      <div
        className={cn('new-project-dialog', 'bug-report-dialog')}
        role="dialog"
        aria-modal="true"
        aria-labelledby="bug-report-title"
        onKeyDown={handleKeyDown}
      >
        <DialogTitlebar title="Report a bug" titleId="bug-report-title" onClose={onClose} />
        <form className={cn('bug-report-body')} onSubmit={(e) => void onSubmit(e)}>
          <p className={cn('bug-report-lede')}>
            Sent to the developer by email. No account needed.
          </p>
          <p className={cn('bug-report-env')}>
            <span className={cn('bug-report-env-label')}>Will send:</span>
            {env?.summary ?? '…'}
          </p>

          <label className={cn('bug-report-field')}>
            <span>Name</span>
            <input
              className={cn('bug-report-input')}
              type="text"
              name="name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              autoComplete="name"
              disabled={sending}
            />
          </label>

          <label className={cn('bug-report-field')}>
            <span>
              Email <span className={cn('bug-report-optional')}>(optional)</span>
            </span>
            <input
              className={cn('bug-report-input')}
              type="email"
              name="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              autoComplete="email"
              disabled={sending}
            />
          </label>

          <label className={cn('bug-report-field')}>
            <span>What happened?</span>
            <textarea
              className={cn('bug-report-textarea')}
              name="message"
              value={message}
              onChange={(e) => setMessage(e.target.value)}
              rows={4}
              required
              disabled={sending}
            />
          </label>

          <label className={cn('bug-report-field')}>
            <span>
              Steps to reproduce{' '}
              <span className={cn('bug-report-optional')}>(optional)</span>
            </span>
            <textarea
              className={cn('bug-report-textarea')}
              name="steps"
              value={steps}
              onChange={(e) => setSteps(e.target.value)}
              rows={3}
              disabled={sending}
            />
          </label>

          <div className={cn('bug-report-actions')}>
            <button
              type="submit"
              className={cn('help-update-btn')}
              disabled={sending || !message.trim() || !env}
            >
              {sending ? 'Sending…' : 'Send Report'}
            </button>
          </div>
          <p
            className={cn(
              'bug-report-status',
              result.startsWith('Sent') && 'is-ok',
              result && !result.startsWith('Sent') && !result.startsWith('Sending') && 'is-err',
            )}
            role="status"
            aria-live="polite"
          >
            {result}
          </p>
        </form>
      </div>
    </div>,
    document.body,
  );
}

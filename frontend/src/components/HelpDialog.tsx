import { useCallback, useState } from 'react';
import { createPortal } from 'react-dom';
import SimpleBar from 'simplebar-react';
import overlayStyles from '../features/document/NewProjectDialog.module.css';
import styles from './HelpDialog.module.css';
import { bind } from '../shared/ui/cn';
import { DialogTitlebar } from '../shared/ui/WindowTitlebar';
import BugReportDialog from './BugReportDialog';
import {
  APP_COPYRIGHT,
  APP_DEVELOPER,
  APP_NAME,
} from '../shared/appMeta';
import {
  USER_AGREEMENT_BODY,
  USER_AGREEMENT_TITLE,
} from '../shared/userAgreement';

const cn = bind({ ...overlayStyles, ...styles });

export interface HelpDialogProps {
  isOpen: boolean;
  version: string;
  checking?: boolean;
  onClose: () => void;
  onCheckForUpdates?: () => void;
}

export default function HelpDialog({
  isOpen,
  version,
  checking = false,
  onClose,
  onCheckForUpdates,
}: HelpDialogProps) {
  const [reportOpen, setReportOpen] = useState(false);
  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) onClose();
    },
    [onClose],
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
      }
    },
    [onClose],
  );

  if (!isOpen && !reportOpen) return null;

  return (
    <>
      {isOpen
        ? createPortal(
            <div
              className={cn('new-project-overlay')}
              onClick={handleOverlayClick}
              data-testid="help-overlay"
            >
              <div
                className={cn('new-project-dialog', 'help-dialog')}
                role="dialog"
                aria-modal="true"
                aria-labelledby="help-title"
                onKeyDown={handleKeyDown}
              >
                <DialogTitlebar title="Help" titleId="help-title" onClose={onClose} />
                <div className={cn('help-body')}>
                  <img className={cn('help-logo')} src="/img/dith.png" alt="" />
                  <h2 className={cn('help-app-name')}>{APP_NAME}</h2>

                  <p className={cn('help-version')}>version {version || '…'}</p>
                  <p className={cn('help-developer')}>
                    <span>Developer:</span>
                    <img
                      className={cn('help-developer-mark')}
                      src="/icons/developer-icon.svg"
                      alt={APP_DEVELOPER}
                    />
                  </p>
                  <SimpleBar className={cn('help-license')} style={{ height: '280px' }}>
                    <div className={cn('help-license-inner')}>
                      <p className={cn('help-license-title')}>{USER_AGREEMENT_TITLE}</p>
                      <p className={cn('help-license-copy')}>{APP_COPYRIGHT}</p>
                      <pre className={cn('help-agreement-body')}>{USER_AGREEMENT_BODY}</pre>
                    </div>
                  </SimpleBar>
                  <div className={cn('help-actions')}>
                    {onCheckForUpdates && (
                      <button
                        type="button"
                        className={cn('help-update-btn')}
                        disabled={checking}
                        onClick={onCheckForUpdates}
                      >
                        {checking ? 'Checking…' : 'Check update'}
                      </button>
                    )}
                    <button
                      type="button"
                      className={cn('help-update-btn')}
                      onClick={() => {
                        onClose();
                        setReportOpen(true);
                      }}
                    >
                      Report a bug
                    </button>
                  </div>
                </div>
              </div>
            </div>,
            document.body,
          )
        : null}
      <BugReportDialog isOpen={reportOpen} onClose={() => setReportOpen(false)} />
    </>
  );
}

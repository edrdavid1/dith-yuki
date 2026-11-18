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
import { IS_DIRECT } from '../channel/flags';

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
                  <div className={cn('help-brand')}>
                    <img className={cn('help-logo')} src="/img/dith.png" alt="" />
                    <div className={cn('help-brand-text')}>
                      <h2 className={cn('help-app-name')}>{APP_NAME}</h2>
                      <p className={cn('help-version')}>version {version || '…'}</p>
                      <p className={cn('help-developer')}>
                        <span className={cn('help-developer-label')}>Developer:</span>
                        <img
                          className={cn('help-developer-mark')}
                          src="/icons/vasdas-icon.svg"
                          alt={APP_DEVELOPER}
                        />
                      </p>
                    </div>
                  </div>

                  <div className={cn('help-section')}>
                    <div className={cn('help-section-label')}>License</div>
                    <SimpleBar className={cn('help-license')}>
                      <div className={cn('help-license-inner')}>
                        <p className={cn('help-license-title')}>{USER_AGREEMENT_TITLE}</p>
                        <p className={cn('help-license-copy')}>{APP_COPYRIGHT}</p>
                        <pre className={cn('help-agreement-body')}>{USER_AGREEMENT_BODY}</pre>
                      </div>
                    </SimpleBar>
                  </div>

                  <div className={cn('help-actions')}>
                    {onCheckForUpdates && (
                      <button
                        type="button"
                        className={cn('help-btn')}
                        disabled={checking}
                        onClick={onCheckForUpdates}
                      >
                        {checking ? 'Checking…' : 'Check update'}
                      </button>
                    )}
                    {IS_DIRECT && (
                      <button
                        type="button"
                        className={cn('help-btn')}
                        onClick={() => {
                          onClose();
                          setReportOpen(true);
                        }}
                      >
                        Report a bug
                      </button>
                    )}
                    <button type="button" className={cn('help-btn')} onClick={onClose}>
                      Close
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

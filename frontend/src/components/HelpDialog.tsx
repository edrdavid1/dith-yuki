import { useCallback } from 'react';
import { createPortal } from 'react-dom';
import SimpleBar from 'simplebar-react';
import overlayStyles from '../features/document/NewProjectDialog.module.css';
import styles from './HelpDialog.module.css';
import { bind } from '../shared/ui/cn';
import { DialogTitlebar } from '../shared/ui/WindowTitlebar';
import {
  APP_COPYRIGHT,
  APP_DEVELOPER,
  APP_LICENSE_ID,
  APP_NAME,
  APP_TAGLINE,
} from '../shared/appMeta';

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
  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) onClose();
    },
    [onClose]
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
      }
    },
    [onClose]
  );

  if (!isOpen) return null;

  return createPortal(
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
          <p className={cn('help-tagline')}>{APP_TAGLINE}</p>
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
              <p className={cn('help-license-title')}>License ({APP_LICENSE_ID})</p>
              <p className={cn('help-license-copy')}>{APP_COPYRIGHT}</p>

              <section className={cn('help-license-section')}>
                <h3>1. Permission</h3>
                <p>
                  This software is provided free of charge for artists, designers,
                  illustrators, indie developers, non-profit organizations, and small
                  businesses (with fewer than 50 employees or annual revenue under
                  €1,000,000). These users are granted permission to use, copy, modify,
                  and distribute this software, including for commercial purposes, under
                  the following conditions.
                </p>
              </section>

              <section className={cn('help-license-section')}>
                <h3>2. Corporate Restriction</h3>
                <p>
                  Large corporations, defined as entities with more than 50 employees or
                  annual revenue exceeding €1,000,000, are not permitted to use, modify,
                  integrate, or distribute this software without the explicit written
                  consent of the author.
                </p>
              </section>

              <section className={cn('help-license-section')}>
                <h3>3. Attribution</h3>
                <p>
                  All copies or substantial portions of the software must include this
                  copyright notice and a link to the original project.
                </p>
              </section>

              <section className={cn('help-license-section')}>
                <h3>4. Warranty Disclaimer</h3>
                <p className={cn('help-license-warranty')}>
                  THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
                  OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
                  MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
                  IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY CLAIM, DAMAGES, OR OTHER
                  LIABILITY ARISING FROM THE USE OR DISTRIBUTION OF THE SOFTWARE.
                </p>
              </section>
            </div>
          </SimpleBar>
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
        </div>
      </div>
    </div>,
    document.body
  );
}

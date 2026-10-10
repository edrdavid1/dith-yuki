/**
 * Upgrade workspace prompt (FR-8 / §6.2). Modal; Escape and overlay are no-ops.
 */

import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from 'react';
import { createPortal } from 'react-dom';
import overlayStyles from '../document/NewProjectDialog.module.css';
import { bind } from '../../shared/ui/cn';
import { useWorkspace } from './WorkspaceContext';
import type { Workspace } from '../../shared/workspace/visibility';

const cn = bind(overlayStyles);

export default function WorkspacePromptDialog() {
  const { showWorkspacePrompt, resolvePrompt } = useWorkspace();
  const [remember, setRemember] = useState(false);
  const generalBtnRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (showWorkspacePrompt) {
      generalBtnRef.current?.focus();
    }
  }, [showWorkspacePrompt]);

  const choose = useCallback(
    (choice: Workspace) => {
      void resolvePrompt(choice, remember);
    },
    [remember, resolvePrompt],
  );

  const onKeyDown = useCallback((e: KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
    }
  }, []);

  if (!showWorkspacePrompt) return null;

  return createPortal(
    <div
      className={cn('new-project-overlay')}
      data-testid="workspace-prompt-overlay"
      onKeyDown={onKeyDown}
    >
      <div
        className={cn('new-project-dialog')}
        role="dialog"
        aria-modal="true"
        aria-labelledby="workspace-prompt-title"
      >
        <div className={cn('new-project-body')}>
          <h2 id="workspace-prompt-title">Choose your workspace</h2>
          <p>
            <strong>General</strong> — Dither images for web, print, and general
            graphics.
          </p>
          <p>
            <strong>Gamedev</strong> — Create seamless textures, tiles, and
            sprites.
          </p>
          <label className={cn('param-checkbox-row')} style={{ display: 'flex', gap: 8 }}>
            <input
              type="checkbox"
              checked={remember}
              onChange={(e) => setRemember(e.target.checked)}
            />
            Remember my choice
          </label>
          <div className={cn('new-project-actions')} style={{ display: 'flex', gap: 12, marginTop: 16 }}>
            <button
              ref={generalBtnRef}
              type="button"
              className={cn('new-project-primary')}
              onClick={() => choose('general')}
            >
              General
            </button>
            <button
              type="button"
              className={cn('new-project-secondary')}
              onClick={() => choose('gamedev')}
            >
              Gamedev
            </button>
          </div>
        </div>
      </div>
    </div>,
    document.body,
  );
}

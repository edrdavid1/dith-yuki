/**
 * Behavioral workspace hint (FR-9 / §6.3). Non-blocking shell toast; auto-dismiss 30s.
 * Shown when Batch (or tile export) runs in General — offers Switch → Gamedev.
 */

import Notification from '../../components/common/Notification';
import { useWorkspace } from './WorkspaceContext';

const AUTO_MS = 30_000;

const HINT_MESSAGE =
  "You're working with tiles. Enable Gamedev workspace for seamless preview and tiling tools.";

export default function WorkspaceHintBanner() {
  const { hintVisible, dismissHint, switchFromHint } = useWorkspace();

  return (
    <Notification
      message={hintVisible ? HINT_MESSAGE : null}
      actionLabel="Switch"
      onAction={() => {
        void switchFromHint();
      }}
      onDismiss={() => {
        void dismissHint('dismiss');
      }}
      duration={AUTO_MS}
      testId="workspace-hint-banner"
    />
  );
}

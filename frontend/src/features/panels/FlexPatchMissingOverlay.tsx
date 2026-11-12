/**
 * W3: fatal overlay in DEV when the flexlayout-react patch marker is missing.
 */

import { isFlexPatchApplied } from './flexlayoutPatchGate';

export default function FlexPatchMissingOverlay() {
  if (!import.meta.env.DEV) return null;
  if (isFlexPatchApplied()) return null;

  return (
    <div
      role="alert"
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 99999,
        background: 'rgba(20, 16, 12, 0.92)',
        color: '#f4efe6',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: 32,
        fontFamily: 'system-ui, sans-serif',
      }}
    >
      <div style={{ maxWidth: 520, lineHeight: 1.45 }}>
        <h1 style={{ fontSize: 20, margin: '0 0 12px' }}>
          FlexLayout patch missing
        </h1>
        <p style={{ margin: '0 0 12px', opacity: 0.9 }}>
          <code>flexlayout-react</code> is stock (no{' '}
          <code>__FLEX_PATCH__</code>). Float/popout will break (gray windows,
          shared names). This usually means{' '}
          <code>npm ci --ignore-scripts</code> or a skipped postinstall.
        </p>
        <p style={{ margin: 0, opacity: 0.85 }}>
          Fix: from <code>frontend/</code> run{' '}
          <code>npm install</code> (applies{' '}
          <code>patches/flexlayout-react+0.7.15.patch</code>), then{' '}
          <code>npm run verify:flexlayout-patch</code>.
        </p>
      </div>
    </div>
  );
}

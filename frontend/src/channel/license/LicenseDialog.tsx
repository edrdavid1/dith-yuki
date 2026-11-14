import { useCallback, useState } from 'react';
import { activateLicense, type Entitlement } from '../index';

type Props = {
  isOpen: boolean;
  onClose: () => void;
  onActivated?: (entitlement: Entitlement) => void;
};

/** Direct-channel only — tree-shaken from store builds via `IS_DIRECT` lazy import. */
export default function LicenseDialog({ isOpen, onClose, onActivated }: Props) {
  const [key, setKey] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const entitlement = await activateLicense(key.trim());
      if (entitlement.state === 'unlocked') {
        onActivated?.(entitlement);
        onClose();
      } else {
        setError(`License locked (${entitlement.reason})`);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, [key, onActivated, onClose]);

  if (!isOpen) return null;

  return (
    <div role="dialog" aria-modal="true" aria-labelledby="license-title">
      <h2 id="license-title">Activate license</h2>
      <input
        type="text"
        value={key}
        onChange={(e) => setKey(e.target.value)}
        placeholder="License key"
        disabled={busy}
        autoFocus
      />
      {error ? <p>{error}</p> : null}
      <button type="button" onClick={() => void submit()} disabled={busy || !key.trim()}>
        {busy ? 'Activating…' : 'Activate'}
      </button>
      <button type="button" onClick={onClose} disabled={busy}>
        Cancel
      </button>
    </div>
  );
}

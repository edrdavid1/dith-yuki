/** Direct-channel only — tree-shaken from store builds via `IS_DIRECT` lazy import. */
export default function UpdateBanner({
  version,
  onInstall,
  onDismiss,
}: {
  version: string;
  onInstall: () => void;
  onDismiss: () => void;
}) {
  return (
    <div role="status">
      <span>Update {version} available</span>
      <button type="button" onClick={onInstall}>
        Install
      </button>
      <button type="button" onClick={onDismiss}>
        Later
      </button>
    </div>
  );
}

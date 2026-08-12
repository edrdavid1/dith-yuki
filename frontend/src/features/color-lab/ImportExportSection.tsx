import Icon from '../../icons/iconRegistry';
import styles from './ColorLabWindow.module.css';
import buttonStyles from './ColorLabButtons.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind({ ...styles, ...buttonStyles });

export interface ImportExportSectionProps {
  canExport: boolean;
  onImport: () => void;
  onExport: (format?: string) => void;
}

export default function ImportExportSection({
  canExport,
  onImport,
  onExport,
}: ImportExportSectionProps) {
  return (
    <div className={cn("color-lab-column")}>
      <div style={{ height: '15px' }} />

      <button
        type="button"
        onClick={onImport}
        className={cn("color-lab-button")}
        style={{ height: '36px' }}
      >
        <Icon name="import" width={16} height={16} />
        Import palette file
      </button>

      <button
        type="button"
        onClick={() => onExport('gpl')}
        disabled={!canExport}
        className={cn("color-lab-button")}
        style={{ height: '36px' }}
      >
        <Icon name="export" width={16} height={16} />
        Export palette file
      </button>

      <div className={cn("color-lab-hint")}>formats: ASE, GPL, HEH/TXT, JSON</div>
    </div>
  );
}

import { useState, type ReactNode } from 'react';
import styles from './ColorLabWindow.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind(styles);

export interface ColorLabSectionProps {
  title: string;
  /** Start expanded. Omit / false → collapsed (same as ramps / harmonization). */
  defaultOpen?: boolean;
  /** Extra controls in the summary row (e.g. Soft proof On/Off). */
  summaryExtra?: ReactNode;
  children: ReactNode;
  className?: string;
}

/**
 * Shared Color Lab submenu: black-line separators + ▸/▾ collapse like harmonization.
 */
export default function ColorLabSection({
  title,
  defaultOpen = false,
  summaryExtra,
  children,
  className,
}: ColorLabSectionProps) {
  const [open, setOpen] = useState(defaultOpen);

  return (
    <details
      className={cn('color-lab-section', className)}
      open={open}
      onToggle={(e) => setOpen((e.currentTarget as HTMLDetailsElement).open)}
    >
      <summary className={cn('color-lab-section-summary')}>
        <span className={cn('color-lab-section-title')}>{title}</span>
        {summaryExtra ? (
          <span
            className={cn('color-lab-section-summary-extra')}
            onClick={(e) => e.preventDefault()}
            onKeyDown={(e) => e.stopPropagation()}
          >
            {summaryExtra}
          </span>
        ) : null}
      </summary>
      <div className={cn('color-lab-section-body')}>{children}</div>
    </details>
  );
}

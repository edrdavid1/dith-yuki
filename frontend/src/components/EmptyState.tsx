import styles from '../shared/ui/EmptyState.module.css';
import { bind } from '../shared/ui/cn';
const cn = bind(styles);

interface EmptyStateProps {
  className?: string;
}

function EmptyState({ className }: EmptyStateProps) {
  return (
    <div className={cn('empty-state', className)}>
      <span className={cn("empty-state-icon")}>📂</span>
      <span className={cn("empty-state-text")}>Перетащите файл или нажмите Открыть</span>
    </div>
  );
}

export default EmptyState;

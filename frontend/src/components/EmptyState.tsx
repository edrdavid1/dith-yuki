interface EmptyStateProps {
  className?: string;
}

function EmptyState({ className }: EmptyStateProps) {
  return (
    <div className={`empty-state ${className ?? ''}`}>
      <span className="empty-state-icon">📂</span>
      <span className="empty-state-text">Перетащите файл или нажмите Открыть</span>
    </div>
  );
}

export default EmptyState;

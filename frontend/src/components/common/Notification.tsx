import { useEffect, useState } from 'react';

interface NotificationProps {
  message: string | null;
  type?: 'error' | 'success';
  duration?: number; // ms, default 5000
  onDismiss?: () => void;
}

function Notification({ message, type = 'error', duration = 5000, onDismiss }: NotificationProps) {
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    if (message) {
      setVisible(true);
      const timer = setTimeout(() => {
        setVisible(false);
        onDismiss?.();
      }, duration);
      return () => clearTimeout(timer);
    } else {
      setVisible(false);
    }
  }, [message, duration, onDismiss]);

  if (!visible || !message) return null;

  return (
    <div className={`notification notification-${type}`}>
      <span className="notification-text">{message}</span>
      <button className="notification-close" onClick={() => { setVisible(false); onDismiss?.(); }}>×</button>
    </div>
  );
}

export default Notification;

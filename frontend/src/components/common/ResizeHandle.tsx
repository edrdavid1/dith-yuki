import { useCallback, useRef } from 'react';
import styles from '../../shared/ui/ResizeHandle.module.css';
import { bind } from '../../shared/ui/cn';
const cn = bind(styles);


interface ResizeHandleProps {
  /** 'horizontal' resizes width (drag left/right), 'vertical' resizes height (drag up/down) */
  direction: 'horizontal' | 'vertical';
  /** Called with pixel delta during drag */
  onResize: (delta: number) => void;
  className?: string;
}

/**
 * A draggable handle for resizing panels.
 * Renders a thin bar that can be dragged to resize adjacent panels.
 */
export default function ResizeHandle({ direction, onResize, className = '' }: ResizeHandleProps) {
  const startPos = useRef(0);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    startPos.current = direction === 'horizontal' ? e.clientX : e.clientY;

    const handleMouseMove = (moveEvent: MouseEvent) => {
      const currentPos = direction === 'horizontal' ? moveEvent.clientX : moveEvent.clientY;
      const delta = currentPos - startPos.current;
      startPos.current = currentPos;
      onResize(delta);
    };

    const handleMouseUp = () => {
      window.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('mouseup', handleMouseUp);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
    };

    document.body.style.cursor = direction === 'horizontal' ? 'col-resize' : 'row-resize';
    document.body.style.userSelect = 'none';
    window.addEventListener('mousemove', handleMouseMove);
    window.addEventListener('mouseup', handleMouseUp);
  }, [direction, onResize]);

  const cursorClass = direction === 'horizontal' ? 'resize-handle-h' : 'resize-handle-v';

  return (
    <div
      className={cn('resize-handle', cursorClass, className)}
      onMouseDown={handleMouseDown}
    />
  );
}

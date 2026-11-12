/**
 * W9: thin React adapter around TitlebarGestureController.
 */

import { useCallback, useEffect, useRef } from 'react';
import { TitlebarGestureController } from './titlebarGestureController';
import type { TitlebarGestureMode } from './titlebarGestureMachine';

export function useTitlebarGestureController(options: {
  columnRef: React.RefObject<HTMLElement | null>;
  onUndock: () => void;
  mode?: TitlebarGestureMode;
}): (e: React.MouseEvent) => void {
  const columnRef = useRef(options.columnRef);
  columnRef.current = options.columnRef;
  const onUndockRef = useRef(options.onUndock);
  onUndockRef.current = options.onUndock;
  const modeRef = useRef(options.mode ?? 'sidebar');
  modeRef.current = options.mode ?? 'sidebar';

  const controllerRef = useRef<TitlebarGestureController | null>(null);
  if (!controllerRef.current) {
    controllerRef.current = new TitlebarGestureController({
      getColumnEl: () => columnRef.current.current,
      getMode: () => modeRef.current,
      onUndock: () => onUndockRef.current(),
    });
  }

  useEffect(() => {
    return () => controllerRef.current?.dispose();
  }, []);

  return useCallback((e: React.MouseEvent) => {
    controllerRef.current?.onTitlebarPointerDown({
      button: e.button,
      clientX: e.clientX,
      clientY: e.clientY,
      target: e.target,
    });
  }, []);
}

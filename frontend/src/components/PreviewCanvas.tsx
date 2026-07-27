import { useRef, useEffect, useState } from 'react';
import { computeFitToView } from '../hooks/usePreview';

interface PreviewCanvasProps {
  previewSrc: string | null;
  isRendering: boolean;
  imgWidth: number;
  imgHeight: number;
}

function PreviewCanvas({ previewSrc, isRendering, imgWidth, imgHeight }: PreviewCanvasProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 });

  // ResizeObserver with 200ms debounce
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    let timeoutId: ReturnType<typeof setTimeout>;
    const observer = new ResizeObserver(() => {
      clearTimeout(timeoutId);
      timeoutId = setTimeout(() => {
        setContainerSize({
          width: container.clientWidth,
          height: container.clientHeight,
        });
      }, 200);
    });

    observer.observe(container);
    // Set initial size
    setContainerSize({ width: container.clientWidth, height: container.clientHeight });

    return () => {
      clearTimeout(timeoutId);
      observer.disconnect();
    };
  }, []);

  const displaySize = computeFitToView(imgWidth, imgHeight, containerSize.width, containerSize.height);

  return (
    <div ref={containerRef} className="preview-container">
      {previewSrc && (
        <img
          src={previewSrc}
          alt="Preview"
          className="preview-image"
          style={{
            width: displaySize.width || 'auto',
            height: displaySize.height || 'auto',
          }}
        />
      )}
      {isRendering && (
        <div className="preview-loading">
          <div className="spinner" />
        </div>
      )}
    </div>
  );
}

export default PreviewCanvas;

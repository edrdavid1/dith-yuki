import './App.css';
import Toolbar from './components/Toolbar';
import ZoomControls from './components/ZoomControls';
import EmptyState from './components/EmptyState';
import FilterList from './components/FilterList';
import FilterPanel from './components/FilterPanel';
import LayerPanel from './components/LayerPanel';
import LayerControls from './components/LayerControls';
import TileCanvas from './components/TileCanvas';
import type { ViewportState } from './components/TileCanvas';
import Notification from './components/common/Notification';
import { useDocument } from './hooks/useDocument';
import { useFilters } from './hooks/useFilters';
import { useLayers } from './hooks/useLayers';
import { usePan } from './hooks/usePan';
import { useViewport } from './hooks/useViewport';
import { useState, useCallback, useEffect } from 'react';
import type { LayerNodeDto } from './components/LayerPanel';

/** Recursively find a layer by ID in the layer tree. */
function findLayerById(layers: LayerNodeDto[], id: number): LayerNodeDto | null {
  for (const layer of layers) {
    if (layer.id === id) return layer;
    if (layer.children) {
      const found = findLayerById(layer.children, id);
      if (found) return found;
    }
  }
  return null;
}

function App() {
  const doc = useDocument();
  // Placeholder refresh — will be replaced by tile-ready event handling
  const refreshNoop = useCallback(() => {}, []);
  const filters = useFilters(doc.layerId, refreshNoop);
  const layerState = useLayers({ docId: doc.docId });
  const [dismissedError, setDismissedError] = useState<string | null>(null);

  // Viewport state managed by useViewport hook (calls set_viewport IPC on changes)
  const { viewport, handleWheel, handlePanDrag, fitToView, setZoom, setCanvasSize } = useViewport(doc.width, doc.height);

  const handleViewportChange = useCallback((_vp: ViewportState) => {
    // The useViewport hook manages state internally and syncs with backend
  }, []);

  // Zoom controls handlers
  const handleZoomChange = useCallback((newZoom: number) => {
    setZoom(newZoom);
  }, [setZoom]);

  const handleFitToView = useCallback(() => {
    fitToView();
  }, [fitToView]);

  // Wire pan behavior to the canvas container
  const { containerRef: panContainerRef } = usePan({ onPanDrag: handlePanDrag });

  // Measure the preview container and update viewport canvas dimensions
  useEffect(() => {
    const el = panContainerRef.current;
    if (!el) return;
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const { width, height } = entry.contentRect;
        if (width > 0 && height > 0) {
          setCanvasSize(width, height);
        }
      }
    });
    observer.observe(el);
    // Initial measurement
    const rect = el.getBoundingClientRect();
    if (rect.width > 0 && rect.height > 0) {
      setCanvasSize(rect.width, rect.height);
    }
    return () => observer.disconnect();
  }, [setCanvasSize]);

  // Aggregate errors from all hooks for display
  const currentError = doc.error || filters.error || layerState.error;
  // Only show if the error hasn't been dismissed
  const displayError = currentError && currentError !== dismissedError ? currentError : null;

  return (
    <div className="app-layout">
      <div className="app-toolbar">
        <Toolbar
          onOpen={doc.openImage}
          onSave={doc.saveImage}
          hasDocument={doc.hasDocument}
        />
        {doc.hasDocument && (
          <ZoomControls
            zoom={viewport.zoom}
            onZoomChange={handleZoomChange}
            onFitToView={handleFitToView}
          />
        )}
      </div>

      <div className="app-canvas">
        {doc.hasDocument ? (
          <div className="preview-container" ref={panContainerRef as React.RefObject<HTMLDivElement>} tabIndex={0} onWheel={(e) => handleWheel(e.nativeEvent)}>
            <TileCanvas
              docId={doc.docId!}
              docWidth={doc.width}
              docHeight={doc.height}
              viewport={viewport}
              onViewportChange={handleViewportChange}
            />
          </div>
        ) : (
          <EmptyState />
        )}
      </div>

      <div className="app-sidebar">
        <LayerPanel
          layers={layerState.layers}
          selectedLayerId={layerState.selectedLayerId}
          onSelect={layerState.setSelectedLayerId}
          onReorder={layerState.reorderLayer}
          onPropsChange={layerState.setLayerProps}
          onAddLayer={layerState.addLayer}
        />
        {layerState.selectedLayerId !== null && (() => {
          const selectedLayer = findLayerById(layerState.layers, layerState.selectedLayerId);
          return selectedLayer ? (
            <LayerControls
              layer={selectedLayer}
              onPropsChange={layerState.setLayerProps}
            />
          ) : null;
        })()}
        <FilterList
          filters={filters.filters}
          activeFilterId={filters.activeFilterId}
          onAdd={filters.addFilter}
          onRemove={filters.removeFilter}
          onSelect={filters.setActiveFilterId}
        />
        {filters.activeFilter && (
          <div className="sidebar-section">
            <h3 className="sidebar-title">{filters.activeFilter.kind} Parameters</h3>
            <FilterPanel
              filter={filters.activeFilter}
              onUpdate={filters.updateFilterParams}
            />
          </div>
        )}
      </div>

      <Notification
        message={displayError}
        type="error"
        onDismiss={() => setDismissedError(currentError)}
      />

      <Notification
        message={doc.notification}
        type="success"
        onDismiss={doc.clearNotification}
      />
    </div>
  );
}

export default App;

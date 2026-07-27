import './App.css';
import Toolbar from './components/Toolbar';
import EmptyState from './components/EmptyState';
import PreviewCanvas from './components/PreviewCanvas';
import FilterList from './components/FilterList';
import FilterPanel from './components/FilterPanel';
import Notification from './components/common/Notification';
import { useDocument } from './hooks/useDocument';
import { usePreview } from './hooks/usePreview';
import { useFilters } from './hooks/useFilters';
import { useState } from 'react';

function App() {
  const doc = useDocument();
  const preview = usePreview(doc.docId);
  const filters = useFilters(doc.layerId, preview.refresh);
  const [dismissedError, setDismissedError] = useState<string | null>(null);

  // Aggregate errors from all hooks for display
  const currentError = doc.error || preview.error || filters.error;
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
      </div>

      <div className="app-canvas">
        {doc.hasDocument ? (
          <PreviewCanvas
            previewSrc={preview.previewSrc}
            isRendering={preview.isRendering}
            imgWidth={doc.width}
            imgHeight={doc.height}
          />
        ) : (
          <EmptyState />
        )}
      </div>

      <div className="app-sidebar">
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

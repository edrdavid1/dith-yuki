import type { FilterInfo, FilterKind } from '../types';

interface SidebarProps {
  filters: FilterInfo[];
  activeFilterId: string | null;
  onAddFilter: (kind: FilterKind) => void;
  onRemoveFilter: (filterId: string) => void;
  onSelectFilter: (filterId: string) => void;
  onUpdateParams: (filterId: string, params: Record<string, unknown>) => void;
  activeFilter: FilterInfo | null;
}

function Sidebar({
  filters,
  activeFilterId,
  onAddFilter,
  onRemoveFilter,
  onSelectFilter,
  onUpdateParams,
  activeFilter,
}: SidebarProps) {
  return (
    <div className="sidebar-content">
      <div className="sidebar-section">
        <h3 className="sidebar-title">Filters</h3>
        <div className="filter-add-buttons">
          <button className="filter-add-btn" onClick={() => onAddFilter('Dither')}>+ Dither</button>
          <button className="filter-add-btn" onClick={() => onAddFilter('Curves')}>+ Curves</button>
          <button className="filter-add-btn" onClick={() => onAddFilter('Levels')}>+ Levels</button>
          <button className="filter-add-btn" onClick={() => onAddFilter('Glitch')}>+ Glitch</button>
        </div>
      </div>

      {filters.length > 0 && (
        <div className="sidebar-section">
          <h3 className="sidebar-title">Applied</h3>
          <ul className="filter-list">
            {filters.map((filter) => (
              <li
                key={filter.id}
                className={`filter-item ${filter.id === activeFilterId ? 'active' : ''}`}
                onClick={() => onSelectFilter(filter.id)}
              >
                <span className="filter-name">{filter.kind}</span>
                <button
                  className="filter-remove-btn"
                  onClick={(e) => { e.stopPropagation(); onRemoveFilter(filter.id); }}
                  aria-label={`Remove ${filter.kind} filter`}
                >
                  ×
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      {activeFilter && (
        <div className="sidebar-section">
          <h3 className="sidebar-title">{activeFilter.kind} Parameters</h3>
          {/* FilterPanel will be rendered here by parent or in a later task */}
        </div>
      )}
    </div>
  );
}

export default Sidebar;

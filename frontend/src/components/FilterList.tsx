import type { FilterInfo, FilterKind } from '../types';

interface FilterListProps {
  filters: FilterInfo[];
  activeFilterId: string | null;
  onAdd: (kind: FilterKind) => void;
  onRemove: (filterId: string) => void;
  onSelect: (filterId: string) => void;
}

function FilterList({ filters, activeFilterId, onAdd, onRemove, onSelect }: FilterListProps) {
  return (
    <>
      <div className="sidebar-section">
        <h3 className="sidebar-title">Add Filter</h3>
        <div className="filter-add-buttons">
          <button className="filter-add-btn" onClick={() => onAdd('Dither')}>+ Dither</button>
          <button className="filter-add-btn" onClick={() => onAdd('Curves')}>+ Curves</button>
          <button className="filter-add-btn" onClick={() => onAdd('Levels')}>+ Levels</button>
          <button className="filter-add-btn" onClick={() => onAdd('Glitch')}>+ Glitch</button>
        </div>
      </div>

      {filters.length > 0 && (
        <div className="sidebar-section">
          <h3 className="sidebar-title">Applied Filters</h3>
          <ul className="filter-list">
            {filters.map((filter) => (
              <li
                key={filter.id}
                className={`filter-item ${filter.id === activeFilterId ? 'active' : ''}`}
                onClick={() => onSelect(filter.id)}
              >
                <span className="filter-name">{filter.kind}</span>
                <button
                  className="filter-remove-btn"
                  onClick={(e) => { e.stopPropagation(); onRemove(filter.id); }}
                  aria-label={`Remove ${filter.kind} filter`}
                >
                  ×
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </>
  );
}

export default FilterList;

import { useCallback, useState, useRef } from 'react';

// ─── Types ────────────────────────────────────────────────────────────────────

export interface LayerNodeDto {
  id: number;
  name: string;
  kind: 'raster' | 'adjustment' | 'group';
  blend_mode: string;
  opacity: number;
  visible: boolean;
  children?: LayerNodeDto[];
}

export interface LayerPropsPatch {
  name?: string;
  opacity?: number;
  blend_mode?: string;
  visible?: boolean;
}

export interface LayerPanelProps {
  layers: LayerNodeDto[];
  selectedLayerId: number | null;
  onSelect: (id: number) => void;
  onReorder: (layerId: number, newParent: number | null, newIndex: number) => void;
  onPropsChange: (layerId: number, patch: LayerPropsPatch) => void;
  onAddLayer: () => void;
}

// ─── Constants ────────────────────────────────────────────────────────────────

const INDENT_PX = 16;
const THUMBNAIL_SIZE = 32;

/** Map layer kind to a representative color for placeholder thumbnails. */
const KIND_COLORS: Record<LayerNodeDto['kind'], string> = {
  raster: '#5a7a9a',
  adjustment: '#8a6a9a',
  group: '#6a8a6a',
};

/** Map layer kind to a display icon character. */
const KIND_ICONS: Record<LayerNodeDto['kind'], string> = {
  raster: '🖼',
  adjustment: '⚙',
  group: '📁',
};

// ─── Drag-and-Drop State ──────────────────────────────────────────────────────

interface DragState {
  /** ID of the layer being dragged. */
  sourceId: number;
  /** Visual index (in reversed/top-first order) where the drop indicator is shown. */
  dropIndex: number | null;
}

// ─── LayerTreeNode (recursive, draggable) ─────────────────────────────────────

interface LayerTreeNodeProps {
  node: LayerNodeDto;
  depth: number;
  selectedId: number | null;
  onSelect: (id: number) => void;
  /** Flat visual index of this node (used for drop-position calculation). */
  visualIndex: number;
  dragState: DragState | null;
  onDragStart: (id: number) => void;
  onDragOverIndex: (visualIndex: number) => void;
  onDrop: () => void;
  onDragEnd: () => void;
}

function LayerTreeNode({
  node,
  depth,
  selectedId,
  onSelect,
  visualIndex,
  dragState,
  onDragStart,
  onDragOverIndex,
  onDrop,
  onDragEnd,
}: LayerTreeNodeProps) {
  const isSelected = node.id === selectedId;
  const paddingLeft = depth * INDENT_PX + 8;
  const isDragSource = dragState?.sourceId === node.id;

  const handleClick = useCallback(() => {
    onSelect(node.id);
  }, [onSelect, node.id]);

  const handleDragStart = useCallback((e: React.DragEvent) => {
    e.dataTransfer.effectAllowed = 'move';
    e.dataTransfer.setData('text/plain', String(node.id));
    onDragStart(node.id);
  }, [node.id, onDragStart]);

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    // Determine if we're in the top or bottom half of this node
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const midY = rect.top + rect.height / 2;
    if (e.clientY < midY) {
      onDragOverIndex(visualIndex);
    } else {
      onDragOverIndex(visualIndex + 1);
    }
  }, [visualIndex, onDragOverIndex]);

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    onDrop();
  }, [onDrop]);

  const handleDragEnd = useCallback(() => {
    onDragEnd();
  }, [onDragEnd]);

  // Show drop indicator above this node if the drop index matches
  const showDropIndicatorAbove = dragState != null && dragState.dropIndex === visualIndex;

  return (
    <>
      {showDropIndicatorAbove && (
        <div className="layer-drop-indicator" aria-hidden="true" />
      )}
      <div
        className={`layer-tree-node${isSelected ? ' layer-tree-node-selected' : ''}${isDragSource ? ' layer-tree-node-dragging' : ''}`}
        style={{ paddingLeft }}
        onClick={handleClick}
        role="treeitem"
        aria-selected={isSelected}
        aria-label={`Layer: ${node.name}`}
        tabIndex={0}
        draggable
        onDragStart={handleDragStart}
        onDragOver={handleDragOver}
        onDrop={handleDrop}
        onDragEnd={handleDragEnd}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault();
            handleClick();
          }
        }}
      >
        {/* 32×32 placeholder thumbnail */}
        <div
          className="layer-thumbnail"
          style={{
            width: THUMBNAIL_SIZE,
            height: THUMBNAIL_SIZE,
            backgroundColor: KIND_COLORS[node.kind],
          }}
          aria-hidden="true"
        />

        {/* Kind icon + layer name */}
        <span className="layer-kind-icon" aria-hidden="true">
          {KIND_ICONS[node.kind]}
        </span>
        <span className="layer-name">{node.name}</span>
      </div>

      {/* Render group children recursively (also in bottom-to-top order) */}
      {node.kind === 'group' && node.children && (
        [...node.children].reverse().map((child, i) => (
          <LayerTreeNode
            key={child.id}
            node={child}
            depth={depth + 1}
            selectedId={selectedId}
            onSelect={onSelect}
            visualIndex={visualIndex + 1 + i}
            dragState={dragState}
            onDragStart={onDragStart}
            onDragOverIndex={onDragOverIndex}
            onDrop={onDrop}
            onDragEnd={onDragEnd}
          />
        ))
      )}
    </>
  );
}

// ─── LayerPanel Component ─────────────────────────────────────────────────────

/**
 * LayerPanel displays the document's layer tree structure.
 * Layers are shown in bottom-to-top visual order (topmost layer at the top).
 * Each layer shows a 32×32 placeholder thumbnail, kind icon, and name.
 * Clicking a layer selects it.
 *
 * Drag-and-drop reordering is implemented with HTML5 drag-and-drop API.
 * For MVP, only flat reordering within the root list is supported.
 * A 2px blue drop indicator line shows the target position during drag.
 * On drop, `onReorder(layerId, targetParent, targetIndex)` is called.
 * If the IPC call fails, the parent component should revert by re-fetching the layer tree.
 */
export default function LayerPanel({
  layers,
  selectedLayerId,
  onSelect,
  onAddLayer,
  onReorder,
  onPropsChange: _onPropsChange,
}: LayerPanelProps) {
  const [dragState, setDragState] = useState<DragState | null>(null);
  const treeRef = useRef<HTMLDivElement>(null);

  // The layers are displayed in reversed order (top-first = reversed from the data array).
  // Visual index 0 = top of the panel = last element in the `layers` array.
  const reversedLayers = [...layers].reverse();

  const handleDragStart = useCallback((id: number) => {
    setDragState({ sourceId: id, dropIndex: null });
  }, []);

  const handleDragOverIndex = useCallback((visualIndex: number) => {
    setDragState(prev => {
      if (!prev) return prev;
      if (prev.dropIndex === visualIndex) return prev;
      return { ...prev, dropIndex: visualIndex };
    });
  }, []);

  const handleDrop = useCallback(() => {
    if (!dragState || dragState.dropIndex == null) {
      setDragState(null);
      return;
    }

    const { sourceId, dropIndex } = dragState;

    // Convert visual drop index back to the actual index in the original (non-reversed) layers array.
    // Visual order is reversed: visualIndex 0 = top = layers[layers.length - 1].
    // A drop at visual index `i` means inserting before the i-th visual item.
    // In the original array (bottom-to-top), this maps to:
    //   targetIndex = layers.length - dropIndex
    // This gives us the index within the root list for the `reorder_layer` IPC call.

    // Find the source's current index in the original array
    const sourceOriginalIndex = layers.findIndex(l => l.id === sourceId);
    if (sourceOriginalIndex === -1) {
      setDragState(null);
      return;
    }

    // Target index in the original (non-reversed) array
    let targetIndex = layers.length - dropIndex;

    // Adjust: if dragging downward in the original array (upward visually),
    // account for the removal of the source item
    if (sourceOriginalIndex < targetIndex) {
      targetIndex -= 1;
    }

    // Clamp to valid range
    targetIndex = Math.max(0, Math.min(layers.length - 1, targetIndex));

    // Don't reorder if the target is the same position
    if (targetIndex !== sourceOriginalIndex) {
      // For MVP: flat reordering within root (parent = null)
      onReorder(sourceId, null, targetIndex);
    }

    setDragState(null);
  }, [dragState, layers, onReorder]);

  const handleDragEnd = useCallback(() => {
    setDragState(null);
  }, []);

  // Handle dragover on the tree container to allow dropping after the last item
  const handleTreeDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
  }, []);

  const handleTreeDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    // If dropping on the container (not on a specific node), place at the end (bottom visually)
    if (dragState && dragState.dropIndex == null) {
      setDragState(prev => prev ? { ...prev, dropIndex: reversedLayers.length } : prev);
    }
    handleDrop();
  }, [dragState, reversedLayers.length, handleDrop]);

  return (
    <div className="layer-panel" role="tree" aria-label="Layer tree">
      <div className="layer-panel-header">
        <span className="layer-panel-title">Layers</span>
        <button
          className="layer-add-btn"
          onClick={onAddLayer}
          title="Add new layer"
          aria-label="Add new layer"
        >
          + Layer
        </button>
      </div>

      <div
        className="layer-tree"
        ref={treeRef}
        onDragOver={handleTreeDragOver}
        onDrop={handleTreeDrop}
      >
        {/* Reverse for bottom-to-top visual order: topmost layer appears first */}
        {reversedLayers.map((node, visualIndex) => (
          <LayerTreeNode
            key={node.id}
            node={node}
            depth={0}
            selectedId={selectedLayerId}
            onSelect={onSelect}
            visualIndex={visualIndex}
            dragState={dragState}
            onDragStart={handleDragStart}
            onDragOverIndex={handleDragOverIndex}
            onDrop={handleDrop}
            onDragEnd={handleDragEnd}
          />
        ))}
        {/* Drop indicator after the last item */}
        {dragState != null && dragState.dropIndex === reversedLayers.length && (
          <div className="layer-drop-indicator" aria-hidden="true" />
        )}
      </div>
    </div>
  );
}
